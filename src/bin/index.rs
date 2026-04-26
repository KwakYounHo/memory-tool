use anyhow::{Context, Result};
use reqwest::Client;
use std::path::PathBuf;

use memory_rag_poc::{
    indexer::{index_files, IndexOptions},
    storage::{open, Scope},
};

const EMBED_MODEL: &str = "embeddinggemma:300m-qat-q4_0";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        anyhow::bail!(
            "usage: {} <dn_path> <markdown_file> [<markdown_file>...]",
            args[0]
        );
    }
    let db_path = PathBuf::from(&args[1]);
    let paths: Vec<PathBuf> = args[2..].iter().map(PathBuf::from).collect();

    let mut conn = open(&db_path).context("open db")?;
    let client = Client::new();

    let opts = IndexOptions {
        embed_model: EMBED_MODEL,
        project: None,
        machine: None,
        scope: Scope::Project,
    };

    let stats = index_files(&mut conn, &paths, &client, &opts).await?;

    println!(
        "indexed {} file(s): {} chunks inserted, {} skipped (duplicates)",
        stats.files, stats.chunks_inserted, stats.chunks_skipped
    );

    Ok(())
}
