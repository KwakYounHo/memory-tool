use memory_rag_poc::chunking::{chunk_files, evaluate, golden_queries, print_stats};
use std::path::PathBuf;
use reqwest::Client;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        anyhow::bail!("usage: {} <markdown_file> [<markdown_file>...]", args[0]);
    }
    let paths: Vec<PathBuf> = args[1..].iter().map(PathBuf::from).collect();

    let chunks = chunk_files(&paths)?;
    println!("Indexed {} chunks from {} file(s)", chunks.len(), paths.len());
    for c in chunks.iter().take(5) {
        let preview: String = c.text.chars().take(60).collect();
        println!("\t[{}] {} ({} chars) - {}...", c.id, c.source, c.text.len(), preview);
    }
    if chunks.len() > 5 {
        println!("\t...and {} more", chunks.len() - 5);
    }

    let queries = golden_queries();
    if queries.is_empty() {
        println!("\n=== Full chunk listing ===");
        for c in &chunks {
            println!("\n[Chunk {}] {} ({} chars)", c.id, c.source, c.text.len());
            println!("{}", c.text);
        }
        println!("\n⚠ No golden queries - populate `golden_queries()` then re-run.");
        return Ok(());
    }
    println!("Golden queries: {}", queries.len());

    let client = Client::new();
    // let stats_bge = evaluate(&client, "bge-m3", &chunks, &queries).await?;
    // print_stats(&stats_bge);
    let stats_eg = evaluate(&client, "embeddinggemma:300m-qat-q4_0", &chunks, &queries).await?;
    print_stats(&stats_eg);

    // print_decision(&stats_eg, &stats_bge);
    Ok(())
}
