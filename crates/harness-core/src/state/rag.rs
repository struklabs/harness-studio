//! Storage of conversation documents for local RAG (schema v5; see `crate::rag`).

use super::{new_id, now_ms, StateDb, StateError, StateResult};
use crate::error_key::err;
use crate::rag::chunk::Chunk;
use crate::rag::search;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Documents one conversation may hold.
pub const MAX_DOCUMENTS_PER_CONVERSATION: i64 = 50;

/// A document attached to a conversation (without its text).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RagDocument {
    pub id: String,
    pub conversation_id: String,
    pub name: String,
    /// `pdf | docx | markdown | code | text`.
    pub kind: String,
    pub source_path: Option<String>,
    pub size: i64,
    pub sha256: String,
    pub pages: Option<i64>,
    pub chars: i64,
    /// Estimated tokens of the whole text (`search::estimate_tokens`).
    pub tokens: i64,
    pub chunks: i64,
    /// Chunks that have an embedding of `embed_model`.
    pub embedded: i64,
    /// `pending | extracting | embedding | ready | failed | cancelled`.
    pub status: String,
    /// Keyed error of a failed document.
    pub error: Option<String>,
    /// `auto | full | retrieval`.
    pub mode: String,
    pub embed_model: Option<String>,
    pub embed_dims: Option<i64>,
    /// Pages of a PDF whose text came from the knowledge base OCR model (1-based, ascending);
    /// empty when the text layer was used throughout.
    #[serde(default)]
    pub ocr_pages: Vec<u32>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A chunk found by retrieval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChunkHit {
    pub chunk_id: i64,
    pub document_id: String,
    pub document_name: String,
    pub seq: i64,
    pub text: String,
    pub page_start: Option<i64>,
    pub page_end: Option<i64>,
    pub heading: Option<String>,
}

/// A chunk waiting for its embedding.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingChunk {
    pub chunk_id: i64,
    pub text: String,
    pub heading: Option<String>,
}

pub const ACTIVE_STATUSES: [&str; 3] = ["pending", "extracting", "embedding"];

const DOC_COLS: &str = "d.id, d.conversation_id, d.name, d.kind, d.source_path, d.size, d.sha256, \
    d.pages, d.chars, d.status, d.error, d.mode, d.embed_model, d.embed_dims, d.created_at, \
    d.updated_at, \
    (SELECT COUNT(*) FROM rag_chunks c WHERE c.document_id = d.id), \
    (SELECT COUNT(*) FROM rag_chunks c WHERE c.document_id = d.id AND c.embedding IS NOT NULL), \n    d.ocr_pages";

/// `"2,5"` -> `[2, 5]` (a stored list of page numbers).
fn parse_pages(text: Option<&str>) -> Vec<u32> {
    text.unwrap_or_default()
        .split(',')
        .filter_map(|n| n.trim().parse().ok())
        .collect()
}

fn doc_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RagDocument> {
    let chars: i64 = r.get(8)?;
    Ok(RagDocument {
        id: r.get(0)?,
        conversation_id: r.get(1)?,
        name: r.get(2)?,
        kind: r.get(3)?,
        source_path: r.get(4)?,
        size: r.get(5)?,
        sha256: r.get(6)?,
        pages: r.get(7)?,
        chars,
        tokens: search::estimate_tokens(chars.max(0) as usize) as i64,
        status: r.get(9)?,
        error: r.get(10)?,
        mode: r.get(11)?,
        embed_model: r.get(12)?,
        embed_dims: r.get(13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
        chunks: r.get(16)?,
        embedded: r.get(17)?,
        ocr_pages: parse_pages(r.get::<_, Option<String>>(18)?.as_deref()),
    })
}

fn get_doc(conn: &Connection, id: &str) -> StateResult<RagDocument> {
    conn.query_row(
        &format!("SELECT {DOC_COLS} FROM rag_documents d WHERE d.id = ?1"),
        [id],
        doc_row,
    )
    .optional()?
    .ok_or_else(|| StateError::not_found(err("rag.document_not_found").with("id", id)))
}

fn check_status(status: &str) -> StateResult<()> {
    const ALL: [&str; 6] = [
        "pending",
        "extracting",
        "embedding",
        "ready",
        "failed",
        "cancelled",
    ];
    if ALL.contains(&status) {
        Ok(())
    } else {
        Err(StateError::invalid(
            err("rag.invalid_status").with("status", status),
        ))
    }
}

impl StateDb {
    /// A new, empty document (`pending`) of a conversation.
    pub fn rag_create_document(
        &self,
        conversation_id: &str,
        name: &str,
        source_path: Option<&str>,
    ) -> StateResult<RagDocument> {
        self.rag_create_document_limited(
            conversation_id,
            name,
            source_path,
            MAX_DOCUMENTS_PER_CONVERSATION,
        )
    }

    /// [`rag_create_document`](Self::rag_create_document) with another document limit (a
    /// knowledge base holds more than a conversation).
    pub fn rag_create_document_limited(
        &self,
        conversation_id: &str,
        name: &str,
        source_path: Option<&str>,
        max_documents: i64,
    ) -> StateResult<RagDocument> {
        self.write(|tx| {
            let exists: Option<i64> = tx
                .query_row("SELECT 1 FROM conversations WHERE id = ?1", [conversation_id], |r| {
                    r.get(0)
                })
                .optional()?;
            if exists.is_none() {
                return Err(StateError::not_found(
                    err("conversations.not_found").with("id", conversation_id),
                ));
            }
            let count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM rag_documents WHERE conversation_id = ?1",
                [conversation_id],
                |r| r.get(0),
            )?;
            if count >= max_documents {
                return Err(StateError::invalid(
                    err("rag.too_many_documents").num("max", max_documents),
                ));
            }
            let id = new_id();
            let now = now_ms();
            tx.execute(
                "INSERT INTO rag_documents (id, conversation_id, name, source_path, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![id, conversation_id, name, source_path, now],
            )?;
            get_doc(tx, &id)
        })
    }

    pub fn rag_document(&self, id: &str) -> StateResult<RagDocument> {
        self.read(|c| get_doc(c, id))
    }

    /// Documents of a conversation, oldest first.
    pub fn rag_documents(&self, conversation_id: &str) -> StateResult<Vec<RagDocument>> {
        self.read(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {DOC_COLS} FROM rag_documents d WHERE d.conversation_id = ?1
                 ORDER BY d.created_at, d.rid"
            ))?;
            let rows = st.query_map([conversation_id], doc_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn rag_set_status(&self, id: &str, status: &str, error: Option<&str>) -> StateResult<()> {
        check_status(status)?;
        self.write(|tx| {
            let n = tx.execute(
                "UPDATE rag_documents SET status = ?2, error = ?3, updated_at = ?4 WHERE id = ?1",
                params![id, status, error, now_ms()],
            )?;
            if n == 0 {
                return Err(StateError::not_found(
                    err("rag.document_not_found").with("id", id),
                ));
            }
            Ok(())
        })
    }

    /// Records the pages whose text came from OCR (empty clears the record).
    pub fn rag_set_ocr_pages(&self, id: &str, pages: &[u32]) -> StateResult<()> {
        let stored = (!pages.is_empty()).then(|| {
            pages
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        });
        self.write(|tx| {
            let n = tx.execute(
                "UPDATE rag_documents SET ocr_pages = ?2 WHERE id = ?1",
                params![id, stored],
            )?;
            if n == 0 {
                return Err(StateError::not_found(
                    err("rag.document_not_found").with("id", id),
                ));
            }
            Ok(())
        })
    }

    pub fn rag_set_mode(&self, id: &str, mode: &str) -> StateResult<RagDocument> {
        if !matches!(mode, "auto" | "full" | "retrieval") {
            return Err(StateError::invalid(
                err("rag.invalid_mode").with("mode", mode),
            ));
        }
        self.write(|tx| {
            let n = tx.execute(
                "UPDATE rag_documents SET mode = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, mode, now_ms()],
            )?;
            if n == 0 {
                return Err(StateError::not_found(
                    err("rag.document_not_found").with("id", id),
                ));
            }
            get_doc(tx, id)
        })
    }

    pub fn rag_delete_document(&self, id: &str) -> StateResult<()> {
        self.write(|tx| {
            let n = tx.execute("DELETE FROM rag_documents WHERE id = ?1", [id])?;
            if n == 0 {
                return Err(StateError::not_found(
                    err("rag.document_not_found").with("id", id),
                ));
            }
            Ok(())
        })
    }

    /// Stores the extracted text and its chunks (replacing earlier ones). Rejects a file the
    /// conversation already has (same SHA-256) and more than `max_chunks` chunks in the
    /// conversation.
    #[allow(clippy::too_many_arguments)]
    pub fn rag_store_extraction(
        &self,
        id: &str,
        kind: &str,
        size: i64,
        sha256: &str,
        pages: Option<i64>,
        content: &str,
        chunks: &[Chunk],
        max_chunks: usize,
    ) -> StateResult<RagDocument> {
        self.write(|tx| {
            let doc = get_doc(tx, id)?;
            let dup: Option<String> = tx
                .query_row(
                    "SELECT name FROM rag_documents
                     WHERE conversation_id = ?1 AND sha256 = ?2 AND id <> ?3",
                    params![doc.conversation_id, sha256, id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(other) = dup {
                return Err(StateError::invalid(
                    err("rag.duplicate")
                        .with("name", &doc.name)
                        .with("other", other),
                ));
            }
            let others: i64 = tx.query_row(
                "SELECT COUNT(*) FROM rag_chunks c JOIN rag_documents d ON d.id = c.document_id
                 WHERE d.conversation_id = ?1 AND d.id <> ?2",
                params![doc.conversation_id, id],
                |r| r.get(0),
            )?;
            if others as usize + chunks.len() > max_chunks {
                return Err(StateError::invalid(
                    err("rag.too_many_chunks")
                        .with("name", &doc.name)
                        .num("chunks", chunks.len() as u64)
                        .num("max", max_chunks as u64),
                ));
            }
            tx.execute("DELETE FROM rag_chunks WHERE document_id = ?1", [id])?;
            {
                let mut ins = tx.prepare(
                    "INSERT INTO rag_chunks (document_id, seq, text, page_start, page_end, heading)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )?;
                for (i, c) in chunks.iter().enumerate() {
                    ins.execute(params![
                        id,
                        i as i64,
                        c.text,
                        c.page_start,
                        c.page_end,
                        c.heading
                    ])?;
                }
            }
            tx.execute(
                "UPDATE rag_documents SET kind = ?2, size = ?3, sha256 = ?4, pages = ?5,
                     content = ?6, chars = ?7, embed_model = NULL, embed_dims = NULL,
                     updated_at = ?8
                 WHERE id = ?1",
                params![
                    id,
                    kind,
                    size,
                    sha256,
                    pages,
                    content,
                    content.chars().count() as i64,
                    now_ms()
                ],
            )?;
            get_doc(tx, id)
        })
    }

    /// Prepares embedding with `model`: a different model than the stored one drops the old
    /// vectors (re-index), the same model keeps them (resume).
    pub fn rag_begin_embedding(&self, id: &str, model: &str) -> StateResult<RagDocument> {
        self.write(|tx| {
            let doc = get_doc(tx, id)?;
            if doc.embed_model.as_deref() != Some(model) {
                tx.execute(
                    "UPDATE rag_chunks SET embedding = NULL WHERE document_id = ?1",
                    [id],
                )?;
                tx.execute(
                    "UPDATE rag_documents SET embed_model = ?2, embed_dims = NULL WHERE id = ?1",
                    params![id, model],
                )?;
            }
            tx.execute(
                "UPDATE rag_documents SET status = 'embedding', error = NULL, updated_at = ?2
                 WHERE id = ?1",
                params![id, now_ms()],
            )?;
            get_doc(tx, id)
        })
    }

    /// Up to `limit` chunks of a document without an embedding, in order.
    pub fn rag_pending_chunks(&self, id: &str, limit: usize) -> StateResult<Vec<PendingChunk>> {
        self.read(|c| {
            let mut st = c.prepare(
                "SELECT rid, text, heading FROM rag_chunks
                 WHERE document_id = ?1 AND embedding IS NULL ORDER BY seq LIMIT ?2",
            )?;
            let rows = st.query_map(params![id, limit as i64], |r| {
                Ok(PendingChunk {
                    chunk_id: r.get(0)?,
                    text: r.get(1)?,
                    heading: r.get(2)?,
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Stores unit vectors of a document's chunks; all must have the document's dimension.
    pub fn rag_store_embeddings(&self, id: &str, vectors: &[(i64, Vec<f32>)]) -> StateResult<()> {
        let Some(dims) = vectors.first().map(|(_, v)| v.len() as i64) else {
            return Ok(());
        };
        self.write(|tx| {
            let doc = get_doc(tx, id)?;
            if vectors.iter().any(|(_, v)| v.len() as i64 != dims)
                || doc.embed_dims.is_some_and(|d| d != dims)
            {
                return Err(StateError::invalid(
                    err("rag.dims_mismatch")
                        .with("name", &doc.name)
                        .num("expected", doc.embed_dims.unwrap_or(dims))
                        .num("got", dims),
                ));
            }
            {
                let mut up = tx.prepare(
                    "UPDATE rag_chunks SET embedding = ?3 WHERE rid = ?1 AND document_id = ?2",
                )?;
                for (rid, v) in vectors {
                    up.execute(params![rid, id, search::to_blob(v)])?;
                }
            }
            tx.execute(
                "UPDATE rag_documents SET embed_dims = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, dims, now_ms()],
            )?;
            Ok(())
        })
    }

    /// Unit vectors of the ready documents in `document_ids` embedded with `model`.
    pub fn rag_vectors(
        &self,
        document_ids: &[String],
        model: &str,
    ) -> StateResult<Vec<(i64, Vec<f32>)>> {
        self.read(|c| {
            let mut st = c.prepare(
                "SELECT c.rid, c.embedding FROM rag_chunks c JOIN rag_documents d ON d.id = c.document_id
                 WHERE d.id = ?1 AND d.embed_model = ?2 AND c.embedding IS NOT NULL",
            )?;
            let mut out = Vec::new();
            for id in document_ids {
                let rows = st.query_map(params![id, model], |r| {
                    let blob: Vec<u8> = r.get(1)?;
                    Ok((r.get::<_, i64>(0)?, search::from_blob(&blob)))
                })?;
                for row in rows {
                    out.push(row?);
                }
            }
            Ok(out)
        })
    }

    /// Keyword (BM25) ranking of chunks of `document_ids` for an FTS5 query, best first.
    pub fn rag_keyword(
        &self,
        document_ids: &[String],
        fts_query: &str,
        limit: usize,
    ) -> StateResult<Vec<i64>> {
        if document_ids.is_empty() {
            return Ok(Vec::new());
        }
        self.read(|c| {
            let marks = vec!["?"; document_ids.len()].join(", ");
            let sql = format!(
                "SELECT c.rid FROM rag_fts JOIN rag_chunks c ON c.rid = rag_fts.rowid
                 WHERE rag_fts MATCH ?1 AND c.document_id IN ({marks})
                 ORDER BY bm25(rag_fts) LIMIT {limit}"
            );
            let mut st = c.prepare(&sql)?;
            let mut args: Vec<&dyn rusqlite::ToSql> = vec![&fts_query];
            for id in document_ids {
                args.push(id);
            }
            let rows = st.query_map(args.as_slice(), |r| r.get::<_, i64>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Chunks by id (in the given order; unknown ids are skipped).
    pub fn rag_chunks(&self, ids: &[i64]) -> StateResult<Vec<ChunkHit>> {
        self.read(|c| {
            let mut st = c.prepare(
                "SELECT c.rid, c.document_id, d.name, c.seq, c.text, c.page_start, c.page_end, c.heading
                 FROM rag_chunks c JOIN rag_documents d ON d.id = c.document_id WHERE c.rid = ?1",
            )?;
            let mut out = Vec::new();
            for id in ids {
                let hit = st
                    .query_row([id], |r| {
                        Ok(ChunkHit {
                            chunk_id: r.get(0)?,
                            document_id: r.get(1)?,
                            document_name: r.get(2)?,
                            seq: r.get(3)?,
                            text: r.get(4)?,
                            page_start: r.get(5)?,
                            page_end: r.get(6)?,
                            heading: r.get(7)?,
                        })
                    })
                    .optional()?;
                out.extend(hit);
            }
            Ok(out)
        })
    }

    /// Extracted text of a document.
    pub fn rag_content(&self, id: &str) -> StateResult<String> {
        self.read(|c| {
            c.query_row(
                "SELECT content FROM rag_documents WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| StateError::not_found(err("rag.document_not_found").with("id", id)))
        })
    }

    /// Documents left in an active state by a job that no longer runs (app closed while
    /// indexing) become `cancelled`; they can be resumed.
    pub fn rag_reconcile(
        &self,
        conversation_id: &str,
        running: impl Fn(&str) -> bool,
    ) -> StateResult<usize> {
        let stale: Vec<String> = self
            .rag_documents(conversation_id)?
            .into_iter()
            .filter(|d| ACTIVE_STATUSES.contains(&d.status.as_str()) && !running(&d.id))
            .map(|d| d.id)
            .collect();
        for id in &stale {
            self.rag_set_status(id, "cancelled", None)?;
        }
        Ok(stale.len())
    }
}
