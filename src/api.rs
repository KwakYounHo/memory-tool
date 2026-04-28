use anyhow::Context;
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use reqwest::Client;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use tokio::sync::Mutex;

use crate::indexer::embed_batch;
use crate::search::embed_query;
use crate::storage::{InsertOutcome, Kind, NewChunk, Scope, SearchFilter, SearchHit, insert_chunk};

fn parse_file_source(source: &str) -> Result<PathBuf, anyhow::Error> {
    let path_str = source
        .strip_prefix("file://")
        .ok_or_else(|| anyhow::anyhow!("source must be a file:// URI, got: {}", source))?;
    let path = PathBuf::from(path_str);
    if !path.is_absolute() {
        anyhow::bail!("file:// path must be absolute: {}", path_str);
    }
    Ok(path)
}

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub client: Client,
    pub embed_model: String,
}

// -- DTOs --------------------------------------------------

#[derive(Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default)]
    pub top_k: Option<usize>,
    #[serde(default)]
    pub filter: FilterDto,
}

#[derive(Deserialize, Default)]
pub struct FilterDto {
    pub project: Option<String>,
    pub machine: Option<String>,
    pub source_prefix: Option<String>,
    pub scope: Option<Vec<String>>,
    pub kind: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct HitDto {
    pub id: i64,
    pub source: String,
    pub text: String,
    pub project: Option<String>,
    pub machine: Option<String>,
    pub scope: String,
    pub kind: String,
    pub distance: f32,
}

impl From<SearchHit> for HitDto {
    fn from(h: SearchHit) -> Self {
        Self {
            id: h.id,
            source: h.source,
            text: h.text,
            project: h.project,
            machine: h.machine,
            scope: h.scope,
            kind: h.kind,
            distance: h.distance,
        }
    }
}

#[derive(Deserialize)]
pub struct AddRequest {
    pub source: String,
    pub project: Option<String>,
    pub machine: Option<String>,
    pub scope: Option<String>, // "agent" | "user", 기본 "agent"
    pub kind: Option<String>,  // "rule"|"feedback"|"reflection"|"reference"|"memory"|"note"
}

#[derive(Serialize)]
pub struct AddResponse {
    pub outcome: &'static str, // "inserted" | "skipped"
    pub id: i64,
}

// -- Handlers --------------------------------------------------

pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

pub async fn search_handler(
    State(state): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<Vec<HitDto>>, ApiError> {
    let scopes: Option<Vec<Scope>> = req.filter.scope.as_ref().map(|ss| {
        ss.iter()
            .filter_map(|s| match s.as_str() {
                "agent" => Some(Scope::Agent),
                "user" => Some(Scope::User),
                _ => None,
            })
            .collect()
    });
    let kinds: Option<Vec<Kind>> = req.filter.kind.as_ref().map(|ks| {
        ks.iter()
            .filter_map(|k| match k.as_str() {
                "rule" => Some(Kind::Rule),
                "feedback" => Some(Kind::Feedback),
                "reflection" => Some(Kind::Reflection),
                "reference" => Some(Kind::Reference),
                "knowledge" => Some(Kind::Knowledge),
                "memory" => Some(Kind::Memory),
                "note" => Some(Kind::Note),
                _ => None,
            })
            .collect()
    });
    let filter = SearchFilter {
        scope: scopes.as_deref(),
        kind: kinds.as_deref(),
        project: req.filter.project.as_deref(),
        machine: req.filter.machine.as_deref(),
        source_prefix: req.filter.source_prefix.as_deref(),
    };

    let q_embed = embed_query(&state.client, &state.embed_model, &req.query).await?;

    let conn = state.db.lock().await;
    let hits = crate::storage::search(&conn, &q_embed, req.top_k.unwrap_or(5), &filter)?;
    drop(conn);

    Ok(Json(hits.into_iter().map(HitDto::from).collect()))
}

pub async fn add_handler(
    State(state): State<AppState>,
    Json(req): Json<AddRequest>,
) -> Result<Json<AddResponse>, ApiError> {
    // 1. verify file:// and extract path
    let path = parse_file_source(&req.source)?;

    // 2. Read file directly (Program)
    let bytes =
        std::fs::read(&path).with_context(|| format!("read source file: {}", path.display()))?;
    let text = String::from_utf8(bytes).context("source file is not valid UTF-8")?;

    // 3. Automatic extract mtime
    let source_mtime = path
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    // 4. Embedding
    let embeddings = embed_batch(&state.client, &state.embed_model, &[text.as_str()]).await?;
    let embedding = embeddings
        .into_iter()
        .next()
        .context("empty embeddings from Ollama")?;

    // 5. Parse metadata
    let scope = match req.scope.as_deref() {
        Some("user") => Scope::User,
        _ => Scope::Agent,
    };
    let kind = match req.kind.as_deref() {
        Some("rule") => Kind::Rule,
        Some("feedback") => Kind::Feedback,
        Some("reflection") => Kind::Reflection,
        Some("reference") => Kind::Reference,
        Some("knowledge") => Kind::Knowledge,
        Some("memory") => Kind::Memory,
        _ => Kind::Note,
    };

    // 6. Save
    let mut conn = state.db.lock().await;
    let canonical_source = path.display().to_string();

    let outcome = insert_chunk(
        &mut conn,
        NewChunk {
            source: &canonical_source,
            text: &text,
            embedding: &embedding,
            project: req.project.as_deref(),
            machine: req.machine.as_deref(),
            scope,
            kind,
            source_mtime,
            embed_model: &state.embed_model,
        },
    )?;

    let resp = match outcome {
        InsertOutcome::Inserted { id } => AddResponse {
            outcome: "inserted",
            id,
        },
        InsertOutcome::Skipped { id } => AddResponse {
            outcome: "skipped",
            id,
        },
    };
    Ok(Json(resp))
}

// -- Error handling --------------------------------------------------

pub struct ApiError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": self.0.to_string() })),
        )
            .into_response()
    }
}
