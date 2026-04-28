use anyhow::Result;
use reqwest::Client;
use rusqlite::Connection;

use crate::indexer::embed_batch;
use crate::storage::{SearchFilter, SearchHit, search};

pub async fn search_memory(
    conn: &Connection,
    client: &Client,
    embed_model: &str,
    query: &str,
    top_k: usize,
    filter: &SearchFilter<'_>,
) -> Result<Vec<SearchHit>> {
    let mut embeddings = embed_batch(client, embed_model, &[query]).await?;
    let query_embedding = embeddings
        .pop()
        .ok_or_else(|| anyhow::anyhow!("Ollama returned empty embeddings"))?;

    search(conn, &query_embedding, top_k, filter)
}

pub async fn embed_query(client: &Client, embed_model: &str, query: &str) -> Result<Vec<f32>> {
    let mut embeddings = embed_batch(client, embed_model, &[query]).await?;
    embeddings
        .pop()
        .ok_or_else(|| anyhow::anyhow!("Ollama returned empty embeddings"))
}
