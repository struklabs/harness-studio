//! User state and conversations in SQLite (`<data root>/state.db`).
//!
//! Configuration stays in JSON files (see `config`, `store`); everything that is
//! relational, grows without bound or needs search lives here (private hs-internal architecture notes).
//!
//! # Opening and migrations
//! [`StateDb::open`] enables WAL, foreign keys and a busy timeout, then applies the
//! ordered migrations in [`schema::MIGRATIONS`] inside one `IMMEDIATE` transaction,
//! bumping `PRAGMA user_version` to the number of applied migrations. Re-opening an
//! up-to-date database is a no-op. A database whose `user_version` is *newer* than
//! this build knows is opened read-only: reads are attempted, every write returns
//! [`StateError::ReadOnly`] and nothing is downgraded or rewritten.
//!
//! # Schema (v1–v6)
//! - `folders(id, name, parent_id → folders, sort, created_at)`. Deleting a folder
//!   moves its conversations and sub-folders to the parent (see [`StateDb::delete_folder`]).
//! - `conversations(rid, id uuid UNIQUE, title, title_source, folder_id → folders,
//!   pinned, favorite, created_at, updated_at, last_opened_at, server_id, model_alias,
//!   preset_id, system_prompt, settings_json, metadata_json, current_leaf_id,
//!   forked_from_conversation_id, forked_from_message_id)`.
//!   `rid` is an `INTEGER PRIMARY KEY` so the search index can address rows by a rowid
//!   that survives `VACUUM`. `title_source` is `none | fallback | auto | user`, so an
//!   auto-generated title never overwrites a title the user typed.
//!   `settings_json` is the sampling snapshot of the Playground (opaque to the core).
//! - `messages(id, conversation_id → conversations, parent_id → messages,
//!   parent_variant_id, role, created_at, active_variant_id)`: messages form a **tree**.
//!   A child hangs under a specific variant of its parent (`parent_variant_id`), so a
//!   regenerated answer keeps its own continuation. The conversation shows the path
//!   from the root to `current_leaf_id`.
//! - `message_variants(rid, id UNIQUE, message_id → messages, content, reasoning,
//!   tool_calls_json, stats_json, finish_reason, stopped, tool_limit_reached, error,
//!   model_alias, created_at)`: alternative generations of one message.
//!   Tool calls live in `tool_calls_json` (id, name, arguments, status, result…): they are
//!   always read and written together with their variant, replayed as context in order
//!   and never queried on their own, so a separate table would only add joins.
//! - `attachments(id, message_id → messages, kind, name, mime, size, sha256, path,
//!   created_at)`: metadata only; the file itself is not stored in the database.
//! - `app_state(key, value)`: small key/value flags (e.g. legacy import done).
//! - `model_state(model_id, favorite, last_used_at, last_definition_id, last_preset_id,
//!   added_at)` (v2): library state of a local model file, keyed by `LocalModel.id`
//!   (see [`StateDb::model_states`]).
//! - `vram_measurements(model_key, settings_key, model_size, measured_at, data_json)` (v3):
//!   memory footprint measured after successful loads (see [`crate::vram`]); at most
//!   [`vram::MAX_PER_MODEL`] rows per model file.
//! - `message_variants.meta_json` (v4): client metadata of a variant. The Playground split
//!   view stores both answers of a split turn as variants of one message, each tagged
//!   `meta.compare = {group, side, ...}` (see [`compare_of`], [`PathMessage::compare`],
//!   [`StateDb::adopt_compare_side`]).
//! - `rag_documents`, `rag_chunks`, `rag_fts` (v5): documents attached to a conversation for
//!   local RAG, their chunks with f32 embeddings (BLOB) and a keyword index (see `crate::rag`,
//!   [`StateDb::rag_documents`]). Deleting a conversation deletes its documents.
//! - `rag_kbs`, `rag_conversation_kbs` (v8): named knowledge bases (their documents are
//!   `rag_documents` of a hidden conversation row with the same id) and their attachment to
//!   conversations (see [`StateDb::kb_list`]).
//! - `gateway_responses(rid, id UNIQUE, scope, previous_id, model, created_at, last_used_at,
//!   input_json, response_json, bytes)` (v6): stored `/v1/responses` of the gateway
//!   (`store`, `previous_response_id`; see `crate::gateway::responses`). Rows are scoped to
//!   the caller and pruned by age since last use, count and bytes ([`StateDb::put_response`]).
//! - `search_index`: FTS5 (`unicode61`, diacritics removed) over conversation titles
//!   (rowid = `-conversations.rid`) and variant contents (rowid = `message_variants.rid`),
//!   kept in sync by triggers (cascaded deletes fire them too).
//!
//! # Branching
//! - **Edit a user message** → a new sibling user message under the same parent
//!   ([`StateDb::edit_user_message`]); the old branch stays reachable.
//! - **Regenerate** → a new variant of the assistant message ([`StateDb::add_variant`]).
//! - **Switch** variant or branch → [`StateDb::select_variant`] / [`StateDb::select_branch`]
//!   move `current_leaf_id` to the most recent leaf below that choice.
//! - **Fork** → a new conversation with a copy of the path up to a message
//!   ([`StateDb::fork_conversation`]); `forked_from_*` records the origin.
//!
//! All timestamps are Unix milliseconds.

mod conversations;
mod export;
mod models;
mod rag;
mod rag_kb;
mod responses;
mod schema;
#[cfg(test)]
mod tests;
pub mod vram;

pub use conversations::*;
pub use export::*;
pub use models::*;
pub use rag::*;
pub use rag_kb::*;
pub use responses::*;

use crate::error_key::{err, ErrorKey};
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Schema version this build writes (`PRAGMA user_version`).
pub const SCHEMA_VERSION: i64 = schema::MIGRATIONS.len() as i64;

#[derive(Debug)]
pub enum StateError {
    /// The database was created by a newer version; it is open read-only.
    ReadOnly {
        found: i64,
    },
    /// A keyed error (`conversations.not_found {...}`); free text is shown as `state.not_found`.
    NotFound(String),
    /// A keyed error (`export.cycle`); free text is shown as `state.invalid`.
    Invalid(String),
    Db(rusqlite::Error),
    Io(std::io::Error),
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::ReadOnly { found } => write!(
                f,
                "{}",
                err("state.read_only")
                    .num("found", *found)
                    .num("supported", SCHEMA_VERSION)
            ),
            StateError::NotFound(what) if crate::error_key::parse(what).is_some() => {
                f.write_str(what)
            }
            StateError::NotFound(what) => {
                write!(f, "{}", err("state.not_found").with("what", what))
            }
            StateError::Invalid(msg) if crate::error_key::parse(msg).is_some() => f.write_str(msg),
            StateError::Invalid(msg) => write!(f, "{}", err("state.invalid").with("detail", msg)),
            StateError::Db(e) => write!(f, "{}", err("db.error").with("error", e)),
            StateError::Io(e) => write!(f, "{}", err("fs.io_failed").with("error", e)),
        }
    }
}

impl std::error::Error for StateError {}

impl StateError {
    pub(crate) fn not_found(key: ErrorKey) -> Self {
        StateError::NotFound(key.into())
    }

    pub(crate) fn invalid(key: ErrorKey) -> Self {
        StateError::Invalid(key.into())
    }
}

impl From<rusqlite::Error> for StateError {
    fn from(e: rusqlite::Error) -> Self {
        StateError::Db(e)
    }
}

impl From<std::io::Error> for StateError {
    fn from(e: std::io::Error) -> Self {
        StateError::Io(e)
    }
}

impl From<StateError> for String {
    fn from(e: StateError) -> Self {
        e.to_string()
    }
}

pub type StateResult<T> = Result<T, StateError>;

/// Handle to `state.db`. One connection behind a mutex: SQLite has a single writer
/// anyway and every operation here is short.
pub struct StateDb {
    conn: Mutex<Connection>,
    /// `Some(version)` when opened read-only because the schema is newer than ours.
    newer_version: Option<i64>,
    path: PathBuf,
}

/// The process-wide handle to `<data root>/state.db`, opened (and migrated) on first use.
/// Call after `paths::init`. Front ends and core hooks (e.g. recording the last use of a
/// model when a server starts) share this one connection.
pub fn shared() -> Result<std::sync::Arc<StateDb>, String> {
    static SHARED: std::sync::OnceLock<Result<std::sync::Arc<StateDb>, String>> =
        std::sync::OnceLock::new();
    SHARED
        .get_or_init(|| {
            StateDb::open_default()
                .map(std::sync::Arc::new)
                .map_err(|e| e.to_string())
        })
        .clone()
}

impl StateDb {
    /// Opens `<data root>/state.db`.
    pub fn open_default() -> StateResult<Self> {
        Self::open(&crate::paths::state_db_file())
    }

    /// Opens (creating if needed) the database at `path` and applies pending migrations.
    pub fn open(path: &Path) -> StateResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            drop(conn);
            let ro = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            ro.busy_timeout(Duration::from_secs(5))?;
            return Ok(Self {
                conn: Mutex::new(ro),
                newer_version: Some(version),
                path: path.to_path_buf(),
            });
        }
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            newer_version: None,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// True when the database belongs to a newer app version and is read-only.
    pub fn is_read_only(&self) -> bool {
        self.newer_version.is_some()
    }

    /// Schema version stored in the file.
    pub fn user_version(&self) -> StateResult<i64> {
        self.read(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn read<T>(&self, f: impl FnOnce(&Connection) -> StateResult<T>) -> StateResult<T> {
        let conn = self.lock();
        f(&conn)
    }

    /// Runs `f` in an `IMMEDIATE` transaction; rolled back if `f` fails.
    pub(crate) fn write<T>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> StateResult<T>,
    ) -> StateResult<T> {
        if let Some(found) = self.newer_version {
            return Err(StateError::ReadOnly { found });
        }
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    /// Reads a small flag from `app_state`.
    pub fn get_flag(&self, key: &str) -> StateResult<Option<String>> {
        self.read(|c| get_flag(c, key))
    }

    pub fn set_flag(&self, key: &str, value: &str) -> StateResult<()> {
        self.write(|tx| set_flag(tx, key, value))
    }
}

pub(crate) fn get_flag(conn: &Connection, key: &str) -> StateResult<Option<String>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row("SELECT value FROM app_state WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub(crate) fn set_flag(conn: &Connection, key: &str, value: &str) -> StateResult<()> {
    conn.execute(
        "INSERT INTO app_state(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

/// Applies every migration after the stored `user_version`, all in one transaction.
fn migrate(conn: &mut Connection) -> StateResult<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Re-read inside the transaction: another process may have migrated meanwhile.
    let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }
    for sql in &schema::MIGRATIONS[version as usize..] {
        tx.execute_batch(sql)?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Client-supplied ids (the Playground keys messages before they are saved) must be
/// short, printable tokens.
pub(crate) fn check_client_id(id: &str) -> StateResult<()> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(StateError::invalid(err("state.invalid_id").with("id", id)))
    }
}

/// SQL of the first schema version (tests upgrade a v1 file to the current version).
#[cfg(test)]
pub(crate) fn schema_v1_sql() -> &'static str {
    schema::MIGRATIONS[0]
}
