use crate::{
    cli::context::CliContext,
    indexer::{IndexOptions, index_files},
    model::EMBED_MODEL,
    storage::{Kind, Scope, open},
};
use anyhow::{Context, Result};
use clap::Parser;
use reqwest::Client;
use std::path::PathBuf;

#[derive(Debug, Parser)]
pub struct IndexArgs {
    pub files: Vec<PathBuf>,
}

pub async fn run(context: &CliContext, args: IndexArgs) -> Result<()> {
    let mut conn = open(&context.db_path).context("open db")?;
    let client = Client::new();

    let opts = IndexOptions {
        embed_model: EMBED_MODEL,
        project: None,
        machine: None,
        scope: Scope::Agent,
        kind: Kind::Note,
    };

    let stats = index_files(&mut conn, &args.files, &client, &opts).await?;

    println!(
        "indexed {} file(s): {} chunks inserted, {} skipped (duplicates)",
        stats.files, stats.chunks_inserted, stats.chunks_skipped
    );

    Ok(())
}
