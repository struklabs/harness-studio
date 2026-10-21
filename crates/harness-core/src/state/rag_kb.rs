//! Storage of knowledge bases for local RAG (schema v8; see `crate::rag::kb`).
//!
//! A knowledge base owns a hidden conversation row with the same id; its documents are the
//! ordinary `rag_documents` of that row.

use super::{now_ms, NewConversation, StateDb, StateError, StateResult};
use crate::error_key::err;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// A named, reusable set of documents (without its documents).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeBaseRow {
    pub id: String,
    pub name: String,
    /// Folder the base was made from (new files of it can be scanned again).
    pub folder: Option<String>,
    /// Embedding model (alias) of the index; `None` = keyword search only.
    pub embed_model: Option<String>,
    /// Vector size of the index.
    pub embed_dims: Option<i64>,
    /// OCR model (alias) that reads the PDF pages without a text layer; `None` = no OCR.
    pub ocr_model: Option<String>,
    /// Total milliseconds spent indexing this base.
    pub index_ms: i64,
    pub indexed_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

const KB_COLS: &str =
    "id, name, folder, embed_model, embed_dims, index_ms, indexed_at, created_at, updated_at, ocr_model";

fn kb_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<KnowledgeBaseRow> {
    Ok(KnowledgeBaseRow {
        id: r.get(0)?,
        name: r.get(1)?,
        folder: r.get(2)?,
        embed_model: r.get(3)?,
        embed_dims: r.get(4)?,
        index_ms: r.get(5)?,
        indexed_at: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
        ocr_model: r.get(9)?,
    })
}

fn get_kb(conn: &Connection, id: &str) -> StateResult<KnowledgeBaseRow> {
    conn.query_row(
        &format!("SELECT {KB_COLS} FROM rag_kbs WHERE id = ?1"),
        [id],
        kb_row,
    )
    .optional()?
    .ok_or_else(|| StateError::not_found(err("rag.kb_not_found").with("id", id)))
}

/// Knowledge bases a user may have.
pub const MAX_KBS: i64 = 50;

impl StateDb {
    /// A new, empty knowledge base.
    pub fn kb_create(&self, name: &str, folder: Option<&str>) -> StateResult<KnowledgeBaseRow> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StateError::invalid(err("rag.kb_name_empty")));
        }
        let count: i64 =
            self.read(|c| Ok(c.query_row("SELECT COUNT(*) FROM rag_kbs", [], |r| r.get(0))?))?;
        if count >= MAX_KBS {
            return Err(StateError::invalid(
                err("rag.kb_too_many").num("max", MAX_KBS),
            ));
        }
        let conv = self.create_conversation(&NewConversation {
            title: Some(format!("Knowledge base: {name}")),
            title_source: Some("user".into()),
            ..Default::default()
        })?;
        let now = now_ms();
        let made = self.write(|tx| {
            tx.execute(
                "INSERT INTO rag_kbs (id, name, folder, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![conv.id, name, folder, now],
            )?;
            get_kb(tx, &conv.id)
        });
        if made.is_err() {
            let _ = self.delete_conversation(&conv.id);
        }
        made
    }

    /// Sets the OCR model of the base (`None` or an empty alias = no OCR).
    pub fn kb_set_ocr_model(&self, id: &str, alias: Option<&str>) -> StateResult<KnowledgeBaseRow> {
        let alias = alias.map(str::trim).filter(|a| !a.is_empty());
        self.write(|tx| {
            let n = tx.execute(
                "UPDATE rag_kbs SET ocr_model = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, alias, now_ms()],
            )?;
            if n == 0 {
                return Err(StateError::not_found(
                    err("rag.kb_not_found").with("id", id),
                ));
            }
            get_kb(tx, id)
        })
    }

    pub fn kb_get(&self, id: &str) -> StateResult<KnowledgeBaseRow> {
        self.read(|c| get_kb(c, id))
    }

    /// Knowledge bases, newest first.
    pub fn kb_list(&self) -> StateResult<Vec<KnowledgeBaseRow>> {
        self.read(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {KB_COLS} FROM rag_kbs ORDER BY created_at DESC, rowid DESC"
            ))?;
            let rows = st.query_map([], kb_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Whether `conversation_id` is the hidden conversation of a knowledge base.
    pub fn rag_is_kb(&self, conversation_id: &str) -> bool {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT 1 FROM rag_kbs WHERE id = ?1",
                [conversation_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
        })
        .unwrap_or(false)
    }

    pub fn kb_rename(&self, id: &str, name: &str) -> StateResult<KnowledgeBaseRow> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StateError::invalid(err("rag.kb_name_empty")));
        }
        self.write(|tx| {
            let n = tx.execute(
                "UPDATE rag_kbs SET name = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, name, now_ms()],
            )?;
            if n == 0 {
                return Err(StateError::not_found(
                    err("rag.kb_not_found").with("id", id),
                ));
            }
            get_kb(tx, id)
        })
    }

    /// Deletes the base with all its documents and chunks.
    pub fn kb_delete(&self, id: &str) -> StateResult<()> {
        self.kb_get(id)?;
        self.delete_conversation(id)
    }

    /// Records the embedding model of the index and `added_ms` of indexing time (a finished
    /// or cancelled run).
    pub fn kb_record_index(
        &self,
        id: &str,
        model: Option<&str>,
        dims: Option<i64>,
        added_ms: i64,
    ) -> StateResult<KnowledgeBaseRow> {
        self.write(|tx| {
            let n = tx.execute(
                "UPDATE rag_kbs SET embed_model = ?2, embed_dims = ?3,
                     index_ms = index_ms + ?4, indexed_at = ?5, updated_at = ?5
                 WHERE id = ?1",
                params![id, model, dims, added_ms.max(0), now_ms()],
            )?;
            if n == 0 {
                return Err(StateError::not_found(
                    err("rag.kb_not_found").with("id", id),
                ));
            }
            get_kb(tx, id)
        })
    }

    /// Attaches a knowledge base to a conversation (idempotent).
    pub fn kb_attach(&self, conversation_id: &str, kb_id: &str) -> StateResult<()> {
        self.write(|tx| {
            get_kb(tx, kb_id)?;
            let exists: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM conversations WHERE id = ?1",
                    [conversation_id],
                    |r| r.get(0),
                )
                .optional()?;
            if exists.is_none() {
                return Err(StateError::not_found(
                    err("conversations.not_found").with("id", conversation_id),
                ));
            }
            tx.execute(
                "INSERT OR IGNORE INTO rag_conversation_kbs (conversation_id, kb_id, created_at)
                 VALUES (?1, ?2, ?3)",
                params![conversation_id, kb_id, now_ms()],
            )?;
            Ok(())
        })
    }

    pub fn kb_detach(&self, conversation_id: &str, kb_id: &str) -> StateResult<()> {
        self.write(|tx| {
            tx.execute(
                "DELETE FROM rag_conversation_kbs WHERE conversation_id = ?1 AND kb_id = ?2",
                params![conversation_id, kb_id],
            )?;
            Ok(())
        })
    }

    /// Knowledge bases attached to a conversation, in attach order.
    pub fn kb_attached(&self, conversation_id: &str) -> StateResult<Vec<KnowledgeBaseRow>> {
        self.read(|c| {
            let mut st = c.prepare(
                "SELECT k.id, k.name, k.folder, k.embed_model, k.embed_dims, k.index_ms,
                        k.indexed_at, k.created_at, k.updated_at, k.ocr_model
                 FROM rag_conversation_kbs a JOIN rag_kbs k ON k.id = a.kb_id
                 WHERE a.conversation_id = ?1 ORDER BY a.created_at, k.rowid",
            )?;
            let rows = st.query_map([conversation_id], kb_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Source paths already in a knowledge base.
    pub fn kb_source_paths(&self, kb_id: &str) -> StateResult<std::collections::HashSet<String>> {
        self.read(|c| {
            let mut st = c.prepare(
                "SELECT source_path FROM rag_documents
                 WHERE conversation_id = ?1 AND source_path IS NOT NULL",
            )?;
            let rows = st.query_map([kb_id], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }
}
