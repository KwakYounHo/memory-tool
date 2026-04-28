use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{Value, json};
use std::path::Path;

const MEMORY_TOOL_URL: &str = "http://localhost:7080";
const MAX_LIST_ENTRISE: usize = 200;
const MAX_FILE_BYTES: usize = 100_000;

pub fn tool_defs() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "search_memory",
                "description": "Semantic vector search over the user's personal memory database. Returns top-K chunks ranked by embedding similarity (lower distance = more similar). Call this BEFORE answering when the response depends on prior context, decisions, preferences, or facts.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Natural language describing the meaning you want to retrieve — not literal keywords. Phrase it as the meaning of what you're looking for. Multilingual queries are supported."
                        },
                        "top_k": { "type": "integer", "default": 5 },
                        "filter": {
                            "type": "object",
                            "properties": {
                                "scope": { "type": "array", "items": { "type": "string", "enum": ["agent", "user"] } },
                                "kind": { "type": "array", "items": { "type": "string", "enum": ["rule", "feedback", "reflection", "reference", "knowledge", "memory", "note"] } },
                                "project": { "type": "string" },
                                "machine": { "type": "string" },
                            }
                        }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "add_memory",
                "description": "Ingest a file into the user's memory database. The file referenced by `source` is read by the server, embedded as a vector, and stored alongside its metadata. Your role is to determine the metadata (scope, kind, project) — not to transcribe the file content. Call this after using read_file to inspect the content for classification.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "source":  {
                            "type": "string",
                            "description": "File URI for the memory's source content. Must start with 'file://' followed by an absolute path. The server reads this file directly from disk."
                        },
                        "scope":   { "type": "string", "enum": ["agent", "user"] },
                        "kind":    { "type": "string", "enum": ["rule", "feedback", "reflection", "reference", "knowledge", "memory", "note"] },
                        "project": { "type": "string" },
                        "machine": { "type": "string" }
                    },
                    "required": ["source"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "list_directory",
                "description": "List files and subdirectories at the given path. Returns names with type and size.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read the contents of a text file. Truncates very large files.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }
            }
        }),
    ]
}

pub async fn execute_tool(client: &Client, name: &str, args_json: &str) -> Result<String> {
    let args: Value =
        serde_json::from_str(args_json).with_context(|| format!("parse arguments for {}", name))?;

    match name {
        "search_memory" => http_proxy(client, "/search_memory", &args).await,
        "add_memory" => http_proxy(client, "/add_memory", &args).await,
        "list_directory" => exec_list(&args),
        "read_file" => exec_read(&args),
        other => Err(anyhow::anyhow!("unknown  tool: {}", other)),
    }
}

async fn http_proxy(client: &Client, path: &str, args: &Value) -> Result<String> {
    let url = format!("{}{}", MEMORY_TOOL_URL, path);
    Ok(client
        .post(&url)
        .json(args)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?)
}

fn exec_list(args: &Value) -> Result<String> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .context("list_direectory requires 'path'")?;
    let p = Path::new(path);
    if !p.is_dir() {
        anyhow::bail!("not a directory: {}", path);
    }
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(p)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        let kind = if meta.is_dir() {
            "dir"
        } else if meta.is_file() {
            "file"
        } else {
            "other"
        };
        entries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "type": kind,
            "size": meta.len()
        }));
        if entries.len() >= MAX_LIST_ENTRISE {
            break;
        }
    }
    Ok(serde_json::to_string(&json!({ "entries": entries }))?)
}

fn exec_read(args: &Value) -> Result<String> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .context("read_file requires 'path'")?;
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path))?;
    let truncated = bytes.len() > MAX_FILE_BYTES;
    let content = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_FILE_BYTES)]).into_owned();
    Ok(serde_json::to_string(&json!({
        "content": content,
        "truncated": truncated,
        "total_bytes": bytes.len()
    }))?)
}
