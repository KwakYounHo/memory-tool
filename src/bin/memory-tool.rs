use anyhow::Result;
use clap::Parser;
use memory_tool::cli::{Cli, Command, context::CliContext};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let context = CliContext::load()?;

    match cli.command {
        Command::Serve(args) => {
            memory_tool::cli::serve::run(&context, args).await?;
        }
        Command::Search(args) => {
            memory_tool::cli::search::run(&context, args).await?;
        }
        Command::Index(args) => {
            memory_tool::cli::index::run(&context, args).await?;
        }
        Command::Tui => {
            memory_tool::cli::tui::run().await?;
        }
    }
    Ok(())
}
