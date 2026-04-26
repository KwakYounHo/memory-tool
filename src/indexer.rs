use anyhow::{Context, Result};
use reqwest::Client;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::UNIX_EPOCH;
use text_splitter::{ChunkConfig, MarkdownSplitter};

use crate::storage::{insert_chunk, InsertOutcome, Kind, NewChunk, Scope};

const MIN_CHUNK_CHARS: usize = 50;
const CHUNK_SIZE: usize = 800;
const CHUNK_OVERLAP: usize = 160;
const EMBED_BATCH: usize = 32;
const OLLAMA_EMBED_URL: &str = "http://localhost:11434/api/embed";

#[derive(Debug, Default, Clone, Copy)]
pub struct IndexStats {
    pub files: usize,
    pub chunks_inserted: usize,
    pub chunks_skipped: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct IndexOptions<'a> {
    pub embed_model: &'a str,
    pub project: Option<&'a str>,
    pub machine: Option<&'a str>,
    pub scope: Scope,
    pub kind: Kind
}

pub async fn index_files<P: AsRef<Path>>(
    conn: &mut Connection,
    paths: &[P],
    client: &Client,
    options: &IndexOptions<'_>,
) -> Result<IndexStats> {
    let config = ChunkConfig::new(CHUNK_SIZE).with_overlap(CHUNK_OVERLAP)?;
    let splitter = MarkdownSplitter::new(config);
    let mut stats = IndexStats::default();

    for path in paths {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?;
        let mtime = file_mtime(path).ok();
        let source = path.display().to_string();

        let chunks: Vec<String> = splitter
            .chunks(&text)
            .filter(|c| c.trim().chars().count() >= MIN_CHUNK_CHARS)
            .map(|s| s.to_string())
            .collect();

        for batch in chunks.chunks(EMBED_BATCH) {
            let inputs: Vec<&str> = batch.iter().map(String::as_str).collect();
            let embeddings = embed_batch(client, options.embed_model, &inputs).await?;

            for (chunk_text, embedding) in batch.iter().zip(embeddings.iter()) {
                let new = NewChunk {
                    source: &source,
                    text: chunk_text,
                    embedding,
                    project: options.project,
                    machine: options.machine,
                    scope: options.scope,
                    kind: options.kind,
                    source_mtime: mtime,
                    embed_model: options.embed_model,
                };
                match insert_chunk(conn, new)? {
                    InsertOutcome::Inserted { .. } => stats.chunks_inserted += 1,
                    InsertOutcome::Skipped { .. } => stats.chunks_skipped += 1,
                }
            }
        }

        stats.files += 1;
    }

    Ok(stats)
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

pub async fn embed_batch(
    client: &Client,
    model: &str,
    inputs: &[&str],
) -> Result<Vec<Vec<f32>>> {
    let req = EmbedRequest { model, input: inputs.to_vec() };
    let resp = client
        .post(OLLAMA_EMBED_URL)
        .json(&req)
        .send().await.context("HTTP send to Ollama failed")?
        .error_for_status().context("Ollama returned error status")?
        .json::<EmbedResponse>().await.context("decode Ollama response")?;
    Ok(resp.embeddings)
}

fn file_mtime(path: &Path) -> Result<i64> {
    Ok(path.metadata()?.modified()?
        .duration_since(UNIX_EPOCH)?.as_secs() as i64)
}

