pub mod context;
pub mod index;
pub mod search;
pub mod serve;
pub mod tui;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "memory-tool")]
#[command(version, about = "Personal memory system")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Serve(serve::ServeArgs),
    Search(search::SearchArgs),
    Index(index::IndexArgs),
    Tui,
}
