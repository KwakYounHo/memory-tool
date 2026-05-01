use crate::{
    cli::context::CliContext,
    model::EMBED_MODEL,
    search::search_memory,
    storage::{SearchFilter, open},
};
use anyhow::{Context, Result};
use clap::Parser;
use reqwest::Client;

#[derive(Debug, Parser)]
pub struct SearchArgs {
    pub query: String,

    #[arg(long, default_value_t = 5)]
    pub top_k: usize,
}

pub async fn run(context: &CliContext, args: SearchArgs) -> Result<()> {
    let conn = open(&context.db_path).context("open db")?;
    let client = Client::new();

    let hits = search_memory(
        &conn,
        &client,
        EMBED_MODEL,
        &args.query,
        args.top_k,
        &SearchFilter::default(),
    )
    .await?;

    if hits.is_empty() {
        println!("(no result)");
        return Ok(());
    }

    println!("query: {}", args.query);
    println!("found {} hit(s):\n", hits.len());

    for (i, hit) in hits.iter().enumerate() {
        let preview: String = hit.text.chars().take(120).collect();
        println!(
            "[{}] distance={:.3}\tscope={}\tkind={}",
            i + 1,
            hit.distance,
            hit.scope,
            hit.kind
        );
        println!("\tsource : {}", hit.source);
        if let Some(p) = &hit.project {
            println!("\tproject: {}", p);
        }
        println!("\tpreview: {}...\n", preview);
    }

    Ok(())
}
