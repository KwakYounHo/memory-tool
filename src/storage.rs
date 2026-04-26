use anyhow::{Context, Result};
use rusqlite::{ffi::sqlite3_auto_extension, Connection};
use sqlite_vec::sqlite3_vec_init;
use std::path::Path;
use std::sync::Once;
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS chunks (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    source              TEXT    NOT NULL,
    text                TEXT    NOT NULL,
    text_hash           TEXT    NOT NULL,

    project             TEXT,
    machine             TEXT,
    scope               TEXT    NOT NULL DEFAULT 'project',

    source_mtime        INTEGER,
    indexed_at          INTEGER NOT NULL,

    embed_model         TEXT    NOT NULL,
    embed_dim           INTEGER NOT NULL,

    superseded_by       INTEGER REFERENCES chunks(id),
    superseded_at       INTEGER,
    superseded_reason   TEXT,

    UNIQUE(source, text_hash)
);

CREATE INDEX IF NOT EXISTS  idx_chunks_source   ON chunks(source);
CREATE INDEX IF NOT EXISTS  idx_chunks_project  ON chunks(project);
CREATE INDEX IF NOT EXISTS  idx_chunks_scope    ON chunks(scope);
CREATE INDEX IF NOT EXISTS  idx_chunks_active
    ON chunks(superseded_by) WHERE superseded_by IS NULL;

CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
    embedding FLOAT[768]
);
"#;

pub fn open(path: impl AsRef<Path>) -> Result<Connection> {
    register_vec_extension();
    let conn = Connection::open(&path)
        .with_context(|| format!("open db: {}", path.as_ref().display()))?;

    // FK는 SQLite 기본 OFF, supersession 무결성을 위해 명시적으로 켬.
    conn.pragma_update(None, "foreign_keys", "ON")?;
    // 동시 읽기/쓰기 친화. 단일 사용자 PoC엔 과하지만 향후 tailnet 다중 클라이언트 대비.
    conn.pragma_update(None, "journal_mode", "WAL")?;

    conn.execute_batch(SCHEMA).context("apply schema")?;
    Ok(conn)
}

fn register_vec_extension() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(
                    sqlite3_vec_init as *const (),
        )));
    });
}

pub const EMBED_DIM: usize = 768;
#[derive(Debug, Clone, Copy)]
pub enum Scope {
    Project,
    User,
    Global,
}

impl Scope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
            Self::Global => "global",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NewChunk<'a> {
    pub source: &'a str,
    pub text: &'a str,
    pub embedding: &'a [f32],
    pub project: Option<&'a str>,
    pub machine: Option<&'a str>,
    pub scope: Scope,
    pub source_mtime: Option<i64>,
    pub embed_model: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub enum InsertOutcome {
    Inserted { id: i64 },
    Skipped { id: i64 },
}

pub fn insert_chunk(conn: &mut Connection, new: NewChunk) -> Result<InsertOutcome> {
    if new.embedding.len() != EMBED_DIM {
        anyhow::bail!(
            "embedding dim mismatch: expected {}, got {}",
            EMBED_DIM,
            new.embedding.len()
        );
    }

    let text_hash = hash_text(new.text);
    let now = epoch_secs()?;
    let tx = conn.transaction()?;

    let inserted_id: Option<i64> = tx
        .query_row(
            "INSERT OR IGNORE INTO chunks(
                source, text, text_hash,
                project, machine, scope,
                source_mtime, indexed_at,
                embed_model, embed_dim
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            RETURNING id",
            rusqlite::params![
            new.source,
            new.text,
            text_hash,
            new.project,
            new.machine,
            new.scope.as_str(),
            new.source_mtime,
            now,
            new.embed_model,
            EMBED_DIM as i64,
            ],
            |row| row.get(0),
            )
                .optional()?;

    let outcome = match inserted_id {
        Some(id) => {
            let bytes: &[u8] = bytemuck::cast_slice(new.embedding);
            tx.execute(
                "INSERT INTO vec_chunks(rowid, embedding) VALUES (?, ?)",
                rusqlite::params![id, bytes],
            )?;
            InsertOutcome::Inserted { id }
        },
        None => {
            let existing: i64 = tx.query_row(
                "SELECT id FROM chunks WHERE source = ? AND text_hash = ?",
                rusqlite::params![new.source, text_hash],
                |row| row.get(0),
            )?;
            InsertOutcome::Skipped { id: existing }
        }
    };

    tx.commit()?;
    Ok(outcome)
}

fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}

fn epoch_secs() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time before epoch")?
        .as_secs() as i64)
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub id: i64,
    pub source: String,
    pub text: String,
    pub project: Option<String>,
    pub machine: Option<String>,
    pub scope: String,
    pub distance: f32,
}

#[derive(Debug, Default)]
pub struct SearchFilter<'a> {
    pub scope: Option<&'a [Scope]>,
    pub project: Option<&'a str>,
    pub machine: Option<&'a str>,
    pub source_prefix: Option<&'a str>,
}

pub fn search(
    conn: &Connection,
    query_embedding: &[f32],
    top_k: usize,
    filter: &SearchFilter,
) -> Result<Vec<SearchHit>> {
    if query_embedding.len() != EMBED_DIM {
        anyhow::bail!(
            "query embedding dim mismatch: expected {}, got {}",
            EMBED_DIM,
            query_embedding.len()
        );
    }

    let mut sql = String::from(
        "SELECT c.id, c.source, c.text, c.project, c.machine, c.scope, distance
        FROM vec_chunks v
        JOIN chunks c ON c.id = v.rowid
        WHERE v.embedding MATCH ?
            AND k = ?
            AND c.superseded_by IS NULL",
    );

    let query_bytes: Vec<u8> = bytemuck::cast_slice(query_embedding).to_vec();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(query_bytes),
        Box::new(top_k as i64),
    ];

    if let Some(scopes) = filter.scope {
        if !scopes.is_empty() {
            let qs: Vec<&str> = scopes.iter().map(|_| "?").collect();
            sql.push_str(&format!("\tAND c.scope IN ({})", qs.join(",")));
            for s in scopes {
                params.push(Box::new(s.as_str().to_string()));
            }
        }
    }
    if let Some(p) = filter.project {
        sql.push_str("\tAND c.project = ?");
        params.push(Box::new(p.to_string()));
    }
    if let Some(m) = filter.machine {
        sql.push_str("\tAND c.machine = ?");
        params.push(Box::new(m.to_string()));
    }
    if let Some(prefix) = filter.source_prefix {
        sql.push_str("\tAND c.source LIKE ? || '%'");
        params.push(Box::new(prefix.to_string()));
    }

    sql.push_str("\tORDER BY distance");

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|p| p.as_ref()).collect();

    let hits = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(SearchHit {
                id: row.get(0)?,
                source: row.get(1)?,
                text: row.get(2)?,
                project: row.get(3)?,
                machine: row.get(4)?,
                scope: row.get(5)?,
                distance: row.get(6)?,
            })
        })?
    .collect::<Result<Vec<_>, _>>()?;

    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_and_loads_vec() -> Result<()> {
        let conn = open(":memory:")?;
        let version: String = conn.query_row("SELECT vec_version()", [], |r| r.get(0))?;
        assert!(!version.is_empty(), "vec_version returned empty");

        let table_count: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name IN ('chunks', 'vec_chunks')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(table_count, 2);
        Ok(())
    }

    #[test]
    fn insert_then_skip_duplicate() -> Result<()> {
        let mut conn = open(":memory:")?;
        let embedding = vec![0.1f32; EMBED_DIM];

        let chunk = NewChunk {
            source: "/tmp/test.md",
            text: "hello world",
            embedding: &embedding,
            project: Some("test"),
            machine: None,
            scope: Scope::Project,
            source_mtime: None,
            embed_model: "embeddinggemma:300m-qat-q4_0",
        };

        let first = insert_chunk(&mut conn, chunk)?;
        let second = insert_chunk(&mut conn, chunk)?;

        let id = match first {
            InsertOutcome::Inserted { id } => id,
            _ => panic!("first call should be Inserted"),
        };
        assert_eq!(second, InsertOutcome::Skipped { id });

        let vec_count: i64 = conn.query_row("SELECT count(*) FROM vec_chunks", [], |r| r.get(0))?;
        assert_eq!(vec_count, 1, "벡터는 한 번만 저장되어야 함");

        Ok(())
    }

    #[test]
    fn rejects_wrong_dim() -> Result<()> {
        let mut conn = open(":memory:")?;
        let bad = vec![0.0f32; EMBED_DIM + 1];

        let chunk = NewChunk {
            source: "/tmp/x.md",
            text: "x",
            embedding: &bad,
            project: None,
            machine: None,
            scope: Scope::Project,
            source_mtime: None,
            embed_model: "test",
        };

        let err = insert_chunk(&mut conn, chunk).unwrap_err();
        assert!(err.to_string().contains("embedding dim mismatch"));
        Ok(())
    }

    #[test]
    fn different_text_same_source_inserts_both() -> Result<()> {
        let mut conn = open(":memory:")?;
        let embedding = vec![0.1f32; EMBED_DIM];

        let a = NewChunk {
            source: "/tmp/test.md", text: "first chunk text",
            embedding: &embedding,
            project: None, machine: None,
            scope: Scope::Project, source_mtime: None,
            embed_model: "test",
        };

        let b = NewChunk { text: "second chunk text", ..a };

        let r1 = insert_chunk(&mut conn, a)?;
        let r2 = insert_chunk(&mut conn, b)?;

        assert!(matches!(r1, InsertOutcome::Inserted { .. }));
        assert!(matches!(r2, InsertOutcome::Inserted { .. }));
        Ok(())
    }

    #[test]
    fn search_returns_closest_first() -> Result<()> {
        let mut conn = open(":memory:")?;

        let mut e_alpha = vec![0.0f32; EMBED_DIM];
        e_alpha[0] = 1.0;
        let mut e_beta = vec![0.0f32; EMBED_DIM];
        e_beta[1] = 1.0;

        insert_chunk(&mut conn, NewChunk {
            source: "/a.md", text: "alpha",
            embedding: &e_alpha,
            project: None, machine: None,
            scope: Scope::Project, source_mtime: None,
            embed_model: "test",
        })?;
        insert_chunk(&mut conn, NewChunk {
            source: "/b.md", text: "beta",
            embedding: &e_beta,
            project: None, machine: None,
            scope: Scope::Project, source_mtime: None,
            embed_model: "test",
        })?;

        let hits = search(&conn, &e_alpha, 2, &SearchFilter::default())?;
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].text, "alpha");
        assert!(hits[0].distance < hits[1].distance);
        Ok(())
    }

    #[test]
    fn search_filters_by_project() -> Result<()> {
        let mut conn = open(":memory:")?;
        let e = vec![0.5f32; EMBED_DIM];

        insert_chunk(&mut conn, NewChunk {
            source: "/a.md", text: "in proj-a",
            embedding: &e,
            project: Some("proj-a"), machine: None,
            scope: Scope::Project, source_mtime: None,
            embed_model: "test",
        })?;
        insert_chunk(&mut conn, NewChunk {
            source: "/b.md", text: "in proj-a",
            embedding: &e,
            project: Some("proj-b"), machine: None,
            scope: Scope::Project, source_mtime: None,
            embed_model: "test",
        })?;

        let hits = search(&conn, &e, 5, &SearchFilter {
            project: Some("proj-a"),
            ..Default::default()
        })?;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "in proj-a");
        Ok(())
    }

    #[test]
    fn search_excludes_superseded() -> Result<()> {
        let mut conn = open(":memory:")?;
        let e = vec![0.5f32; EMBED_DIM];

        let old_id = match insert_chunk(&mut conn, NewChunk {
            source: "a.md", text: "old",
            embedding: &e,
            project: None, machine: None,
            scope: Scope::Project, source_mtime: None,
            embed_model: "test",
        })? {
            InsertOutcome::Inserted { id } => id,
            _ => panic!("expected Inserted"),
        };
        let new_id = match insert_chunk(&mut conn, NewChunk {
            source: "b.md", text: "new",
            embedding: &e,
            project: None, machine: None,
            scope: Scope::Project, source_mtime: None,
            embed_model: "test",
        }) ? {
            InsertOutcome::Inserted { id } => id,
            _ => panic!("expected Inserted"),
        };

        conn.execute(
            "UPDATE chunks SET superseded_by = ?, superseded_at = ? WHERE id = ?",
            rusqlite::params![new_id, 0i64, old_id],
        )?;

        let hits = search(&conn, &e, 5, &SearchFilter::default())?;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "new");
        Ok(())
    }
}
