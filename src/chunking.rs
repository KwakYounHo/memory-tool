use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;
use text_splitter::{ChunkConfig, MarkdownSplitter};

const OLLAMA_EMBED_URL: &str = "http://localhost:11434/api/embed";
const TOP_K: usize = 5;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub id: usize,
    pub source: String,
    pub text: String,
}

#[derive(Debug)]
pub struct GoldenQuery {
    pub query: String,
    pub expected_chunk_id: usize,
}

#[derive(Debug)]
pub struct ModelStats {
    pub model: String,
    pub chunks: usize,
    pub queries: usize,
    pub indexing_total_ms: u128,
    pub chunks_per_sec: f64,
    pub avg_query_ms: f64,
    pub mrr_at_k: f64,
    pub recall_at_k: f64,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

async fn embed(client: &Client, model: &str, inputs: &[&str]) -> Result<Vec<Vec<f32>>> {
    let req = EmbedRequest { model, input: inputs.to_vec() };
    let resp = client
        .post(OLLAMA_EMBED_URL)
        .json(&req)
        .send()
        .await
        .context("HTTP send to Ollama failed")?
        .error_for_status()
        .context("Ollama returned error status")?
        .json::<EmbedResponse>()
        .await
        .context("decode Ollama response")?;
    Ok(resp.embeddings)
}

pub fn chunk_files(paths: &[PathBuf]) -> Result<Vec<Chunk>> {
    // Character-based for now; swap ChunkConfig::new(...) to a tokenizer-aware
    // sizer when accurate token counts matter.
    let config = ChunkConfig::new(800).with_overlap(160)?;
    let splitter = MarkdownSplitter::new(config);

    let mut all = Vec::new();
    let mut next_id = 0usize;
    for path in paths {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?;
        let source = path.display().to_string();
        const MIN_CHUNK_CHARS: usize = 50;
        for chunk in splitter.chunks(&text) {
            if chunk.trim().chars().count() < MIN_CHUNK_CHARS {
                continue;
            }
            all.push(Chunk { id: next_id, source: source.clone(), text: chunk.to_string() });
            next_id +=1;
        }
    }
    Ok(all)
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (na * nb)
}

pub async fn evaluate(
    client: &Client,
    model: &str,
    chunks: &[Chunk],
    queries: &[GoldenQuery],
) -> Result<ModelStats> {
    // Batched indexing keeps a single request from getting too large; Ollama
    // handles bigger batches but 32 is conservative.
    const INDEX_BATCH: usize = 32;
    let chunk_texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();

    let t0 = Instant::now();
    let mut chunk_embeds: Vec<Vec<f32>> = Vec::with_capacity(chunks.len());
    for batch in chunk_texts.chunks(INDEX_BATCH) {
        let mut e = embed(client, model, batch).await?;
        chunk_embeds.append(&mut e);
    }
    let indexing_total_ms = t0.elapsed().as_millis();
    let chunks_per_sec = chunks.len() as f64 / (indexing_total_ms as f64 / 1000.0);

    let mut total_query_ms: u128 = 0;
    let mut reciprocal_ranks: Vec<f64> = Vec::with_capacity(queries.len());
    let mut hits: usize = 0;

    for q in queries {
        let t = Instant::now();
        let q_embed = embed(client, model, &[q.query.as_str()]).await?;
        total_query_ms += t.elapsed().as_millis();
        let q_vec = &q_embed[0];

        let mut scored: Vec<(usize, f32)> = chunks
            .iter()
            .zip(chunk_embeds.iter())
            .map(|(c, e)| (c.id, cosine(q_vec, e)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).expect("no NaN in cosine"));

        match scored.iter().position(|(id, _)| *id == q.expected_chunk_id) {
            Some(pos) if pos < TOP_K => {
                reciprocal_ranks.push(1.0 / (pos + 1) as f64);
                hits += 1;
                println!("\t[{:>2}] rank {:<2}\t{}", q.expected_chunk_id, pos + 1, q.query);
            },
            _ => {
                reciprocal_ranks.push(0.0);
                println!("\t[{:>2}] MISS\t\t{}", q.expected_chunk_id, q.query);
            }
        }
    }

    let mrr_at_k = reciprocal_ranks.iter().sum::<f64>() / queries.len() as f64;
    let recall_at_k = hits as f64 / queries.len() as f64;
    let avg_query_ms = total_query_ms as f64 / queries.len() as f64;

    Ok(ModelStats {
        model: model.to_string(),
        chunks: chunks.len(),
        queries: queries.len(),
        indexing_total_ms,
        chunks_per_sec,
        avg_query_ms,
        mrr_at_k,
        recall_at_k,
    })
}

pub fn print_stats(s: &ModelStats) {
    println!("\n┌── {}", s.model);
    println!("│  chunks indexed   : {}", s.chunks);
    println!("│  queries evaluated: {}", s.queries);
    println!("│  indexing total   : {} ms ({:.1} chunks/s)", s.indexing_total_ms, s.chunks_per_sec);
    println!("│  avg query latency: {:.1} ms", s.avg_query_ms);
    println!("│  MRR@{}            : {:.3}", TOP_K, s.mrr_at_k);
    println!("└  Recall@{}         : {:.3}", TOP_K, s.recall_at_k);
}

pub fn print_decision(eg: &ModelStats, bge: &ModelStats) {
    println!("\n=== Decision ===");
    let ratio = eg.mrr_at_k / bge.mrr_at_k;
    println!("MRR ratio (EmbeddingGemma / bge-m3): {:.3}", ratio);
    if ratio >= 0.95 {
        println!("→ EmbeddingGemma - quality near-equal, efficiency wins");
    } else if ratio >= 0.85 {
        println!("→ Judgement call - quality gap moderate, weigh against efficiency");
    } else {
        println!("→ bge-m3 - quality gap large enough to justify the cost");
    }
}

// Fill these in after seeing the chunk listing from the first run.
// expected_chunk_id refers to the `id` printed in the chunk listing.
pub fn golden_queries() -> Vec<GoldenQuery> {
    vec![
        // ── AGENT.md (10) ──────────────────────────────────────
        GoldenQuery {
            query: "agent의 역할 — 강의자가 아닌 무엇?".into(),
            expected_chunk_id: 0,
        },
        GoldenQuery {
            query: "이 저장소의 main learning thread를 owns하는 도구는?".into(),
            expected_chunk_id: 1,
        },
        GoldenQuery {
            query: "Codex가 명시적 동의 없이 하지 말아야 할 일들".into(),
            expected_chunk_id: 2,
        },
        GoldenQuery {
            query: "Rust Book의 가르침 흐름 5단계".into(),
            expected_chunk_id: 4,
        },
        GoldenQuery {
            query: "한 번에 여러 개념 설명하지 말라는 페이싱 원칙".into(),
            expected_chunk_id: 5,
        },
        GoldenQuery {
            query: "Tutorial부터 Independent까지 phase별 역할 분담".into(),
            expected_chunk_id: 6,
        },
        GoldenQuery {
            query: "TypeScript 비유는 어떤 기준으로 써야 하나".into(),
            expected_chunk_id: 7,
        },
        GoldenQuery {
            query: "숙련도 평가에 쓰이는 4가지 학술 프레임워크".into(),
            expected_chunk_id: 8,
        },
        GoldenQuery {
            query: "학습자 출력 공유 시 첫 번째로 해야 할 mandatory 행동".into(),
            expected_chunk_id: 20,
        },
        GoldenQuery {
            query: "confident하게 잘못된 정보를 주는 위험에 대한 규칙".into(),
            expected_chunk_id: 22,
        },

        // ── Phase 6 milestone 1 평가 (4) ───────────────────────
        GoldenQuery {
            query: "git log를 결정의 역사로 활용한 학습자 사례".into(),
            expected_chunk_id: 28,
        },
        GoldenQuery {
            query: "도메인 지식 없이 grid search 표준 관행을 유추".into(),
            expected_chunk_id: 29,
        },
        GoldenQuery {
            query: "agent의 분류 오류를 질문 형태로 드러낸 패턴".into(),
            expected_chunk_id: 30,
        },
        GoldenQuery {
            query: "B2에서 C1로 넘어가지 못한 이유들".into(),
            expected_chunk_id: 32,
        },

        // ── Phase 6 milestone 2 평가 (4) ───────────────────────
        GoldenQuery {
            query: "agent 제안보다 더 idiomatic한 Rust 선택을 학습자가 한 사례".into(),
            expected_chunk_id: 38,
        },
        GoldenQuery {
            query: "학습자가 agent의 MANDATORY 규칙 위반을 메타 감지".into(),
            expected_chunk_id: 39,
        },
        GoldenQuery {
            query: "Sharpe ratio를 단독 학습 대신 실전에서 익히겠다는 메타 전략".into(),
            expected_chunk_id: 41,
        },
        GoldenQuery {
            query: "MDD를 가격 변동으로 잘못 해석한 재발 실수".into(),
            expected_chunk_id: 43,
        },
    ]
}

