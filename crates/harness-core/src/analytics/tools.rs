//! The tool log in `analytics.db` (`tool_calls`): who called which tool with which arguments,
//! how long it took, how it ended. Written by `tools::ToolService`, one row per call (including
//! calls the policy refused). Raw rows are kept for the raw retention (`analytics.raw_retention_days`).

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::db::{day_of, db_err, AnalyticsDb};

/// One tool call as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ToolCallRow {
    pub id: String,
    pub started_at_ms: u64,
    /// The client: `playground`, `cli`, `api`...
    pub client: String,
    pub conversation: Option<String>,
    pub model: Option<String>,
    pub tool: String,
    /// `safe`, `restricted`, `dangerous`.
    pub class: String,
    /// `builtin`, `mcp`, `installed`, `generated`.
    pub source: String,
    /// The arguments as JSON text, long strings shortened.
    pub args: String,
    /// The user approved this call when the policy asked.
    pub approved: bool,
    /// `ToolErrorCode` name; `None` = the call succeeded.
    pub error_code: Option<String>,
    pub duration_ms: u64,
    pub result_chars: u64,
    pub truncated: bool,
    /// Host a network tool contacted.
    pub network_target: Option<String>,
}

/// Filters for [`AnalyticsDb::tool_calls`] (all optional).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema, utoipa::IntoParams))]
pub struct ToolCallQuery {
    pub tool: Option<String>,
    pub client: Option<String>,
    /// Only calls that started at or after this time (a date range).
    pub from_ms: Option<u64>,
    /// Only calls that started before this time (paging, newest first).
    pub before_ms: Option<u64>,
    /// At most this many (default 50, at most 500).
    pub limit: Option<u32>,
}

impl AnalyticsDb {
    pub fn insert_tool_call(&self, c: &ToolCallRow) -> Result<(), String> {
        self.writable()?;
        let conn = self.lock();
        conn.execute(
            "INSERT OR REPLACE INTO tool_calls (id, started_at_ms, day, client, conversation_id, model,
                tool, class, source, args, approved, error_code, duration_ms, result_chars,
                truncated, network_target)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                c.id,
                c.started_at_ms.min(i64::MAX as u64) as i64,
                day_of(c.started_at_ms),
                c.client,
                c.conversation,
                c.model,
                c.tool,
                c.class,
                c.source,
                c.args,
                c.approved,
                c.error_code,
                c.duration_ms.min(i64::MAX as u64) as i64,
                c.result_chars.min(i64::MAX as u64) as i64,
                c.truncated,
                c.network_target,
            ],
        )
        .map_err(db_err)?;
        Ok(())
    }

    /// Tool calls, newest first.
    pub fn tool_calls(&self, q: &ToolCallQuery) -> Result<Vec<ToolCallRow>, String> {
        let conn = self.lock();
        let mut sql = String::from(
            "SELECT id, started_at_ms, client, conversation_id, model, tool, class, source, args,
                approved, error_code, duration_ms, result_chars, truncated, network_target
             FROM tool_calls WHERE 1 = 1",
        );
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(t) = q.tool.as_deref().filter(|s| !s.is_empty()) {
            sql.push_str(" AND tool = ?");
            args.push(t.to_string().into());
        }
        if let Some(c) = q.client.as_deref().filter(|s| !s.is_empty()) {
            sql.push_str(" AND client = ?");
            args.push(c.to_string().into());
        }
        if let Some(f) = q.from_ms {
            sql.push_str(" AND started_at_ms >= ?");
            args.push((f.min(i64::MAX as u64) as i64).into());
        }
        if let Some(b) = q.before_ms {
            sql.push_str(" AND started_at_ms < ?");
            args.push((b.min(i64::MAX as u64) as i64).into());
        }
        sql.push_str(" ORDER BY started_at_ms DESC, id LIMIT ?");
        args.push(i64::from(q.limit.unwrap_or(50).clamp(1, 500)).into());
        let mut st = conn.prepare(&sql).map_err(db_err)?;
        let rows = st
            .query_map(rusqlite::params_from_iter(args), |r| {
                Ok(ToolCallRow {
                    id: r.get(0)?,
                    started_at_ms: r.get::<_, i64>(1)?.max(0) as u64,
                    client: r.get(2)?,
                    conversation: r.get(3)?,
                    model: r.get(4)?,
                    tool: r.get(5)?,
                    class: r.get(6)?,
                    source: r.get(7)?,
                    args: r.get(8)?,
                    approved: r.get::<_, i64>(9)? != 0,
                    error_code: r.get(10)?,
                    duration_ms: r.get::<_, i64>(11)?.max(0) as u64,
                    result_chars: r.get::<_, i64>(12)?.max(0) as u64,
                    truncated: r.get::<_, i64>(13)? != 0,
                    network_target: r.get(14)?,
                })
            })
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::schema;

    fn row(id: &str, at: u64, tool: &str) -> ToolCallRow {
        ToolCallRow {
            id: id.into(),
            started_at_ms: at,
            client: "cli".into(),
            conversation: None,
            model: Some("qwen".into()),
            tool: tool.into(),
            class: "restricted".into(),
            source: "builtin".into(),
            args: r#"{"path":"a.txt"}"#.into(),
            approved: false,
            error_code: None,
            duration_ms: 3,
            result_chars: 10,
            truncated: false,
            network_target: None,
        }
    }

    #[test]
    fn rows_round_trip_filter_and_expire() {
        let dir = tempfile::tempdir().unwrap();
        let db = AnalyticsDb::open(&dir.path().join("a.db")).unwrap();
        let day = 86_400_000u64;
        let now = 100 * day;
        db.insert_tool_call(&row("old", now - 40 * day, "read_file"))
            .unwrap();
        let mut b = row("b", now - 2000, "write_file");
        b.approved = true;
        b.error_code = Some("failed".into());
        b.network_target = Some("example.com:443".into());
        db.insert_tool_call(&b).unwrap();
        db.insert_tool_call(&row("c", now - 1000, "read_file"))
            .unwrap();

        let all = db.tool_calls(&ToolCallQuery::default()).unwrap();
        assert_eq!(
            all.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["c", "b", "old"]
        );
        assert_eq!(all[1], b);
        let q = ToolCallQuery {
            tool: Some("read_file".into()),
            ..Default::default()
        };
        assert_eq!(db.tool_calls(&q).unwrap().len(), 2);
        let q = ToolCallQuery {
            before_ms: Some(now - 1500),
            limit: Some(1),
            ..Default::default()
        };
        assert_eq!(db.tool_calls(&q).unwrap()[0].id, "b");
        let q = ToolCallQuery {
            from_ms: Some(now - 1500),
            ..Default::default()
        };
        assert_eq!(db.tool_calls(&q).unwrap().len(), 1);

        // Retention follows the raw rows.
        db.maintain(now, 30).unwrap();
        let left = db.tool_calls(&ToolCallQuery::default()).unwrap();
        assert_eq!(left.len(), 2);
        db.clear().unwrap();
        assert!(db.tool_calls(&ToolCallQuery::default()).unwrap().is_empty());
    }

    /// A database written before the tool log existed (schema 3, with data) opens, gets the new
    /// table and keeps its rows.
    #[test]
    fn a_v3_database_migrates_and_keeps_its_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("analytics.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            for sql in &schema::MIGRATIONS[..3] {
                conn.execute_batch(sql).unwrap();
            }
            conn.execute(
                "INSERT INTO requests (id, started_at_ms, day, endpoint, alias, status, total_ms)
                 VALUES ('r1', 1000, 0, '/v1/chat/completions', 'qwen', 200, 5)",
                [],
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 3).unwrap();
        }
        let db = AnalyticsDb::open(&path).unwrap();
        assert!(!db.is_read_only());
        let version: i64 = db
            .lock()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, crate::analytics::SCHEMA_VERSION);
        assert!(version >= 4);
        assert_eq!(db.storage().unwrap().raw_rows, 1);
        db.insert_tool_call(&row("t1", 5, "get_info")).unwrap();
        assert_eq!(db.tool_calls(&ToolCallQuery::default()).unwrap().len(), 1);
    }
}
