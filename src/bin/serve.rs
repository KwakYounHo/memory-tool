use anyhow::{Context, Result};
use axum::{
    Router,
    routing::{get, post},
};
use reqwest::Client;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use memory_tool::{
    api::{AppState, add_handler, health, search_handler},
    model::EMBED_MODEL,
    storage::open,
};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        anyhow::bail!("usage: {} <db_path> [bind_addr]", args[0]);
    }
    let db_path = PathBuf::from(&args[1]);
    let bind_addr = args
        .get(2)
        .map(String::from)
        .unwrap_or_else(|| "127.0.0.1:7080".to_string());

    let conn = open(&db_path).context("open db")?;
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

    let listener = TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("bind {}", bind_addr))?;
    println!("serving on http://{}", bind_addr);
    axum::serve(listener, app).await?;

    Ok(())
}
