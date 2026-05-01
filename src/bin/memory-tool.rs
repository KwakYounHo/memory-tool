use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "memory-tool")]
#[command(version, about = "Personal memory system")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve(ServeArgs),
    Search(SearchArgs),
    Index(IndexArgs),
    Tui,
}

#[derive(Debug, Parser)]
struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:7080")]
    bind: String,
}

#[derive(Debug, Parser)]
struct SearchArgs {
    query: String,

    #[arg(long, default_value_t = 5)]
    top_k: usize,
}

#[derive(Debug, Parser)]
struct IndexArgs {
    files: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Serve(args) => {
            println!("serve placeholder: bind={}", args.bind);
        }
        Command::Search(args) => {
            println!(
                "search placeholder: query={}, top_k={}",
                args.query, args.top_k
            );
        }
        Command::Index(args) => {
            println!("index placeholder: files={:?}", args.files);
        }
        Command::Tui => {
            println!("tui placeholder");
        }
    }
    Ok(())
}
