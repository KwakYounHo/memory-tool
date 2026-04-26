use anyhow::{Context, Result};
use reqwest::Client;
use std::path::PathBuf;

use memory_tool::{
    search::search_memory,
    storage::{open, SearchFilter},
    model::EMBED_MODEL,
};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        anyhow::bail!(
            "usage: {} <db_path> <query> [top_k]",
            args[0]
        );
    }
    let db_path = PathBuf::from(&args[1]);
    let query = &args[2];
    let top_k: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(5);

    let conn = open(&db_path).context("open db")?;
    let client = Client::new();

    let hits = search_memory(
        &conn,
        &client,
        EMBED_MODEL,
        query,
        top_k,
        &SearchFilter::default(),
    )
        .await?;

    if hits.is_empty() {
        println!("(no result)");
        return Ok(());
    }

    println!("query: {}", query);
    println!("found {} hit(s):\n", hits.len());

    for (i, hit) in hits.iter().enumerate() {
        let preview: String = hit.text.chars().take(120).collect();
        println!("[{}] distance={:.3}\tscope={}\tkind={}", i + 1, hit.distance, hit.scope, hit.kind);
        println!("\tsource : {}", hit.source);
        if let Some(p) = &hit.project {
            println!("\tproject: {}", p);
        }
        println!("\tpreview: {}...\n", preview);
    }

    Ok(())
}
