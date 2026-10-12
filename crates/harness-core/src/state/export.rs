//! Conversation export/import: versioned JSON (`harness.conversation.v1`, the whole
//! tree with every variant) and Markdown (the current path, for reading).

use super::conversations::{
    build_view, insert_variant, load_conversation, variant_from_row, Tree, VariantInput, ROLES,
    TITLE_SOURCES,
};
use super::{new_id, now_ms, StateDb, StateError, StateResult, MAX_DOCUMENTS_PER_CONVERSATION};
use crate::error_key::err;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const EXPORT_SCHEMA: &str = "harness.conversation.v1";
/// Upper bound for imported files (messages); protects against absurd inputs.
const MAX_IMPORT_MESSAGES: usize = 200_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationExport {
    pub schema: String,
    #[serde(default)]
    pub exported_at: i64,
    pub conversation: ExportedConversation,
    pub messages: Vec<ExportedMessage>,
    /// Documents attached for local RAG (`ready` ones, with their text and chunks). Absent in
    /// files written before documents were exported.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<ExportedDocument>,
}

/// A document of the conversation. A file export carries the text and chunks but no vectors:
/// an imported document is searchable by keywords at once and gets embeddings again when the
/// user indexes it with an embedding model. Duplicating or forking a conversation copies the
/// vectors as well (the `embedding` fields never reach a file).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ExportedDocument {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub size: i64,
    pub sha256: String,
    pub pages: Option<i64>,
    pub content: String,
    /// `auto | full | retrieval`.
    pub mode: String,
    pub embed_model: Option<String>,
    pub embed_dims: Option<i64>,
    pub created_at: i64,
    /// Where the file was read from (copies inside this database only).
    #[serde(skip)]
    pub source_path: Option<String>,
    pub chunks: Vec<ExportedChunk>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ExportedChunk {
    pub text: String,
    pub page_start: Option<i64>,
    pub page_end: Option<i64>,
    pub heading: Option<String>,
    /// Little-endian f32 vector (copies inside this database only).
    #[serde(skip)]
    pub embedding: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ExportedConversation {
    pub id: String,
    pub title: String,
    pub title_source: String,
    pub pinned: bool,
    pub favorite: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub server_id: Option<String>,
    pub model_alias: Option<String>,
    pub preset_id: Option<String>,
    pub system_prompt: String,
    pub settings: Value,
    pub metadata: Value,
    pub current_leaf_id: Option<String>,
    pub forked_from_conversation_id: Option<String>,
    pub forked_from_message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportedMessage {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub parent_variant_id: Option<String>,
    pub role: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub active_variant_id: Option<String>,
    pub variants: Vec<ExportedVariant>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ExportedVariant {
    pub id: String,
    pub content: String,
    pub reasoning: Option<String>,
    pub tool_calls: Option<Value>,
    pub stats: Option<Value>,
    pub finish_reason: Option<String>,
    pub stopped: bool,
    pub tool_limit_reached: bool,
    pub error: Option<String>,
    pub model_alias: Option<String>,
    pub created_at: i64,
    /// Client metadata (split-view side...). Absent in files written before schema v4.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Attachment metadata (the file itself is not part of the database or the export).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Attachment {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub mime: Option<String>,
    pub size: Option<i64>,
    pub sha256: Option<String>,
    pub path: Option<String>,
    pub created_at: i64,
}

/// Role headings etc. for the Markdown export (the UI passes its locale's words).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MarkdownLabels {
    pub user: String,
    pub assistant: String,
    pub system: String,
    pub tool: String,
    pub reasoning: String,
    pub tool_call: String,
    pub result: String,
    pub error: String,
    /// Heading of a compaction summary (a `system` message with `meta.compaction`).
    pub summary: String,
    /// Heading of the list of attached documents.
    pub documents: String,
}

impl Default for MarkdownLabels {
    fn default() -> Self {
        Self {
            user: "User".into(),
            assistant: "Assistant".into(),
            system: "System prompt".into(),
            tool: "Tool".into(),
            reasoning: "Reasoning".into(),
            tool_call: "Tool call".into(),
            result: "Result".into(),
            error: "Error".into(),
            summary: "Summary of earlier messages".into(),
            documents: "Documents".into(),
        }
    }
}

pub(crate) struct InsertOptions {
    pub title: Option<String>,
    /// Copy inside this database (duplicate/fork): same folder, not pinned/favorite.
    /// Otherwise (import): no folder, pinned/favorite kept from the file.
    pub keep_folder: bool,
    pub fresh_timestamps: bool,
    pub forked_from: Option<(String, String)>,
}

fn load_attachments(conn: &Connection, message_id: &str) -> StateResult<Vec<Attachment>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, kind, name, mime, size, sha256, path, created_at FROM attachments \
         WHERE message_id = ?1 ORDER BY created_at, id",
    )?;
    let rows = stmt
        .query_map([message_id], |r| {
            Ok(Attachment {
                id: r.get(0)?,
                kind: r.get(1)?,
                name: r.get(2)?,
                mime: r.get(3)?,
                size: r.get(4)?,
                sha256: r.get(5)?,
                path: r.get(6)?,
                created_at: r.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn exported_variant(conn: &Connection, id: &str) -> StateResult<ExportedVariant> {
    let v = super::conversations::load_variant(conn, id)?;
    Ok(ExportedVariant {
        id: v.id,
        content: v.content,
        reasoning: v.reasoning,
        tool_calls: v.tool_calls,
        stats: v.stats,
        finish_reason: v.finish_reason,
        stopped: v.stopped,
        tool_limit_reached: v.tool_limit_reached,
        error: v.error,
        model_alias: v.model_alias,
        created_at: v.created_at,
        meta: v.meta,
    })
}

/// The `ready` documents of a conversation with their chunks. `in_database`: a copy inside
/// this database (vectors and source paths included) rather than a file export.
fn read_documents(
    conn: &Connection,
    conversation_id: &str,
    in_database: bool,
) -> StateResult<Vec<ExportedDocument>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, name, kind, source_path, size, sha256, pages, content, mode, embed_model, \
         embed_dims, created_at FROM rag_documents \
         WHERE conversation_id = ?1 AND status = 'ready' ORDER BY created_at, rid",
    )?;
    let mut docs = stmt
        .query_map([conversation_id], |r| {
            Ok(ExportedDocument {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                source_path: r.get::<_, Option<String>>(3)?.filter(|_| in_database),
                size: r.get(4)?,
                sha256: r.get(5)?,
                pages: r.get(6)?,
                content: r.get(7)?,
                mode: r.get(8)?,
                embed_model: r.get::<_, Option<String>>(9)?.filter(|_| in_database),
                embed_dims: r.get::<_, Option<i64>>(10)?.filter(|_| in_database),
                created_at: r.get(11)?,
                chunks: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut chunks = conn.prepare_cached(
        "SELECT text, page_start, page_end, heading, embedding FROM rag_chunks \
         WHERE document_id = ?1 ORDER BY seq",
    )?;
    for d in &mut docs {
        d.chunks = chunks
            .query_map([&d.id], |r| {
                Ok(ExportedChunk {
                    text: r.get(0)?,
                    page_start: r.get(1)?,
                    page_end: r.get(2)?,
                    heading: r.get(3)?,
                    embedding: r.get::<_, Option<Vec<u8>>>(4)?.filter(|_| in_database),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
    }
    Ok(docs)
}

/// Copies the documents of an export into a new conversation (always `ready`).
fn insert_documents(
    conn: &Connection,
    conv_id: &str,
    docs: &[ExportedDocument],
) -> StateResult<()> {
    let now = now_ms();
    let mut total_chunks = 0usize;
    for d in docs.iter().take(MAX_DOCUMENTS_PER_CONVERSATION as usize) {
        // Same cap as indexing: brute-force search stays fast.
        if total_chunks + d.chunks.len() > crate::rag::MAX_CHUNKS_PER_CONVERSATION {
            break;
        }
        total_chunks += d.chunks.len();
        let id = new_id();
        let mode = if matches!(d.mode.as_str(), "auto" | "full" | "retrieval") {
            d.mode.as_str()
        } else {
            "auto"
        };
        let created = if d.created_at > 0 { d.created_at } else { now };
        // A copy without vectors has no embedding model either.
        let has_vectors = d.chunks.iter().any(|c| c.embedding.is_some());
        conn.execute(
            "INSERT INTO rag_documents (id, conversation_id, name, kind, source_path, size, sha256, \
             pages, content, chars, status, mode, embed_model, embed_dims, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'ready', ?11, ?12, ?13, ?14, ?15)",
            params![
                id,
                conv_id,
                d.name,
                if d.kind.is_empty() { "text" } else { d.kind.as_str() },
                d.source_path,
                d.size,
                d.sha256,
                d.pages,
                d.content,
                d.content.chars().count() as i64,
                mode,
                d.embed_model.as_ref().filter(|_| has_vectors),
                d.embed_dims.filter(|_| has_vectors),
                created,
                now,
            ],
        )?;
        let mut ins = conn.prepare_cached(
            "INSERT INTO rag_chunks (document_id, seq, text, page_start, page_end, heading, embedding) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for (i, c) in d.chunks.iter().enumerate() {
            ins.execute(params![
                id,
                i as i64,
                c.text,
                c.page_start,
                c.page_end,
                c.heading,
                c.embedding,
            ])?;
        }
    }
    Ok(())
}

/// Reads a conversation as an export document. With `path_to`, only the path from the
/// root to that message is included, one (the shown) variant per message. `in_database`
/// marks a copy that stays in this database (documents keep their vectors).
pub(crate) fn read_export(
    conn: &Connection,
    conversation_id: &str,
    path_to: Option<&str>,
    in_database: bool,
) -> StateResult<ConversationExport> {
    let c = load_conversation(conn, conversation_id)?;
    let tree = Tree::load(conn, conversation_id)?;
    let mut messages = Vec::new();
    match path_to {
        Some(target) => {
            if tree.get(target).is_none() {
                return Err(StateError::not_found(
                    err("conversations.message_not_found").with("id", target),
                ));
            }
            // The path shown when `target` is on screen: its ancestors through the current
            // view if possible, otherwise through their effective variants.
            let path = tree.path_to(target);
            let variants = tree.path_variants(&path);
            for (m, v) in path.iter().zip(variants) {
                let Some(v) = v else { break };
                messages.push(ExportedMessage {
                    id: m.id.clone(),
                    parent_id: m.parent_id.clone(),
                    parent_variant_id: m.parent_variant_id.clone(),
                    role: m.role.clone(),
                    created_at: m.created_at,
                    active_variant_id: Some(v.clone()),
                    variants: vec![exported_variant(conn, &v)?],
                    attachments: load_attachments(conn, &m.id)?,
                });
            }
        }
        None => {
            let mut stmt = conn.prepare_cached(
                "SELECT v.id, v.message_id, v.content, v.reasoning, v.tool_calls_json, v.stats_json, \
                 v.finish_reason, v.stopped, v.tool_limit_reached, v.error, v.model_alias, v.created_at, \
                 v.meta_json \
                 FROM message_variants v JOIN messages m ON m.id = v.message_id \
                 WHERE m.conversation_id = ?1 ORDER BY v.created_at, v.rid",
            )?;
            let mut by_message: HashMap<String, Vec<ExportedVariant>> = HashMap::new();
            for v in stmt.query_map([conversation_id], variant_from_row)? {
                let v = v?;
                by_message
                    .entry(v.message_id.clone())
                    .or_default()
                    .push(ExportedVariant {
                        id: v.id,
                        content: v.content,
                        reasoning: v.reasoning,
                        tool_calls: v.tool_calls,
                        stats: v.stats,
                        finish_reason: v.finish_reason,
                        stopped: v.stopped,
                        tool_limit_reached: v.tool_limit_reached,
                        error: v.error,
                        model_alias: v.model_alias,
                        created_at: v.created_at,
                        meta: v.meta,
                    });
            }
            for m in &tree.order {
                let variants = by_message.remove(&m.id).unwrap_or_default();
                if variants.is_empty() {
                    continue;
                }
                messages.push(ExportedMessage {
                    id: m.id.clone(),
                    parent_id: m.parent_id.clone(),
                    parent_variant_id: m.parent_variant_id.clone(),
                    role: m.role.clone(),
                    created_at: m.created_at,
                    active_variant_id: m.active_variant_id.clone(),
                    variants,
                    attachments: load_attachments(conn, &m.id)?,
                });
            }
        }
    }
    let current_leaf_id = match path_to {
        Some(t) => Some(t.to_string()),
        None => c.current_leaf_id.clone(),
    };
    Ok(ConversationExport {
        schema: EXPORT_SCHEMA.to_string(),
        exported_at: now_ms(),
        conversation: ExportedConversation {
            id: c.id,
            title: c.title,
            title_source: c.title_source,
            pinned: c.pinned,
            favorite: c.favorite,
            created_at: c.created_at,
            updated_at: c.updated_at,
            server_id: c.server_id,
            model_alias: c.model_alias,
            preset_id: c.preset_id,
            system_prompt: c.system_prompt,
            settings: c.settings,
            metadata: c.metadata,
            current_leaf_id,
            forked_from_conversation_id: c.forked_from_conversation_id,
            forked_from_message_id: c.forked_from_message_id,
        },
        messages,
        documents: read_documents(conn, conversation_id, in_database)?,
    })
}

/// Checks structure and returns the messages in parent-before-child order.
fn validated_order(e: &ConversationExport) -> StateResult<Vec<&ExportedMessage>> {
    if e.messages.len() > MAX_IMPORT_MESSAGES {
        return Err(StateError::invalid(
            err("export.too_many_messages")
                .num("count", e.messages.len() as u64)
                .num("max", MAX_IMPORT_MESSAGES as u64),
        ));
    }
    let mut ids = HashSet::new();
    let mut variant_ids = HashSet::new();
    for m in &e.messages {
        if m.id.is_empty() || !ids.insert(m.id.as_str()) {
            return Err(StateError::invalid(
                err("export.bad_message_id").with("id", &m.id),
            ));
        }
        if !ROLES.contains(&m.role.as_str()) {
            return Err(StateError::invalid(
                err("conversations.invalid_role").with("role", &m.role),
            ));
        }
        if m.variants.is_empty() {
            return Err(StateError::invalid(
                err("export.message_without_variants").with("id", &m.id),
            ));
        }
        for v in &m.variants {
            if v.id.is_empty() || !variant_ids.insert(v.id.as_str()) {
                return Err(StateError::invalid(
                    err("export.bad_variant_id").with("id", &v.id),
                ));
            }
        }
    }
    for m in &e.messages {
        if let Some(p) = &m.parent_id {
            if !ids.contains(p.as_str()) {
                return Err(StateError::invalid(
                    err("export.parent_missing")
                        .with("id", &m.id)
                        .with("parent", p),
                ));
            }
        }
    }
    // Kahn-style ordering; anything left over is part of a cycle.
    let mut children: HashMap<Option<&str>, Vec<&ExportedMessage>> = HashMap::new();
    for m in &e.messages {
        children.entry(m.parent_id.as_deref()).or_default().push(m);
    }
    let mut order = Vec::with_capacity(e.messages.len());
    let mut stack: Vec<&ExportedMessage> = children.get(&None).cloned().unwrap_or_default();
    while let Some(m) = stack.pop() {
        order.push(m);
        if let Some(kids) = children.get(&Some(m.id.as_str())) {
            stack.extend(kids.iter().copied());
        }
    }
    if order.len() != e.messages.len() {
        return Err(StateError::invalid(err("export.cycle")));
    }
    Ok(order)
}

/// Inserts an export document as a new conversation with fresh ids. Returns its id.
pub(crate) fn insert_export(
    conn: &Connection,
    e: &ConversationExport,
    opts: &InsertOptions,
) -> StateResult<String> {
    let order = validated_order(e)?;
    let c = &e.conversation;
    let now = now_ms();
    let conv_id = new_id();
    let (created_at, updated_at) = if opts.fresh_timestamps || c.created_at <= 0 {
        (now, now)
    } else {
        (c.created_at, c.updated_at.max(c.created_at))
    };
    let (title, title_source) = match &opts.title {
        Some(t) => (t.clone(), "user".to_string()),
        None => {
            let src = if TITLE_SOURCES.contains(&c.title_source.as_str()) {
                c.title_source.clone()
            } else if c.title.trim().is_empty() {
                "none".into()
            } else {
                "user".into()
            };
            (c.title.clone(), src)
        }
    };
    let title: String = title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect();
    let title_source = if title.is_empty() {
        "none".to_string()
    } else {
        title_source
    };
    let folder_id: Option<String> = if opts.keep_folder {
        conn.query_row(
            "SELECT folder_id FROM conversations WHERE id = ?1",
            [&c.id],
            |r| r.get(0),
        )
        .unwrap_or(None)
    } else {
        None
    };
    let obj = |v: &Value| match v {
        Value::Object(_) => v.to_string(),
        _ => "{}".to_string(),
    };
    let (ff_conv, ff_msg) = match &opts.forked_from {
        Some((cid, mid)) => (Some(cid.clone()), Some(mid.clone())),
        None => (
            c.forked_from_conversation_id.clone(),
            c.forked_from_message_id.clone(),
        ),
    };
    conn.execute(
        "INSERT INTO conversations (id, title, title_source, folder_id, pinned, favorite, created_at, \
         updated_at, server_id, model_alias, preset_id, system_prompt, settings_json, metadata_json, \
         forked_from_conversation_id, forked_from_message_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            conv_id,
            title,
            title_source,
            folder_id,
            (!opts.keep_folder && c.pinned) as i64,
            (!opts.keep_folder && c.favorite) as i64,
            created_at,
            updated_at,
            c.server_id,
            c.model_alias,
            c.preset_id,
            c.system_prompt,
            obj(&c.settings),
            obj(&c.metadata),
            ff_conv,
            ff_msg,
        ],
    )?;

    let mut msg_map: HashMap<&str, String> = HashMap::new();
    let mut var_map: HashMap<&str, String> = HashMap::new();
    for m in order {
        let new_mid = new_id();
        let parent = m.parent_id.as_deref().and_then(|p| msg_map.get(p)).cloned();
        let parent_variant = m
            .parent_variant_id
            .as_deref()
            .and_then(|v| var_map.get(v))
            .cloned();
        let msg_created = if m.created_at > 0 { m.created_at } else { now };
        conn.execute(
            "INSERT INTO messages (id, conversation_id, parent_id, parent_variant_id, role, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![new_mid, conv_id, parent, parent_variant, m.role, msg_created],
        )?;
        let mut active = None;
        for v in &m.variants {
            let new_vid = new_id();
            insert_variant(
                conn,
                &new_vid,
                &new_mid,
                &VariantInput {
                    content: v.content.clone(),
                    reasoning: v.reasoning.clone(),
                    tool_calls: v.tool_calls.clone(),
                    stats: v.stats.clone(),
                    finish_reason: v.finish_reason.clone(),
                    stopped: v.stopped,
                    tool_limit_reached: v.tool_limit_reached,
                    error: v.error.clone(),
                    model_alias: v.model_alias.clone(),
                    // A compaction marker points at messages of this conversation by id.
                    meta: crate::compaction::remap_meta(v.meta.as_ref(), |id| {
                        msg_map.get(id).cloned()
                    }),
                },
                if v.created_at > 0 {
                    v.created_at
                } else {
                    msg_created
                },
            )?;
            if m.active_variant_id.as_deref() == Some(v.id.as_str()) {
                active = Some(new_vid.clone());
            }
            var_map.insert(v.id.as_str(), new_vid);
        }
        let active = active.or_else(|| {
            m.variants
                .last()
                .and_then(|v| var_map.get(v.id.as_str()))
                .cloned()
        });
        conn.execute(
            "UPDATE messages SET active_variant_id = ?2 WHERE id = ?1",
            params![new_mid, active],
        )?;
        for a in &m.attachments {
            conn.execute(
                "INSERT INTO attachments (id, message_id, kind, name, mime, size, sha256, path, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8)",
                params![
                    new_id(),
                    new_mid,
                    a.kind,
                    a.name,
                    a.mime,
                    a.size,
                    a.sha256,
                    if a.created_at > 0 { a.created_at } else { msg_created },
                ],
            )?;
        }
        msg_map.insert(m.id.as_str(), new_mid);
    }
    insert_documents(conn, &conv_id, &e.documents)?;
    let leaf = match c.current_leaf_id.as_deref() {
        // Rewound to before the first message: stays so.
        Some(super::conversations::EMPTY_LEAF) => {
            Some(super::conversations::EMPTY_LEAF.to_string())
        }
        Some(l) => msg_map.get(l).cloned(),
        None => None,
    };
    conn.execute(
        "UPDATE conversations SET current_leaf_id = ?2 WHERE id = ?1",
        params![conv_id, leaf],
    )?;
    Ok(conv_id)
}

fn quote_block(text: &str) -> String {
    text.lines()
        .map(|l| {
            if l.is_empty() {
                ">".to_string()
            } else {
                format!("> {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Longest run of backticks in `text` + 1 (at least 3), so fenced blocks never break.
fn fence_for(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for ch in text.chars() {
        if ch == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat((longest + 1).max(3))
}

impl StateDb {
    /// The whole conversation tree as `harness.conversation.v1` JSON.
    pub fn export_json(&self, conversation_id: &str) -> StateResult<ConversationExport> {
        self.read(|c| read_export(c, conversation_id, None, false))
    }

    /// Imports an export document as a new conversation (new ids, original timestamps).
    pub fn import_json(&self, e: &ConversationExport) -> StateResult<super::Conversation> {
        if e.schema != EXPORT_SCHEMA {
            return Err(StateError::invalid(
                err("export.unsupported_format")
                    .with("found", &e.schema)
                    .with("expected", EXPORT_SCHEMA),
            ));
        }
        self.write(|tx| {
            let id = insert_export(
                tx,
                e,
                &InsertOptions {
                    title: None,
                    keep_folder: false,
                    fresh_timestamps: false,
                    forked_from: None,
                },
            )?;
            load_conversation(tx, &id)
        })
    }

    /// Parses and imports JSON text.
    pub fn import_json_str(&self, text: &str) -> StateResult<super::Conversation> {
        let value: Value = serde_json::from_str(text)
            .map_err(|e| StateError::invalid(err("json.invalid").with("error", e)))?;
        let schema = value.get("schema").and_then(Value::as_str).unwrap_or("");
        if schema != EXPORT_SCHEMA {
            return Err(StateError::invalid(
                err("export.unsupported_format")
                    .with("found", schema)
                    .with("expected", EXPORT_SCHEMA),
            ));
        }
        let e: ConversationExport = serde_json::from_value(value)
            .map_err(|e| StateError::invalid(err("export.invalid_file").with("error", e)))?;
        self.import_json(&e)
    }

    /// The current path as Markdown.
    pub fn export_markdown(
        &self,
        conversation_id: &str,
        labels: &MarkdownLabels,
    ) -> StateResult<String> {
        self.read(|c| {
            let view = build_view(c, conversation_id)?;
            let conv = &view.conversation;
            let mut out = String::new();
            let title = if conv.title.trim().is_empty() {
                "Harness Studio"
            } else {
                conv.title.as_str()
            };
            out.push_str(&format!("# {title}\n\n"));
            if let Some(model) = &conv.model_alias {
                out.push_str(&format!("`{model}`\n\n"));
            }
            if !conv.system_prompt.trim().is_empty() {
                out.push_str(&format!(
                    "**{}**\n\n{}\n\n",
                    labels.system,
                    quote_block(conv.system_prompt.trim())
                ));
            }
            let mut names = c.prepare_cached(
                "SELECT name FROM rag_documents WHERE conversation_id = ?1 AND status = 'ready'                  ORDER BY created_at, rid",
            )?;
            let names = names
                .query_map([conversation_id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if !names.is_empty() {
                out.push_str(&format!("**{}**\n\n", labels.documents));
                for n in &names {
                    out.push_str(&format!("- {n}\n"));
                }
                out.push('\n');
            }
            for m in &view.messages {
                let v = &m.variant;
                let heading = match m.role.as_str() {
                    "user" => &labels.user,
                    "assistant" => &labels.assistant,
                    "system" if crate::compaction::message_marker(m).is_some() => &labels.summary,
                    "system" => &labels.system,
                    _ => &labels.tool,
                };
                out.push_str(&format!("---\n\n### {heading}\n\n"));
                if m.compare.len() > 1 {
                    // A split turn: every side, each under its own heading.
                    for side in &m.compare {
                        out.push_str(&format!("#### {}\n\n", compare_heading(side)));
                        push_variant_markdown(&mut out, side, labels);
                    }
                } else {
                    push_variant_markdown(&mut out, v, labels);
                }
            }
            Ok(out)
        })
    }
}

/// `A · qwen3-4b · Precise` for a split-view side (letter, model, preset label).
fn compare_heading(v: &super::Variant) -> String {
    let c = v.meta.as_ref().and_then(|m| m.get("compare"));
    let text = |k: &str| {
        c.and_then(|c| c.get(k))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let mut parts = vec![text("side").unwrap_or("?").to_uppercase()];
    if let Some(model) = text("model").or(v.model_alias.as_deref()) {
        parts.push(format!("`{model}`"));
    }
    if let Some(preset) = text("preset_name") {
        parts.push(preset.to_string());
    }
    parts.join(" · ")
}

fn push_variant_markdown(out: &mut String, v: &super::Variant, labels: &MarkdownLabels) {
    if let Some(r) = v.reasoning.as_deref().filter(|r| !r.trim().is_empty()) {
        out.push_str(&format!(
            "<details>\n<summary>{}</summary>\n\n{}\n\n</details>\n\n",
            labels.reasoning,
            r.trim()
        ));
    }
    if !v.content.trim().is_empty() {
        out.push_str(v.content.trim_end());
        out.push_str("\n\n");
    }
    if let Some(Value::Array(calls)) = &v.tool_calls {
        for call in calls {
            let name = call.get("name").and_then(Value::as_str).unwrap_or("?");
            let args = call.get("arguments").and_then(Value::as_str).unwrap_or("");
            let result = call.get("result").and_then(Value::as_str).unwrap_or("");
            let fence = fence_for(args);
            out.push_str(&format!(
                "**{}:** `{name}`\n\n{fence}\n{args}\n{fence}\n\n",
                labels.tool_call
            ));
            if !result.is_empty() {
                let fence = fence_for(result);
                out.push_str(&format!(
                    "**{}:**\n\n{fence}\n{result}\n{fence}\n\n",
                    labels.result
                ));
            }
        }
    }
    if let Some(err) = v.error.as_deref().filter(|e| !e.is_empty()) {
        out.push_str(&format!("**{}:** {}\n\n", labels.error, err));
    }
}
