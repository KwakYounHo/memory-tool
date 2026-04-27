use memory_tool::model::{CHAT_MODEL};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, Write};
use std::path::Path;

const OLLAMA_CHAT_URL: &str = "http://localhost:11434/v1/chat/completions";
const MEMORY_TOOL_URL: &str = "http://localhost:7080";
const MAX_LIST_ENTRISE: usize = 200;
const MAX_FILE_BYTES: usize = 100_000;

// -- Wire types --------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Message {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: FunctionCall,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct FunctionCall {
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
    tools: &'a [Value],
    stream: bool,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

// -- Wire types --------------------------------------------------

fn tool_defs() -> Vec<Value> {
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
                                "kind": { "type": "array", "items": { "type": "string", "enum": ["rule", "feedback", "reflection", "reference", "memory", "note"] } },
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
                "description": "Persist a new memory chunk. The `text` is embedded as a vector and stored alongside its metadata for future semantic retrieval. Use when the user explicitly asks to save content, or when classifying/ingesting a file.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "source":  {
                            "type": "string",
                            "description": "Provenance identifier. For files, use a path or 'file://' URI. For chat-derived memories, use 'chat://<date>'. The `text` must actually originate from this source."
                        },
                        "text":    {
                            "type": "string",
                            "description": "The verbatim content to embed and store. MUST contain ONLY the data being saved — no narration, status announcements, or self-commentary. If transforming (summarizing, extracting), include only the transformation result."
                        },
                        "scope":   { "type": "string", "enum": ["agent", "user"] },
                        "kind":    { "type": "string", "enum": ["rule", "feedback", "reflection", "reference", "memory", "note"] },
                        "project": { "type": "string" },
                        "machine": { "type": "string" }
                    },
                    "required": ["source", "text"]
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

// -- Tool dispatch --------------------------------------------------

async fn execute_tool(client: &Client, name: &str, args_json: &str) -> Result<String> {
    let args: Value = serde_json::from_str(args_json)
        .with_context(|| format!("parse arguments for {}", name))?;

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
    Ok(client.post(&url).json(args).send().await?
        .error_for_status()?
        .text().await?)
}

fn exec_list(args: &Value) -> Result<String> {
    let path = args.get("path").and_then(Value::as_str)
        .context("list_direectory requires 'path'")?;
    let p = Path::new(path);
    if !p.is_dir() {
        anyhow::bail!("not a directory: {}", path);
    }
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(p)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        let kind = if meta.is_dir() { "dir" } else if meta.is_file() { "file" } else { "other" };
        entries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "type": kind,
            "size": meta.len()
        }));
        if entries.len() >= MAX_LIST_ENTRISE { break; }
    }
    Ok(serde_json::to_string(&json!({ "entries": entries }))?)
}

fn exec_read(args: &Value) -> Result<String> {
    let path = args.get("path").and_then(Value::as_str)
        .context("read_file requires 'path'")?;
    let bytes = std::fs::read(path)
        .with_context(|| format!("read {}", path))?;
    let truncated = bytes.len() > MAX_FILE_BYTES;
    let content = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_FILE_BYTES)]).into_owned();
    Ok(serde_json::to_string(&json!({
        "content": content,
        "truncated": truncated,
        "total_bytes": bytes.len()
    }))?)
}

// -- Agent loop --------------------------------------------------

async fn agent_turn(client: &Client, messages: &mut Vec<Message>) -> Result<()> {
    let tools = tool_defs();

    loop {
        let req = ChatRequest {
            model: CHAT_MODEL,
            messages,
            tools: &tools,
            stream: false,
        };

        let resp: ChatResponse = client.post(OLLAMA_CHAT_URL)
            .json(&req)
            .send().await?
            .error_for_status()?
            .json().await?;

        let msg = resp.choices.into_iter().next()
            .context("no choices in response")?
            .message;
        messages.push(msg.clone());

        let calls = msg.tool_calls.unwrap_or_default();
        if calls.is_empty() {
            if let Some(content) = msg.content {
                if !content.is_empty() {
                    println!("\n{}\n", content);
                }
            }
            return Ok(());
        }

        for call in calls {
            println!("\t→ {}({})", call.function.name, call.function.arguments);
            let result = match execute_tool(client, &call.function.name, &call.function.arguments).await {
                Ok(s) => s,
                Err(e) => serde_json::to_string(&json!({ "error": e.to_string() })).unwrap(),
            };
            let preview: String = result.chars().take(200).collect();
            println!("\t← {}{}", preview, if result.len() > 200 { "…" } else { "" });

            messages.push(Message {
                role: "tool".to_string(),
                content: Some(result),
                tool_calls: None,
                tool_call_id: Some(call.id),
            })
        }
    }
}

// -- Main REPL --------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::new();
    let mut messages: Vec<Message> = Vec::new();
    let stdin = io::stdin();

    println!("Chat with {}. Type 'exit' or Ctrl-D to quit", CHAT_MODEL);
    println!("Tools available: search_memory, add_memory, list_direectory, read_file\n");

    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut buffer = String::new();
        if stdin.read_line(&mut buffer)? == 0 {
            println!();
            break;
        }
        let input = buffer.trim();
        if input.is_empty() { continue; }
        if input == "exit" { break; }

        messages.push(Message {
            role: "user".to_string(),
            content: Some(input.to_string()),
            tool_calls: None,
            tool_call_id: None,
        });

        if let Err(e) = agent_turn(&client, &mut messages).await {
            eprintln!("error: {:#}", e);
        }
    }

    Ok(())
}
