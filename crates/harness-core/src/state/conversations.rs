//! Conversation service: folders, conversations, the message tree and its variants.
//! Plain Rust over [`StateDb`], shared by the GUI and future CLI/API/MCP surfaces.

use super::{
    check_client_id, get_flag, new_id, now_ms, set_flag, StateDb, StateError, StateResult,
};
use crate::error_key::err;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const TITLE_SOURCES: &[&str] = &["none", "fallback", "auto", "user"];
pub const ROLES: &[&str] = &["system", "user", "assistant", "tool"];
const MAX_TITLE_CHARS: usize = 200;
const LEGACY_IMPORT_FLAG: &str = "legacy_playground_v1_imported";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub sort: i64,
    pub created_at: i64,
}

/// One row of the conversation list.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub title_source: String,
    pub folder_id: Option<String>,
    pub pinned: bool,
    pub favorite: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_opened_at: Option<i64>,
    pub model_alias: Option<String>,
    /// Matching text fragment when the list comes from a search.
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub title_source: String,
    pub folder_id: Option<String>,
    pub pinned: bool,
    pub favorite: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_opened_at: Option<i64>,
    pub server_id: Option<String>,
    pub model_alias: Option<String>,
    pub preset_id: Option<String>,
    pub system_prompt: String,
    /// Sampling snapshot (opaque JSON object owned by the client).
    pub settings: Value,
    pub metadata: Value,
    pub current_leaf_id: Option<String>,
    pub forked_from_conversation_id: Option<String>,
    pub forked_from_message_id: Option<String>,
}

/// Content of a new or updated variant.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VariantInput {
    #[serde(default)]
    pub content: String,
    pub reasoning: Option<String>,
    /// Tool calls of this turn (id, name, arguments, status, result…).
    pub tool_calls: Option<Value>,
    /// Response stats (tokens, TTFT, t/s, cache…).
    pub stats: Option<Value>,
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub stopped: bool,
    #[serde(default)]
    pub tool_limit_reached: bool,
    pub error: Option<String>,
    pub model_alias: Option<String>,
    /// Client metadata (a JSON object), e.g. the split-view side (`compare`, see
    /// [`compare_of`]). Not interpreted otherwise.
    #[serde(default)]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = MessageVariant))]
pub struct Variant {
    pub id: String,
    pub message_id: String,
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
    pub meta: Option<Value>,
}

/// A message on the current path, with its shown variant and navigation info.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PathMessage {
    pub id: String,
    pub parent_id: Option<String>,
    pub parent_variant_id: Option<String>,
    pub role: String,
    pub created_at: i64,
    pub variant: Variant,
    /// All variants of this message, oldest first.
    pub variant_ids: Vec<String>,
    pub variant_index: usize,
    /// Messages sharing this message's parent (and parent variant), oldest first.
    pub sibling_ids: Vec<String>,
    pub sibling_index: usize,
    /// Split view: every variant of the shown variant's comparison group (sides `a`, `b`),
    /// in side order. Empty when the shown variant is not part of a comparison.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub compare: Vec<Variant>,
    /// Split view: one variant id per comparison group of this message (the side-`a` variant),
    /// oldest group first. More than one group means the turn was regenerated; the shown
    /// group is the one `compare` belongs to. Empty when the message has no comparison.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub compare_groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConversationView {
    pub conversation: Conversation,
    /// Root → current leaf.
    pub messages: Vec<PathMessage>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewConversation {
    pub title: Option<String>,
    pub title_source: Option<String>,
    pub folder_id: Option<String>,
    pub server_id: Option<String>,
    pub model_alias: Option<String>,
    pub preset_id: Option<String>,
    pub system_prompt: Option<String>,
    pub settings: Option<Value>,
    pub metadata: Option<Value>,
}

/// Partial update; `None` leaves a field unchanged, `Some("")` clears a nullable text.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConversationPatch {
    pub system_prompt: Option<String>,
    pub settings: Option<Value>,
    pub metadata: Option<Value>,
    pub server_id: Option<String>,
    pub model_alias: Option<String>,
    pub preset_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ListQuery {
    pub folder_id: Option<String>,
    /// Only conversations outside any folder.
    pub unfiled: bool,
    pub pinned_only: bool,
    pub favorites_only: bool,
    /// Full-text search over titles and message contents.
    pub query: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppendMessage {
    pub conversation_id: String,
    /// `None` = a root message.
    pub parent_id: Option<String>,
    pub role: String,
    pub variant: VariantInput,
    /// Optional client-chosen ids (see `check_client_id`).
    pub id: Option<String>,
    pub variant_id: Option<String>,
    /// Attach the message to its parent as a whole instead of the parent's active variant
    /// (`parent_variant_id` stays empty): the continuation of a split turn belongs to every
    /// side, so it stays on the path whichever side is shown.
    #[serde(default)]
    pub any_parent_variant: bool,
}

/// A linear chat to store as a new conversation (e.g. the old single-chat localStorage).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LinearImport {
    pub conversation: NewConversation,
    pub messages: Vec<LinearMessage>,
}

/// Browser-owned Playground state moved into `state.db` on first start.
#[derive(Debug, Clone, Deserialize)]
pub struct PlaygroundStateImport {
    pub conversation: Option<LinearImport>,
    #[serde(default)]
    pub values: Vec<PlaygroundStateValue>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaygroundStateValue {
    pub key: String,
    pub value: String,
}

/// The client removes only local values that were read back byte-for-byte.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlaygroundStateImportResult {
    pub conversation_verified: bool,
    pub verified_keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LinearMessage {
    pub role: String,
    pub variant: VariantInput,
}

// ---------------------------------------------------------------------------
// Row helpers
// ---------------------------------------------------------------------------

const CONV_COLS: &str = "id, title, title_source, folder_id, pinned, favorite, created_at, \
    updated_at, last_opened_at, server_id, model_alias, preset_id, system_prompt, settings_json, \
    metadata_json, current_leaf_id, forked_from_conversation_id, forked_from_message_id";

const VARIANT_COLS: &str = "id, message_id, content, reasoning, tool_calls_json, stats_json, \
    finish_reason, stopped, tool_limit_reached, error, model_alias, created_at, meta_json";

fn parse_json(text: Option<String>) -> Option<Value> {
    text.and_then(|t| serde_json::from_str(&t).ok())
}

fn object_or_empty(v: Option<Value>) -> Value {
    match v {
        Some(Value::Object(m)) => Value::Object(m),
        _ => Value::Object(Default::default()),
    }
}

fn conversation_from_row(r: &Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: r.get(0)?,
        title: r.get(1)?,
        title_source: r.get(2)?,
        folder_id: r.get(3)?,
        pinned: r.get::<_, i64>(4)? != 0,
        favorite: r.get::<_, i64>(5)? != 0,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
        last_opened_at: r.get(8)?,
        server_id: r.get(9)?,
        model_alias: r.get(10)?,
        preset_id: r.get(11)?,
        system_prompt: r.get(12)?,
        settings: object_or_empty(parse_json(r.get(13)?)),
        metadata: object_or_empty(parse_json(r.get(14)?)),
        current_leaf_id: r.get(15)?,
        forked_from_conversation_id: r.get(16)?,
        forked_from_message_id: r.get(17)?,
    })
}

fn summary_of(c: Conversation) -> ConversationSummary {
    ConversationSummary {
        id: c.id,
        title: c.title,
        title_source: c.title_source,
        folder_id: c.folder_id,
        pinned: c.pinned,
        favorite: c.favorite,
        created_at: c.created_at,
        updated_at: c.updated_at,
        last_opened_at: c.last_opened_at,
        model_alias: c.model_alias,
        snippet: None,
    }
}

pub(crate) fn variant_from_row(r: &Row<'_>) -> rusqlite::Result<Variant> {
    Ok(Variant {
        id: r.get(0)?,
        message_id: r.get(1)?,
        content: r.get(2)?,
        reasoning: r.get(3)?,
        tool_calls: parse_json(r.get(4)?),
        stats: parse_json(r.get(5)?),
        finish_reason: r.get(6)?,
        stopped: r.get::<_, i64>(7)? != 0,
        tool_limit_reached: r.get::<_, i64>(8)? != 0,
        error: r.get(9)?,
        model_alias: r.get(10)?,
        created_at: r.get(11)?,
        meta: parse_json(r.get(12)?).filter(Value::is_object),
    })
}

/// Split-view side of a variant: `meta.compare = {group, side, ...}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompareTag {
    /// One split turn (both sides share it).
    pub group: String,
    /// `a`, `b`.
    pub side: String,
}

/// The comparison tag of a variant's metadata, if any.
pub fn compare_of(meta: Option<&Value>) -> Option<CompareTag> {
    let c = meta?.get("compare")?;
    let group = c.get("group")?.as_str().filter(|s| !s.is_empty())?;
    let side = c.get("side")?.as_str().filter(|s| !s.is_empty())?;
    Some(CompareTag {
        group: group.to_string(),
        side: side.to_string(),
    })
}

fn json_text(v: &Option<Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.to_string()),
    }
}

/// Variant metadata is a non-empty object; anything else is not stored.
fn meta_text(v: &Option<Value>) -> Option<String> {
    match v {
        Some(Value::Object(m)) if !m.is_empty() => Some(Value::Object(m.clone()).to_string()),
        _ => None,
    }
}

fn empty_to_none(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn clean_title(title: &str) -> String {
    let one_line = title.split_whitespace().collect::<Vec<_>>().join(" ");
    one_line.chars().take(MAX_TITLE_CHARS).collect()
}

fn check_title_source(s: &str) -> StateResult<()> {
    if TITLE_SOURCES.contains(&s) {
        Ok(())
    } else {
        Err(StateError::invalid(
            err("conversations.invalid_title_source").with("value", s),
        ))
    }
}

fn check_role(role: &str) -> StateResult<()> {
    if ROLES.contains(&role) {
        Ok(())
    } else {
        Err(StateError::invalid(
            err("conversations.invalid_role").with("role", role),
        ))
    }
}

pub(crate) fn load_conversation(conn: &Connection, id: &str) -> StateResult<Conversation> {
    conn.query_row(
        &format!("SELECT {CONV_COLS} FROM conversations WHERE id = ?1"),
        [id],
        conversation_from_row,
    )
    .optional()?
    .ok_or_else(|| StateError::not_found(err("conversations.not_found").with("id", id)))
}

pub(crate) fn load_variant(conn: &Connection, id: &str) -> StateResult<Variant> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {VARIANT_COLS} FROM message_variants WHERE id = ?1"
    ))?;
    stmt.query_row([id], variant_from_row)
        .optional()?
        .ok_or_else(|| StateError::not_found(err("conversations.variant_not_found").with("id", id)))
}

pub(crate) fn insert_variant(
    conn: &Connection,
    id: &str,
    message_id: &str,
    v: &VariantInput,
    created_at: i64,
) -> StateResult<()> {
    conn.prepare_cached(
        "INSERT INTO message_variants (id, message_id, content, reasoning, tool_calls_json, \
         stats_json, finish_reason, stopped, tool_limit_reached, error, model_alias, created_at, \
         meta_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?
    .execute(params![
        id,
        message_id,
        v.content,
        v.reasoning,
        json_text(&v.tool_calls),
        json_text(&v.stats),
        v.finish_reason,
        v.stopped as i64,
        v.tool_limit_reached as i64,
        v.error,
        v.model_alias,
        created_at,
        meta_text(&v.meta),
    ])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Message tree
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct MsgRow {
    pub id: String,
    pub parent_id: Option<String>,
    pub parent_variant_id: Option<String>,
    pub role: String,
    pub created_at: i64,
    pub active_variant_id: Option<String>,
}

/// All messages of one conversation (ids only for variants), in creation order.
pub(crate) struct Tree {
    pub order: Vec<MsgRow>,
    index: HashMap<String, usize>,
    pub variants: HashMap<String, Vec<String>>,
}

impl Tree {
    pub fn load(conn: &Connection, conversation_id: &str) -> StateResult<Tree> {
        let mut stmt = conn.prepare_cached(
            "SELECT id, parent_id, parent_variant_id, role, created_at, active_variant_id \
             FROM messages WHERE conversation_id = ?1 ORDER BY created_at, rowid",
        )?;
        let order = stmt
            .query_map([conversation_id], |r| {
                Ok(MsgRow {
                    id: r.get(0)?,
                    parent_id: r.get(1)?,
                    parent_variant_id: r.get(2)?,
                    role: r.get(3)?,
                    created_at: r.get(4)?,
                    active_variant_id: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let index = order
            .iter()
            .enumerate()
            .map(|(i, m)| (m.id.clone(), i))
            .collect();
        let mut variants: HashMap<String, Vec<String>> = HashMap::new();
        let mut vstmt = conn.prepare_cached(
            "SELECT v.id, v.message_id FROM message_variants v \
             JOIN messages m ON m.id = v.message_id \
             WHERE m.conversation_id = ?1 ORDER BY v.created_at, v.rid",
        )?;
        let rows = vstmt.query_map([conversation_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (vid, mid) = row?;
            variants.entry(mid).or_default().push(vid);
        }
        Ok(Tree {
            order,
            index,
            variants,
        })
    }

    pub fn get(&self, id: &str) -> Option<&MsgRow> {
        self.index.get(id).map(|&i| &self.order[i])
    }

    pub fn variant_ids(&self, id: &str) -> &[String] {
        self.variants.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The active variant if it still exists, else the newest one.
    pub fn effective_variant(&self, id: &str) -> Option<&str> {
        let list = self.variant_ids(id);
        let msg = self.get(id)?;
        match &msg.active_variant_id {
            Some(a) if list.iter().any(|v| v == a) => Some(a.as_str()),
            _ => list.last().map(String::as_str),
        }
    }

    /// Children of `parent` that belong to `parent_variant` (children without a recorded
    /// parent variant belong to every variant).
    pub fn children(&self, parent: Option<&str>, parent_variant: Option<&str>) -> Vec<&MsgRow> {
        self.order
            .iter()
            .filter(|m| m.parent_id.as_deref() == parent)
            .filter(|m| match (parent_variant, m.parent_variant_id.as_deref()) {
                (Some(pv), Some(mv)) => pv == mv,
                _ => true,
            })
            .collect()
    }

    pub fn siblings(&self, m: &MsgRow) -> Vec<&MsgRow> {
        self.children(m.parent_id.as_deref(), m.parent_variant_id.as_deref())
    }

    /// Follows the newest child (under the effective variant) down to a leaf.
    pub fn descend(&self, from: &str) -> String {
        let mut cur = from.to_string();
        let mut seen = HashSet::new();
        while seen.insert(cur.clone()) {
            let pv = self.effective_variant(&cur).map(str::to_string);
            let next = self
                .children(Some(&cur), pv.as_deref())
                .last()
                .map(|m| m.id.clone());
            match next {
                Some(n) => cur = n,
                None => break,
            }
        }
        cur
    }

    /// Leaf used when the stored one is missing: descend from the newest root.
    pub fn default_leaf(&self) -> Option<String> {
        self.children(None, None)
            .last()
            .map(|m| self.descend(&m.id))
    }

    /// Root → `leaf`. Stops on broken links or cycles.
    pub fn path_to(&self, leaf: &str) -> Vec<&MsgRow> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut cur = self.get(leaf);
        while let Some(m) = cur {
            if !seen.insert(m.id.as_str()) {
                break;
            }
            out.push(m);
            cur = m.parent_id.as_deref().and_then(|p| self.get(p));
        }
        out.reverse();
        out
    }

    /// Variant shown for each message of a path: the variant the next message descends
    /// from, else the effective one.
    pub fn path_variants(&self, path: &[&MsgRow]) -> Vec<Option<String>> {
        path.iter()
            .enumerate()
            .map(|(i, m)| {
                let list = self.variant_ids(&m.id);
                let via_child = path
                    .get(i + 1)
                    .and_then(|next| next.parent_variant_id.as_deref())
                    .filter(|pv| list.iter().any(|v| v == pv));
                via_child
                    .or_else(|| self.effective_variant(&m.id))
                    .map(str::to_string)
            })
            .collect()
    }
}

fn path_message(
    conn: &Connection,
    tree: &Tree,
    m: &MsgRow,
    variant_id: &str,
) -> StateResult<PathMessage> {
    let variant = load_variant(conn, variant_id)?;
    let variant_ids = tree.variant_ids(&m.id).to_vec();
    let variant_index = variant_ids
        .iter()
        .position(|v| v == variant_id)
        .unwrap_or(0);
    let sibling_ids: Vec<String> = tree.siblings(m).iter().map(|s| s.id.clone()).collect();
    let sibling_index = sibling_ids.iter().position(|s| *s == m.id).unwrap_or(0);
    let (compare, compare_groups) = compare_group(conn, &variant, &variant_ids)?;
    Ok(PathMessage {
        id: m.id.clone(),
        parent_id: m.parent_id.clone(),
        parent_variant_id: m.parent_variant_id.clone(),
        role: m.role.clone(),
        created_at: m.created_at,
        variant,
        variant_ids,
        variant_index,
        sibling_ids,
        sibling_index,
        compare,
        compare_groups,
    })
}

/// The variants of `shown`'s comparison group, sorted by side (empty when `shown` is not part
/// of one), and one variant id per comparison group of the message (oldest group first).
fn compare_group(
    conn: &Connection,
    shown: &Variant,
    variant_ids: &[String],
) -> StateResult<(Vec<Variant>, Vec<String>)> {
    let Some(tag) = compare_of(shown.meta.as_ref()) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut out = Vec::new();
    // (group, representative variant id): the side-`a` variant of each group.
    let mut groups: Vec<(String, String)> = Vec::new();
    for id in variant_ids {
        let v = if *id == shown.id {
            shown.clone()
        } else {
            load_variant(conn, id)?
        };
        let Some(t) = compare_of(v.meta.as_ref()) else {
            continue;
        };
        match groups.iter_mut().find(|g| g.0 == t.group) {
            Some(g) => {
                if t.side == "a" {
                    g.1 = v.id.clone();
                }
            }
            None => groups.push((t.group.clone(), v.id.clone())),
        }
        if t.group == tag.group {
            out.push(v);
        }
    }
    out.sort_by_cached_key(|v| {
        compare_of(v.meta.as_ref())
            .map(|t| t.side)
            .unwrap_or_default()
    });
    Ok((out, groups.into_iter().map(|g| g.1).collect()))
}

/// `current_leaf_id` of a conversation whose path was rewound to before its first message: the
/// view is empty (the stored branches stay reachable) until the next message is appended.
pub(crate) const EMPTY_LEAF: &str = "";

pub(crate) fn build_view(
    conn: &Connection,
    conversation_id: &str,
) -> StateResult<ConversationView> {
    let mut conversation = load_conversation(conn, conversation_id)?;
    let tree = Tree::load(conn, conversation_id)?;
    let rewound = conversation.current_leaf_id.as_deref() == Some(EMPTY_LEAF);
    let leaf = if rewound {
        None
    } else {
        conversation
            .current_leaf_id
            .clone()
            .filter(|l| tree.get(l).is_some())
            .or_else(|| tree.default_leaf())
    };
    conversation.current_leaf_id = leaf.clone();
    let mut messages = Vec::new();
    if let Some(leaf) = leaf {
        let path = tree.path_to(&leaf);
        let variants = tree.path_variants(&path);
        for (m, v) in path.iter().zip(variants) {
            // A message without any variant cannot be shown; the path ends there.
            let Some(v) = v else { break };
            messages.push(path_message(conn, &tree, m, &v)?);
        }
    }
    Ok(ConversationView {
        conversation,
        messages,
    })
}

fn message_conversation(conn: &Connection, message_id: &str) -> StateResult<String> {
    conn.query_row(
        "SELECT conversation_id FROM messages WHERE id = ?1",
        [message_id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| {
        StateError::not_found(err("conversations.message_not_found").with("id", message_id))
    })
}

fn set_leaf(conn: &Connection, conversation_id: &str, leaf: &str) -> StateResult<()> {
    conn.execute(
        "UPDATE conversations SET current_leaf_id = ?2 WHERE id = ?1",
        params![conversation_id, leaf],
    )?;
    Ok(())
}

/// Makes every message on the path to `leaf` active in the variant the path goes through, so
/// anything that later reads the "shown" variant from the active one (forking at a message in
/// the middle of the history, a new child appended to it) sees the same variant as the screen.
/// Needed when the path was reached through a child (a sibling branch, a variant switch deeper
/// down) without the ancestors' active variants having followed.
fn sync_active_variants(conn: &Connection, conversation_id: &str, leaf: &str) -> StateResult<()> {
    let tree = Tree::load(conn, conversation_id)?;
    let path = tree.path_to(leaf);
    for (m, shown) in path.iter().zip(tree.path_variants(&path)) {
        let Some(shown) = shown else { break };
        if m.active_variant_id.as_deref() != Some(shown.as_str()) {
            conn.execute(
                "UPDATE messages SET active_variant_id = ?2 WHERE id = ?1",
                params![m.id, shown],
            )?;
        }
    }
    Ok(())
}

fn touch(conn: &Connection, conversation_id: &str) -> StateResult<()> {
    conn.execute(
        "UPDATE conversations SET updated_at = ?2 WHERE id = ?1",
        params![conversation_id, now_ms()],
    )?;
    Ok(())
}

/// Creation time for a new message: never earlier than (and never equal to) the newest
/// message of the conversation, so sibling order is deterministic.
fn next_created_at(conn: &Connection, conversation_id: &str) -> StateResult<i64> {
    let last: i64 = conn.query_row(
        "SELECT COALESCE(MAX(created_at), 0) FROM messages WHERE conversation_id = ?1",
        [conversation_id],
        |r| r.get(0),
    )?;
    Ok(now_ms().max(last + 1))
}

/// Creation time for a new variant of `message_id`: later than every existing variant of it.
/// The first variant takes the message's (possibly clock-ahead) timestamp, so the plain clock
/// could sort a regenerated variant before it.
fn next_variant_created_at(conn: &Connection, message_id: &str) -> StateResult<i64> {
    let last: i64 = conn.query_row(
        "SELECT COALESCE(MAX(created_at), 0) FROM message_variants WHERE message_id = ?1",
        [message_id],
        |r| r.get(0),
    )?;
    Ok(now_ms().max(last + 1))
}

fn resolve_id(id: Option<String>) -> StateResult<String> {
    match id {
        Some(id) => {
            check_client_id(&id)?;
            Ok(id)
        }
        None => Ok(new_id()),
    }
}

fn insert_conversation(conn: &Connection, id: &str, n: &NewConversation) -> StateResult<()> {
    let title = clean_title(n.title.as_deref().unwrap_or(""));
    let source = n.title_source.clone().unwrap_or_else(|| {
        if title.is_empty() {
            "none".into()
        } else {
            "user".into()
        }
    });
    check_title_source(&source)?;
    if let Some(f) = &n.folder_id {
        ensure_folder(conn, f)?;
    }
    let now = now_ms();
    conn.execute(
        "INSERT INTO conversations (id, title, title_source, folder_id, created_at, updated_at, \
         server_id, model_alias, preset_id, system_prompt, settings_json, metadata_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id,
            title,
            source,
            n.folder_id,
            now,
            empty_to_none(n.server_id.clone()),
            empty_to_none(n.model_alias.clone()),
            empty_to_none(n.preset_id.clone()),
            n.system_prompt.clone().unwrap_or_default(),
            object_or_empty(n.settings.clone()).to_string(),
            object_or_empty(n.metadata.clone()).to_string(),
        ],
    )?;
    Ok(())
}

fn ensure_folder(conn: &Connection, id: &str) -> StateResult<()> {
    let exists: bool = conn
        .query_row("SELECT 1 FROM folders WHERE id = ?1", [id], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    if exists {
        Ok(())
    } else {
        Err(StateError::not_found(
            err("conversations.folder_not_found").with("id", id),
        ))
    }
}

/// Inserts a message plus its first variant and makes it the current leaf.
fn append_inner(conn: &Connection, input: AppendMessage) -> StateResult<(String, String)> {
    check_role(&input.role)?;
    let conv = &input.conversation_id;
    load_conversation(conn, conv)?;
    let parent_variant_id = match &input.parent_id {
        Some(p) => {
            if message_conversation(conn, p)? != *conv {
                return Err(StateError::invalid(
                    err("conversations.parent_elsewhere").with("id", p),
                ));
            }
            if input.any_parent_variant {
                None
            } else {
                let tree = Tree::load(conn, conv)?;
                tree.effective_variant(p).map(str::to_string)
            }
        }
        None => None,
    };
    let id = resolve_id(input.id)?;
    let vid = resolve_id(input.variant_id)?;
    let now = next_created_at(conn, conv)?;
    conn.execute(
        "INSERT INTO messages (id, conversation_id, parent_id, parent_variant_id, role, created_at, \
         active_variant_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, conv, input.parent_id, parent_variant_id, input.role, now, vid],
    )?;
    insert_variant(conn, &vid, &id, &input.variant, now)?;
    set_leaf(conn, conv, &id)?;
    touch(conn, conv)?;
    Ok((id, vid))
}

fn path_message_by_id(
    conn: &Connection,
    conversation_id: &str,
    message_id: &str,
    variant_id: &str,
) -> StateResult<PathMessage> {
    let tree = Tree::load(conn, conversation_id)?;
    let m = tree.get(message_id).ok_or_else(|| {
        StateError::not_found(err("conversations.message_not_found").with("id", message_id))
    })?;
    path_message(conn, &tree, m, variant_id)
}

// ---------------------------------------------------------------------------
// Service API
// ---------------------------------------------------------------------------

impl StateDb {
    // ---- Folders ----

    pub fn list_folders(&self) -> StateResult<Vec<Folder>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT id, name, parent_id, sort, created_at FROM folders ORDER BY sort, name",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(Folder {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        parent_id: r.get(2)?,
                        sort: r.get(3)?,
                        created_at: r.get(4)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn create_folder(&self, name: &str, parent_id: Option<&str>) -> StateResult<Folder> {
        let name = clean_title(name);
        if name.is_empty() {
            return Err(StateError::invalid(err("conversations.folder_name_empty")));
        }
        self.write(|tx| {
            if let Some(p) = parent_id {
                ensure_folder(tx, p)?;
            }
            let sort: i64 = tx.query_row("SELECT COALESCE(MAX(sort), -1) + 1 FROM folders", [], |r| {
                r.get(0)
            })?;
            let folder = Folder {
                id: new_id(),
                name,
                parent_id: parent_id.map(str::to_string),
                sort,
                created_at: now_ms(),
            };
            tx.execute(
                "INSERT INTO folders (id, name, parent_id, sort, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![folder.id, folder.name, folder.parent_id, folder.sort, folder.created_at],
            )?;
            Ok(folder)
        })
    }

    pub fn rename_folder(&self, id: &str, name: &str) -> StateResult<()> {
        let name = clean_title(name);
        if name.is_empty() {
            return Err(StateError::invalid(err("conversations.folder_name_empty")));
        }
        self.write(|tx| {
            let n = tx.execute(
                "UPDATE folders SET name = ?2 WHERE id = ?1",
                params![id, name],
            )?;
            if n == 0 {
                return Err(StateError::not_found(
                    err("conversations.folder_not_found").with("id", id),
                ));
            }
            Ok(())
        })
    }

    /// Deletes a folder; its conversations and sub-folders move to its parent.
    pub fn delete_folder(&self, id: &str) -> StateResult<()> {
        self.write(|tx| {
            let parent: Option<String> = tx
                .query_row("SELECT parent_id FROM folders WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?
                .ok_or_else(|| {
                    StateError::not_found(err("conversations.folder_not_found").with("id", id))
                })?;
            tx.execute(
                "UPDATE conversations SET folder_id = ?2 WHERE folder_id = ?1",
                params![id, parent],
            )?;
            tx.execute(
                "UPDATE folders SET parent_id = ?2 WHERE parent_id = ?1",
                params![id, parent],
            )?;
            tx.execute("DELETE FROM folders WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    // ---- Conversations ----

    pub fn create_conversation(&self, n: &NewConversation) -> StateResult<Conversation> {
        self.write(|tx| {
            let id = new_id();
            insert_conversation(tx, &id, n)?;
            load_conversation(tx, &id)
        })
    }

    /// Lists conversations: pinned first, then most recently updated. With a `query`,
    /// results come from the full-text index in relevance order.
    pub fn list_conversations(&self, q: &ListQuery) -> StateResult<Vec<ConversationSummary>> {
        let limit = q.limit.unwrap_or(200).clamp(1, 1000) as usize;
        let offset = q.offset.unwrap_or(0) as usize;
        let fts = q.query.as_deref().and_then(fts_query);
        self.read(|c| {
            let Some(fts) = fts else {
                let mut stmt = c.prepare(&format!(
                    "SELECT {CONV_COLS} FROM conversations \
                     WHERE (?1 IS NULL OR folder_id = ?1) AND (?2 = 0 OR folder_id IS NULL) \
                       AND (?3 = 0 OR pinned = 1) AND (?4 = 0 OR favorite = 1) \
                       AND id NOT IN (SELECT id FROM rag_kbs) \
                     ORDER BY pinned DESC, updated_at DESC, rid DESC LIMIT ?5 OFFSET ?6"
                ))?;
                let rows = stmt
                    .query_map(
                        params![
                            q.folder_id,
                            q.unfiled as i64,
                            q.pinned_only as i64,
                            q.favorites_only as i64,
                            limit as i64,
                            offset as i64
                        ],
                        conversation_from_row,
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                return Ok(rows.into_iter().map(summary_of).collect());
            };
            let mut stmt = c.prepare(
                "SELECT conversation_id, snippet(search_index, 1, '', '', '…', 12) \
                 FROM search_index WHERE search_index MATCH ?1 ORDER BY rank LIMIT 5000",
            )?;
            let hits = stmt
                .query_map([&fts], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut seen = HashSet::new();
            let mut out = Vec::new();
            for (id, snippet) in hits {
                if !seen.insert(id.clone()) {
                    continue;
                }
                let Ok(conv) = load_conversation(c, &id) else {
                    continue;
                };
                if q.folder_id.is_some() && conv.folder_id != q.folder_id
                    || q.unfiled && conv.folder_id.is_some()
                    || q.pinned_only && !conv.pinned
                    || q.favorites_only && !conv.favorite
                {
                    continue;
                }
                let mut s = summary_of(conv);
                s.snippet = Some(snippet);
                out.push(s);
            }
            Ok(out.into_iter().skip(offset).take(limit).collect())
        })
    }

    pub fn get_conversation(&self, id: &str) -> StateResult<ConversationView> {
        self.read(|c| build_view(c, id))
    }

    /// Like [`get_conversation`](Self::get_conversation) and records it as last opened.
    /// In read-only mode the timestamp is simply not written.
    pub fn open_conversation(&self, id: &str) -> StateResult<ConversationView> {
        if !self.is_read_only() {
            self.write(|tx| {
                let n = tx.execute(
                    "UPDATE conversations SET last_opened_at = ?2 WHERE id = ?1",
                    params![id, now_ms()],
                )?;
                if n == 0 {
                    return Err(StateError::not_found(
                        err("conversations.not_found").with("id", id),
                    ));
                }
                Ok(())
            })?;
        }
        self.get_conversation(id)
    }

    pub fn last_opened_conversation(&self) -> StateResult<Option<String>> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT id FROM conversations WHERE last_opened_at IS NOT NULL \
                 ORDER BY last_opened_at DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    /// Sets the title. With `only_if_source`, the title is changed only while the current
    /// source matches (so an auto title never replaces one the user typed). Returns
    /// whether the title changed.
    pub fn set_title(
        &self,
        id: &str,
        title: &str,
        source: &str,
        only_if_source: Option<&str>,
    ) -> StateResult<bool> {
        check_title_source(source)?;
        let title = clean_title(title);
        let source = if title.is_empty() { "none" } else { source };
        self.write(|tx| {
            let conv = load_conversation(tx, id)?;
            if let Some(expected) = only_if_source {
                if conv.title_source != expected {
                    return Ok(false);
                }
            }
            tx.execute(
                "UPDATE conversations SET title = ?2, title_source = ?3 WHERE id = ?1",
                params![id, title, source],
            )?;
            Ok(true)
        })
    }

    pub fn update_conversation(
        &self,
        id: &str,
        p: &ConversationPatch,
    ) -> StateResult<Conversation> {
        self.write(|tx| {
            load_conversation(tx, id)?;
            if let Some(sp) = &p.system_prompt {
                tx.execute(
                    "UPDATE conversations SET system_prompt = ?2 WHERE id = ?1",
                    params![id, sp],
                )?;
            }
            if let Some(s) = &p.settings {
                tx.execute(
                    "UPDATE conversations SET settings_json = ?2 WHERE id = ?1",
                    params![id, object_or_empty(Some(s.clone())).to_string()],
                )?;
            }
            if let Some(m) = &p.metadata {
                tx.execute(
                    "UPDATE conversations SET metadata_json = ?2 WHERE id = ?1",
                    params![id, object_or_empty(Some(m.clone())).to_string()],
                )?;
            }
            for (col, val) in [
                ("server_id", &p.server_id),
                ("model_alias", &p.model_alias),
                ("preset_id", &p.preset_id),
            ] {
                if val.is_some() {
                    tx.execute(
                        &format!("UPDATE conversations SET {col} = ?2 WHERE id = ?1"),
                        params![id, empty_to_none(val.clone())],
                    )?;
                }
            }
            load_conversation(tx, id)
        })
    }

    pub fn set_pinned(&self, id: &str, pinned: bool) -> StateResult<()> {
        self.set_flag_column(id, "pinned", pinned)
    }

    pub fn set_favorite(&self, id: &str, favorite: bool) -> StateResult<()> {
        self.set_flag_column(id, "favorite", favorite)
    }

    fn set_flag_column(&self, id: &str, col: &str, on: bool) -> StateResult<()> {
        self.write(|tx| {
            let n = tx.execute(
                &format!("UPDATE conversations SET {col} = ?2 WHERE id = ?1"),
                params![id, on as i64],
            )?;
            if n == 0 {
                return Err(StateError::not_found(
                    err("conversations.not_found").with("id", id),
                ));
            }
            Ok(())
        })
    }

    pub fn move_to_folder(&self, id: &str, folder_id: Option<&str>) -> StateResult<()> {
        self.write(|tx| {
            if let Some(f) = folder_id {
                ensure_folder(tx, f)?;
            }
            let n = tx.execute(
                "UPDATE conversations SET folder_id = ?2 WHERE id = ?1",
                params![id, folder_id],
            )?;
            if n == 0 {
                return Err(StateError::not_found(
                    err("conversations.not_found").with("id", id),
                ));
            }
            Ok(())
        })
    }

    pub fn delete_conversation(&self, id: &str) -> StateResult<()> {
        self.write(|tx| {
            let n = tx.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
            if n == 0 {
                return Err(StateError::not_found(
                    err("conversations.not_found").with("id", id),
                ));
            }
            Ok(())
        })
    }

    /// Full copy (all branches and variants) under new ids.
    pub fn duplicate_conversation(
        &self,
        id: &str,
        title: Option<&str>,
    ) -> StateResult<Conversation> {
        self.write(|tx| {
            let export = super::export::read_export(tx, id, None, true)?;
            let new_id = super::export::insert_export(
                tx,
                &export,
                &super::export::InsertOptions {
                    title: title.map(str::to_string),
                    keep_folder: true,
                    fresh_timestamps: true,
                    forked_from: None,
                },
            )?;
            load_conversation(tx, &new_id)
        })
    }

    /// New conversation holding a copy of the current path up to `message_id`.
    pub fn fork_conversation(
        &self,
        message_id: &str,
        title: Option<&str>,
    ) -> StateResult<Conversation> {
        self.write(|tx| {
            let conv = message_conversation(tx, message_id)?;
            let export = super::export::read_export(tx, &conv, Some(message_id), true)?;
            let new_id = super::export::insert_export(
                tx,
                &export,
                &super::export::InsertOptions {
                    title: title.map(str::to_string),
                    keep_folder: true,
                    fresh_timestamps: true,
                    forked_from: Some((conv.clone(), message_id.to_string())),
                },
            )?;
            load_conversation(tx, &new_id)
        })
    }

    // ---- Messages ----

    /// Appends a message (with its first variant) and makes it the current leaf. The
    /// child is attached to the parent's active variant.
    pub fn append_message(&self, input: AppendMessage) -> StateResult<PathMessage> {
        let conv = input.conversation_id.clone();
        self.write(|tx| {
            let (id, vid) = append_inner(tx, input)?;
            path_message_by_id(tx, &conv, &id, &vid)
        })
    }

    /// Adds a variant (e.g. a regenerated answer), activates it and makes the message the
    /// current leaf (continuations of other variants stay stored).
    pub fn add_variant(
        &self,
        message_id: &str,
        input: &VariantInput,
        variant_id: Option<String>,
    ) -> StateResult<PathMessage> {
        self.write(|tx| {
            let conv = message_conversation(tx, message_id)?;
            let vid = resolve_id(variant_id)?;
            let created_at = next_variant_created_at(tx, message_id)?;
            insert_variant(tx, &vid, message_id, input, created_at)?;
            tx.execute(
                "UPDATE messages SET active_variant_id = ?2 WHERE id = ?1",
                params![message_id, vid],
            )?;
            set_leaf(tx, &conv, message_id)?;
            touch(tx, &conv)?;
            path_message_by_id(tx, &conv, message_id, &vid)
        })
    }

    /// Replaces a variant's content (streaming finished, tool results arrived…). Metadata is
    /// replaced only when `v.meta` is set (a content update keeps the split-view side).
    pub fn update_variant(&self, variant_id: &str, v: &VariantInput) -> StateResult<()> {
        self.write(|tx| {
            let n = tx.execute(
                "UPDATE message_variants SET content = ?2, reasoning = ?3, tool_calls_json = ?4, \
                 stats_json = ?5, finish_reason = ?6, stopped = ?7, tool_limit_reached = ?8, \
                 error = ?9, model_alias = ?10, meta_json = COALESCE(?11, meta_json) WHERE id = ?1",
                params![
                    variant_id,
                    v.content,
                    v.reasoning,
                    json_text(&v.tool_calls),
                    json_text(&v.stats),
                    v.finish_reason,
                    v.stopped as i64,
                    v.tool_limit_reached as i64,
                    v.error,
                    v.model_alias,
                    meta_text(&v.meta),
                ],
            )?;
            if n == 0 {
                return Err(StateError::not_found(err("conversations.variant_not_found").with("id", variant_id)));
            }
            let conv: String = tx.query_row(
                "SELECT m.conversation_id FROM message_variants v JOIN messages m ON m.id = v.message_id \
                 WHERE v.id = ?1",
                [variant_id],
                |r| r.get(0),
            )?;
            touch(tx, &conv)
        })
    }

    /// Shows another variant of a message; the path continues with the newest
    /// continuation of that variant.
    pub fn select_variant(
        &self,
        message_id: &str,
        variant_id: &str,
    ) -> StateResult<ConversationView> {
        self.write(|tx| {
            let conv = message_conversation(tx, message_id)?;
            let owner: Option<String> = tx
                .query_row(
                    "SELECT message_id FROM message_variants WHERE id = ?1",
                    [variant_id],
                    |r| r.get(0),
                )
                .optional()?;
            if owner.as_deref() != Some(message_id) {
                return Err(StateError::not_found(
                    err("conversations.variant_not_found").with("id", variant_id),
                ));
            }
            tx.execute(
                "UPDATE messages SET active_variant_id = ?2 WHERE id = ?1",
                params![message_id, variant_id],
            )?;
            let leaf = Tree::load(tx, &conv)?.descend(message_id);
            set_leaf(tx, &conv, &leaf)?;
            sync_active_variants(tx, &conv, &leaf)?;
            build_view(tx, &conv)
        })
    }

    /// Switches to the branch through `message_id` (e.g. a sibling of an edited message).
    pub fn select_branch(&self, message_id: &str) -> StateResult<ConversationView> {
        self.write(|tx| {
            let conv = message_conversation(tx, message_id)?;
            let leaf = Tree::load(tx, &conv)?.descend(message_id);
            set_leaf(tx, &conv, &leaf)?;
            sync_active_variants(tx, &conv, &leaf)?;
            build_view(tx, &conv)
        })
    }

    /// Makes `message_id` the exact current leaf (the path is cut there).
    pub fn set_current_leaf(&self, message_id: &str) -> StateResult<ConversationView> {
        self.write(|tx| {
            let conv = message_conversation(tx, message_id)?;
            set_leaf(tx, &conv, message_id)?;
            sync_active_variants(tx, &conv, message_id)?;
            build_view(tx, &conv)
        })
    }

    /// Edit-and-resend: stores `content` as a new sibling of a user message and makes it
    /// the current leaf. The original message and its answers stay as another branch.
    pub fn edit_user_message(
        &self,
        message_id: &str,
        content: &str,
        new_message_id: Option<String>,
    ) -> StateResult<PathMessage> {
        self.write(|tx| {
            let conv = message_conversation(tx, message_id)?;
            let (role, parent_id, parent_variant_id): (String, Option<String>, Option<String>) = tx
                .query_row(
                    "SELECT role, parent_id, parent_variant_id FROM messages WHERE id = ?1",
                    [message_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
            if role != "user" {
                return Err(StateError::invalid(err("conversations.only_user_editable")));
            }
            // Multimodal attachments stay with edited text; transient request metadata does not.
            let attachments = tx
                .query_row(
                    "SELECT v.meta_json FROM messages m JOIN message_variants v \
                     ON v.id = m.active_variant_id WHERE m.id = ?1",
                    [message_id],
                    |r| r.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .and_then(|m| {
                    let mut attachments = serde_json::Map::new();
                    for key in ["images", "audio"] {
                        if let Some(value) = m
                            .get(key)
                            .filter(|v| v.as_array().is_some_and(|a| !a.is_empty()))
                        {
                            attachments.insert(key.to_string(), value.clone());
                        }
                    }
                    (!attachments.is_empty()).then_some(Value::Object(attachments))
                });
            let id = resolve_id(new_message_id)?;
            let vid = new_id();
            let now = next_created_at(tx, &conv)?;
            tx.execute(
                "INSERT INTO messages (id, conversation_id, parent_id, parent_variant_id, role, \
                 created_at, active_variant_id) VALUES (?1, ?2, ?3, ?4, 'user', ?5, ?6)",
                params![id, conv, parent_id, parent_variant_id, now, vid],
            )?;
            insert_variant(
                tx,
                &vid,
                &id,
                &VariantInput {
                    content: content.to_string(),
                    meta: attachments,
                    ..Default::default()
                },
                now,
            )?;
            set_leaf(tx, &conv, &id)?;
            touch(tx, &conv)?;
            path_message_by_id(tx, &conv, &id, &vid)
        })
    }

    /// Rewinds to just before a user message ("edit from here"): the path now ends at the
    /// message's parent (an empty path for a first message), so the message and everything
    /// after it leave the active path. Nothing is deleted: the old continuation stays in the
    /// tree and shows up as a branch next to whatever the user sends from here (a new message
    /// with the same parent is its sibling).
    pub fn rewind_before(&self, message_id: &str) -> StateResult<ConversationView> {
        self.write(|tx| {
            let conv = message_conversation(tx, message_id)?;
            let (role, parent_id, parent_variant_id): (String, Option<String>, Option<String>) = tx
                .query_row(
                    "SELECT role, parent_id, parent_variant_id FROM messages WHERE id = ?1",
                    [message_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
            if role != "user" {
                return Err(StateError::invalid(err("conversations.only_user_editable")));
            }
            match parent_id {
                Some(parent) => {
                    // The path reached the message through this variant of its parent.
                    if let Some(pv) = parent_variant_id {
                        tx.execute(
                            "UPDATE messages SET active_variant_id = ?2 WHERE id = ?1",
                            params![parent, pv],
                        )?;
                    }
                    set_leaf(tx, &conv, &parent)?;
                }
                None => set_leaf(tx, &conv, EMPTY_LEAF)?,
            }
            touch(tx, &conv)?;
            build_view(tx, &conv)
        })
    }

    /// Split view, "continue with this side": every message of the current path that shows a
    /// comparison variant switches to the variant of `side` from the same group (messages
    /// without that side keep theirs). The leaf stays, so the history reads as that side's
    /// conversation and the next message continues it.
    pub fn adopt_compare_side(
        &self,
        conversation_id: &str,
        side: &str,
    ) -> StateResult<ConversationView> {
        self.write(|tx| {
            let view = build_view(tx, conversation_id)?;
            for m in &view.messages {
                let target = m
                    .compare
                    .iter()
                    .find(|v| compare_of(v.meta.as_ref()).is_some_and(|t| t.side == side));
                if let Some(target) = target {
                    tx.execute(
                        "UPDATE messages SET active_variant_id = ?2 WHERE id = ?1",
                        params![m.id, target.id],
                    )?;
                }
            }
            if let Some(leaf) = &view.conversation.current_leaf_id {
                set_leaf(tx, conversation_id, leaf)?;
            }
            touch(tx, conversation_id)?;
            build_view(tx, conversation_id)
        })
    }

    /// Stores a linear chat (the old single-conversation Playground) once. Returns the new
    /// conversation id, or `None` if this import already ran or there was nothing to store.
    pub fn import_legacy_playground(&self, import: &LinearImport) -> StateResult<Option<String>> {
        self.write(|tx| {
            if get_flag(tx, LEGACY_IMPORT_FLAG)?.is_some() {
                return Ok(None);
            }
            let created = import_legacy_inner(tx, import)?;
            set_flag(tx, LEGACY_IMPORT_FLAG, &now_ms().to_string())?;
            Ok(created)
        })
    }

    /// Atomically imports the old Playground conversation and persistent defaults. Existing
    /// database values win; a conflicting browser value is deliberately not verified so the
    /// caller keeps its local copy. Safe to retry after any interruption.
    pub fn import_playground_state(
        &self,
        import: &PlaygroundStateImport,
    ) -> StateResult<PlaygroundStateImportResult> {
        self.write(|tx| {
            let already_imported = get_flag(tx, LEGACY_IMPORT_FLAG)?.is_some();
            let conversation_verified = if already_imported {
                true
            } else if let Some(conversation) = &import.conversation {
                import_legacy_inner(tx, conversation)?;
                set_flag(tx, LEGACY_IMPORT_FLAG, &now_ms().to_string())?;
                true
            } else {
                false
            };

            let mut verified_keys = Vec::new();
            for item in &import.values {
                tx.execute(
                    "INSERT OR IGNORE INTO app_state(key, value) VALUES (?1, ?2)",
                    params![item.key, item.value],
                )?;
                if get_flag(tx, &item.key)?.as_deref() == Some(item.value.as_str()) {
                    verified_keys.push(item.key.clone());
                }
            }
            Ok(PlaygroundStateImportResult {
                conversation_verified,
                verified_keys,
            })
        })
    }
}

fn import_legacy_inner(
    tx: &rusqlite::Transaction<'_>,
    import: &LinearImport,
) -> StateResult<Option<String>> {
    if import.messages.is_empty() {
        return Ok(None);
    }
    let id = new_id();
    insert_conversation(tx, &id, &import.conversation)?;
    let mut parent: Option<String> = None;
    for m in &import.messages {
        let (mid, _) = append_inner(
            tx,
            AppendMessage {
                conversation_id: id.clone(),
                parent_id: parent.clone(),
                role: m.role.clone(),
                variant: m.variant.clone(),
                id: None,
                variant_id: None,
                any_parent_variant: false,
            },
        )?;
        parent = Some(mid);
    }
    Ok(Some(id))
}

/// Turns free text into a safe FTS5 query: every word becomes a quoted prefix term
/// (all must match). Returns `None` when nothing searchable remains.
pub(crate) fn fts_query(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split_whitespace()
        .map(|w| w.replace('"', ""))
        .filter(|w| !w.is_empty())
        .take(16)
        .map(|w| format!("\"{w}\"*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}
