use crate::{
    api::{AppState, add_handler, health, search_handler},
    cli::context::CliContext,
    model::EMBED_MODEL,
    storage::open,
};
use anyhow::{Context, Result};
use axum::{
    Router,
    routing::{get, post},
};
use clap::Parser;
use reqwest::Client;
use std::sync::Arc;
use tokio::{net::TcpListener, sync::Mutex};

#[derive(Debug, Parser)]
pub struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:7080")]
    pub bind: String,
}

pub async fn run(context: &CliContext, args: ServeArgs) -> Result<()> {
    let conn = open(&context.db_path).context("open db")?;
    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        client: Client::new(),
        embed_model: EMBED_MODEL.to_string(),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/search_memory", post(search_handler))
        .route("/add_memory", post(add_handler))
        .with_state(state);

    let listener = TcpListener::bind(&args.bind)
        .await
        .with_context(|| format!("bind {}", args.bind))?;

    println!(
        "serving on http://{} using {}",
        args.bind,
        context.db_path.display()
    );

    axum::serve(listener, app).await?;

    Ok(())
}
