//! Stored gateway Responses (`gateway_responses`, schema v6; see `crate::gateway::responses`).
//!
//! Every read and write is scoped: a row is only visible to the scope that stored it, and a
//! row older than the retention age (since its last use) is invisible even before
//! [`StateDb::prune_responses`] removes it.
//!
//! Deleting a response (expiry, limits, `DELETE`, "Delete all") overwrites its pages
//! (`secure_delete` for these statements only; the rest of `state.db` is unchanged), and
//! [`StateDb::clear_responses`] also truncates the WAL, so the content does not linger in the
//! files.

use super::{now_ms, StateDb, StateResult};
use crate::gateway::responses::{summary_of, ChainSummary, Retention, MAX_CHAIN};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

/// One persisted Responses SSE event. The JSON includes the same `sequence_number`.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredResponseEvent {
    pub sequence_number: i64,
    pub event: Value,
}

/// A response to store.
#[derive(Debug, Clone)]
pub struct NewStoredResponse {
    pub id: String,
    pub scope: String,
    pub previous_id: Option<String>,
    /// Model alias the gateway served it with.
    pub model: String,
    pub created_at: i64,
    /// The request's own input items (normalized, with ids).
    pub input: Vec<Value>,
    /// The response object as the client received it.
    pub response: Value,
}

/// A stored response.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredResponse {
    pub id: String,
    pub previous_id: Option<String>,
    pub model: String,
    pub created_at: i64,
    pub last_used_at: i64,
    pub input: Vec<Value>,
    pub response: Value,
}

/// A chain as a request sees it: the links after the newest compaction, oldest first, and that
/// compaction (its summary stands for everything before them).
#[derive(Debug, Clone, PartialEq)]
pub struct CompactedChain {
    pub links: Vec<StoredResponse>,
    pub summary: Option<ChainSummary>,
}

/// Size of the store (Settings > Gateway).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StoredResponsesStats {
    pub count: u64,
    pub bytes: u64,
}

fn cutoff(retention: &Retention, now: i64) -> i64 {
    now.saturating_sub(retention.max_age_ms)
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredResponse> {
    let input: String = r.get(5)?;
    let response: String = r.get(6)?;
    Ok(StoredResponse {
        id: r.get(0)?,
        previous_id: r.get(1)?,
        model: r.get(2)?,
        created_at: r.get(3)?,
        last_used_at: r.get(4)?,
        input: serde_json::from_str(&input).unwrap_or_default(),
        response: serde_json::from_str(&response).unwrap_or(Value::Null),
    })
}

fn get_row(
    c: &Connection,
    scope: &str,
    id: &str,
    min_used: i64,
) -> StateResult<Option<StoredResponse>> {
    Ok(c.query_row(
        "SELECT id, previous_id, model, created_at, last_used_at, input_json, response_json
         FROM gateway_responses WHERE id = ?1 AND scope = ?2 AND last_used_at >= ?3",
        params![id, scope, min_used],
        row,
    )
    .optional()?)
}

/// Runs `f` with `secure_delete` on: pages freed by it are overwritten with zeros.
fn scrubbing<T>(c: &Connection, f: impl FnOnce() -> StateResult<T>) -> StateResult<T> {
    c.pragma_update(None, "secure_delete", true)?;
    let out = f();
    c.pragma_update(None, "secure_delete", false)?;
    out
}

/// Removes expired rows, then the least recently used ones beyond the row and byte limits.
fn prune(c: &Connection, retention: &Retention, now: i64) -> StateResult<usize> {
    scrubbing(c, || prune_rows(c, retention, now))
}

fn prune_rows(c: &Connection, retention: &Retention, now: i64) -> StateResult<usize> {
    let mut n = c.execute(
        "DELETE FROM gateway_responses WHERE last_used_at < ?1",
        [cutoff(retention, now)],
    )?;
    n += c.execute(
        "DELETE FROM gateway_responses WHERE rid IN (
            SELECT rid FROM gateway_responses ORDER BY last_used_at DESC, rid DESC
            LIMIT -1 OFFSET ?1)",
        [retention.max_rows as i64],
    )?;
    n += c.execute(
        "DELETE FROM gateway_responses WHERE rid IN (
            SELECT rid FROM (
                SELECT rid, SUM(bytes) OVER (ORDER BY last_used_at DESC, rid DESC) AS total
                FROM gateway_responses)
            WHERE total > ?1)",
        [retention.max_bytes as i64],
    )?;
    Ok(n)
}

impl StateDb {
    /// Stores a response and applies retention. A response id stored by another scope is
    /// never overwritten.
    pub fn put_response(&self, r: &NewStoredResponse, retention: &Retention) -> StateResult<()> {
        let input = serde_json::to_string(&r.input).unwrap_or_else(|_| "[]".into());
        let response = serde_json::to_string(&r.response).unwrap_or_else(|_| "null".into());
        let bytes = (input.len() + response.len()) as i64;
        let now = now_ms();
        self.write(|tx| {
            tx.execute(
                "INSERT INTO gateway_responses(id, scope, previous_id, model, created_at,
                    last_used_at, input_json, response_json, bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                    previous_id = excluded.previous_id, model = excluded.model,
                    created_at = excluded.created_at, last_used_at = excluded.last_used_at,
                    input_json = excluded.input_json, response_json = excluded.response_json,
                    bytes = excluded.bytes
                 WHERE gateway_responses.scope = excluded.scope",
                params![
                    r.id,
                    r.scope,
                    r.previous_id,
                    r.model,
                    r.created_at,
                    input,
                    response,
                    bytes
                ],
            )?;
            tx.execute(
                "UPDATE gateway_responses SET bytes = ?1 + COALESCE((
                    SELECT SUM(LENGTH(event_json)) FROM gateway_response_events
                    WHERE response_id = ?2), 0)
                 WHERE id = ?2 AND scope = ?3",
                params![bytes, r.id, r.scope],
            )?;
            prune(tx, retention, now.max(r.created_at))?;
            Ok(())
        })
    }

    /// One stored response of `scope` (`None` if missing, another scope's or expired).
    pub fn get_response(
        &self,
        scope: &str,
        id: &str,
        retention: &Retention,
    ) -> StateResult<Option<StoredResponse>> {
        let min = cutoff(retention, now_ms());
        self.read(|c| get_row(c, scope, id, min))
    }

    /// Appends one event to a response owned by `scope`, assigning the next sequence number.
    /// `None` means the response is absent or belongs to another caller.
    pub fn append_response_event(
        &self,
        scope: &str,
        response_id: &str,
        mut event: Value,
        retention: &Retention,
    ) -> StateResult<Option<StoredResponseEvent>> {
        self.write(|tx| {
            let owned: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM gateway_responses WHERE id = ?1 AND scope = ?2)",
                params![response_id, scope],
                |r| r.get(0),
            )?;
            if !owned {
                return Ok(None);
            }
            let sequence_number: i64 = tx.query_row(
                "SELECT COALESCE(MAX(sequence_number) + 1, 0)
                 FROM gateway_response_events WHERE response_id = ?1",
                [response_id],
                |r| r.get(0),
            )?;
            if let Some(obj) = event.as_object_mut() {
                obj.insert("sequence_number".into(), sequence_number.into());
            }
            let encoded = serde_json::to_string(&event).unwrap_or_else(|_| "null".into());
            tx.execute(
                "INSERT INTO gateway_response_events(response_id, sequence_number, event_json)
                 VALUES (?1, ?2, ?3)",
                params![response_id, sequence_number, encoded],
            )?;
            tx.execute(
                "UPDATE gateway_responses
                 SET bytes = bytes + ?1, last_used_at = ?2
                 WHERE id = ?3 AND scope = ?4",
                params![encoded.len() as i64, now_ms(), response_id, scope],
            )?;
            prune(tx, retention, now_ms())?;
            Ok(Some(StoredResponseEvent {
                sequence_number,
                event,
            }))
        })
    }

    /// Events after `starting_after`, in sequence order. Scope is checked through the parent.
    pub fn response_events_after(
        &self,
        scope: &str,
        response_id: &str,
        starting_after: i64,
    ) -> StateResult<Vec<StoredResponseEvent>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT e.sequence_number, e.event_json
                 FROM gateway_response_events e
                 JOIN gateway_responses r ON r.id = e.response_id
                 WHERE e.response_id = ?1 AND r.scope = ?2 AND e.sequence_number > ?3
                 ORDER BY e.sequence_number ASC",
            )?;
            let rows = stmt.query_map(params![response_id, scope, starting_after], |r| {
                let encoded: String = r.get(1)?;
                Ok(StoredResponseEvent {
                    sequence_number: r.get(0)?,
                    event: serde_json::from_str(&encoded).unwrap_or(Value::Null),
                })
            })?;
            Ok(rows.filter_map(Result::ok).collect())
        })
    }

    /// Whether the response has a replayable stream. A queued event is written at creation,
    /// so this is true even before model generation starts.
    pub fn has_response_events(&self, scope: &str, response_id: &str) -> StateResult<bool> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM gateway_response_events e
                    JOIN gateway_responses r ON r.id = e.response_id
                    WHERE e.response_id = ?1 AND r.scope = ?2)",
                params![response_id, scope],
                |r| r.get(0),
            )?)
        })
    }

    /// The chain ending at `id`, oldest first, following `previous_id`. `Err(missing)` names
    /// the first link that is missing (or another scope's, or expired). Every link's
    /// `last_used_at` moves to now, so a conversation in use outlives retention.
    pub fn response_chain(
        &self,
        scope: &str,
        id: &str,
        retention: &Retention,
    ) -> StateResult<Result<Vec<StoredResponse>, String>> {
        Ok(self
            .walk_chain(scope, id, retention, false)?
            .map(|(chain, _)| chain))
    }

    /// Like [`StateDb::response_chain`], but a compaction ends the walk: the links up to the
    /// newest summary's `through` are not read (they may even be gone), and the summary comes
    /// back with the links after it.
    pub fn response_chain_compacted(
        &self,
        scope: &str,
        id: &str,
        retention: &Retention,
    ) -> StateResult<Result<CompactedChain, String>> {
        Ok(self
            .walk_chain(scope, id, retention, true)?
            .map(|(links, summary)| CompactedChain { links, summary }))
    }

    #[allow(clippy::type_complexity)]
    fn walk_chain(
        &self,
        scope: &str,
        id: &str,
        retention: &Retention,
        compacted: bool,
    ) -> StateResult<Result<(Vec<StoredResponse>, Option<ChainSummary>), String>> {
        let now = now_ms();
        let min = cutoff(retention, now);
        let chain = self.read(|c| {
            let mut chain: Vec<StoredResponse> = Vec::new();
            let mut summary: Option<ChainSummary> = None;
            let mut next = Some(id.to_string());
            while let Some(cur) = next.take() {
                if chain.len() >= MAX_CHAIN || chain.iter().any(|r| r.id == cur) {
                    break;
                }
                // Everything up to `through` is replaced by the summary.
                if summary.as_ref().is_some_and(|s| s.through == cur) {
                    break;
                }
                match get_row(c, scope, &cur, min)? {
                    Some(r) => {
                        if compacted && summary.is_none() {
                            summary = summary_of(&r.response);
                        }
                        next = r.previous_id.clone();
                        chain.push(r);
                    }
                    None => return Ok(Err(cur)),
                }
            }
            chain.reverse();
            Ok(Ok((chain, summary)))
        })?;
        let Ok((chain, summary)) = chain else {
            return Ok(chain);
        };
        if self.is_read_only() {
            return Ok(Ok((chain, summary)));
        }
        self.write(|tx| {
            let mut stmt = tx.prepare_cached(
                "UPDATE gateway_responses SET last_used_at = ?1 WHERE id = ?2 AND scope = ?3",
            )?;
            for r in &chain {
                stmt.execute(params![now, r.id, scope])?;
            }
            Ok(())
        })?;
        Ok(Ok((chain, summary)))
    }

    /// Deletes one stored response of `scope`; `false` if there was none.
    pub fn delete_response(&self, scope: &str, id: &str) -> StateResult<bool> {
        self.write(|tx| {
            scrubbing(tx, || {
                Ok(tx.execute(
                    "DELETE FROM gateway_responses WHERE id = ?1 AND scope = ?2",
                    params![id, scope],
                )? > 0)
            })
        })
    }

    /// Applies retention at `now` (Unix ms); returns the number of rows removed.
    pub fn prune_responses(&self, retention: &Retention, now: i64) -> StateResult<usize> {
        self.write(|tx| prune(tx, retention, now))
    }

    pub fn responses_stats(&self) -> StateResult<StoredResponsesStats> {
        self.read(|c| {
            let (count, bytes): (i64, i64) = c.query_row(
                "SELECT COUNT(*), COALESCE(SUM(bytes), 0) FROM gateway_responses",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok(StoredResponsesStats {
                count: count.max(0) as u64,
                bytes: bytes.max(0) as u64,
            })
        })
    }

    /// Deletes every stored response (all scopes) and truncates the WAL; returns how many
    /// there were.
    pub fn clear_responses(&self) -> StateResult<usize> {
        let n = self
            .write(|tx| scrubbing(tx, || Ok(tx.execute("DELETE FROM gateway_responses", [])?)))?;
        self.read(|c| {
            // Best effort: a reader elsewhere can keep the WAL from being truncated.
            let _ = c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
            Ok(())
        })?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::responses::{
        chain_context, normalize_input, Link, ResponsesSettings, COMPACTION_KEY,
    };
    use serde_json::json;

    fn db() -> (tempfile::TempDir, StateDb) {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).unwrap();
        (dir, db)
    }

    fn resp(id: &str, prev: Option<&str>, scope: &str, text: &str, at: i64) -> NewStoredResponse {
        NewStoredResponse {
            id: id.into(),
            scope: scope.into(),
            previous_id: prev.map(str::to_string),
            model: "m".into(),
            created_at: at,
            input: normalize_input(Some(&json!(format!("user says {text}")))),
            response: json!({"id": id, "object": "response", "output": [
                {"type":"message","role":"assistant","content":[{"type":"output_text","text": text}]}
            ]}),
        }
    }

    fn keep_all() -> Retention {
        ResponsesSettings::default().retention()
    }

    #[test]
    fn chain_is_built_oldest_first_and_scoped() {
        let (_d, db) = db();
        let now = now_ms();
        let r = keep_all();
        db.put_response(&resp("resp_a", None, "token:1", "A", now), &r)
            .unwrap();
        db.put_response(&resp("resp_b", Some("resp_a"), "token:1", "B", now), &r)
            .unwrap();
        db.put_response(&resp("resp_c", Some("resp_b"), "token:1", "C", now), &r)
            .unwrap();
        let chain = db.response_chain("token:1", "resp_c", &r).unwrap().unwrap();
        assert_eq!(
            chain.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["resp_a", "resp_b", "resp_c"]
        );
        let links: Vec<Link> = chain
            .iter()
            .map(|c| Link {
                input: &c.input,
                response: &c.response,
            })
            .collect();
        let ctx = chain_context(&links);
        let texts: Vec<&str> = ctx
            .iter()
            .map(|v| v["content"][0]["text"].as_str().unwrap())
            .collect();
        assert_eq!(
            texts,
            ["user says A", "A", "user says B", "B", "user says C", "C"]
        );
        // Another token (or anonymous) sees nothing of it.
        assert_eq!(
            db.response_chain("token:2", "resp_c", &r).unwrap(),
            Err("resp_c".to_string())
        );
        assert!(db
            .get_response("anonymous", "resp_a", &r)
            .unwrap()
            .is_none());
        assert!(!db.delete_response("token:2", "resp_a").unwrap());
        // Another scope cannot overwrite an id either.
        db.put_response(&resp("resp_a", None, "token:2", "X", now), &r)
            .unwrap();
        let a = db.get_response("token:1", "resp_a", &r).unwrap().unwrap();
        assert_eq!(a.response["output"][0]["content"][0]["text"], "A");
        assert!(db.get_response("token:2", "resp_a", &r).unwrap().is_none());
        // A deleted link breaks the chain and is named.
        assert!(db.delete_response("token:1", "resp_b").unwrap());
        assert_eq!(
            db.response_chain("token:1", "resp_c", &r).unwrap(),
            Err("resp_b".to_string())
        );
    }

    #[test]
    fn a_compaction_ends_the_walk_and_survives_pruned_links() {
        let (_d, db) = db();
        let now = now_ms();
        let r = keep_all();
        for (id, prev) in [
            ("resp_a", None),
            ("resp_b", Some("resp_a")),
            ("resp_c", Some("resp_b")),
            ("resp_d", Some("resp_c")),
        ] {
            db.put_response(&resp(id, prev, "s", &id.to_uppercase(), now), &r)
                .unwrap();
        }
        // resp_d summarizes everything up to resp_b; resp_c stays verbatim.
        let mut d = resp("resp_d", Some("resp_c"), "s", "D", now);
        d.response[COMPACTION_KEY] = serde_json::json!({
            "through": "resp_b", "summary": "A and B happened.", "tokens_before": 400,
            "tokens_after": 120, "model": "m", "links": 2, "created_at": now});
        db.put_response(&d, &r).unwrap();
        let c = db
            .response_chain_compacted("s", "resp_d", &r)
            .unwrap()
            .unwrap();
        assert_eq!(
            c.links.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
            ["resp_c", "resp_d"]
        );
        assert_eq!(c.summary.as_ref().unwrap().summary, "A and B happened.");
        // The plain chain still has everything.
        let full = db.response_chain("s", "resp_d", &r).unwrap().unwrap();
        assert_eq!(full.len(), 4);
        // The summarized links may be gone (retention): the compacted chain does not need them.
        db.delete_response("s", "resp_a").unwrap();
        db.delete_response("s", "resp_b").unwrap();
        assert!(db.response_chain("s", "resp_d", &r).unwrap().is_err());
        let c = db
            .response_chain_compacted("s", "resp_d", &r)
            .unwrap()
            .unwrap();
        assert_eq!(c.links.len(), 2);
        // From a link after the marker's response the summary is found further back.
        db.put_response(&resp("resp_e", Some("resp_d"), "s", "E", now), &r)
            .unwrap();
        let c = db
            .response_chain_compacted("s", "resp_e", &r)
            .unwrap()
            .unwrap();
        assert_eq!(
            c.links.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
            ["resp_c", "resp_d", "resp_e"]
        );
        assert!(c.summary.is_some());
        // Before the compaction (continuing resp_c itself) there is no summary.
        let c = db.response_chain_compacted("s", "resp_c", &r).unwrap();
        assert_eq!(c, Err("resp_b".to_string()));
    }

    #[test]
    fn retention_drops_expired_and_least_recently_used() {
        let (_d, db) = db();
        let now = now_ms();
        let day = 86_400_000;
        let big = Retention {
            max_age_ms: 30 * day,
            max_rows: 1000,
            max_bytes: u64::MAX / 4,
        };
        // An old row: invisible at once, removed by the next prune.
        db.put_response(&resp("resp_old", None, "s", "old", now - 40 * day), &big)
            .unwrap();
        assert!(db.get_response("s", "resp_old", &big).unwrap().is_none());
        assert_eq!(db.responses_stats().unwrap().count, 0, "pruned on insert");

        // Row limit: the least recently used go first; continuing a chain refreshes it.
        let rows = Retention { max_rows: 3, ..big };
        for (i, id) in ["resp_1", "resp_2", "resp_3"].iter().enumerate() {
            db.put_response(&resp(id, None, "s", id, now - 10 + i as i64), &rows)
                .unwrap();
        }
        // resp_1 is the oldest, but continuing it makes it the most recently used.
        db.response_chain("s", "resp_1", &rows).unwrap().unwrap();
        db.put_response(&resp("resp_4", None, "s", "resp_4", now), &rows)
            .unwrap();
        let st = db.responses_stats().unwrap();
        assert_eq!(st.count, 3);
        assert!(db.get_response("s", "resp_2", &rows).unwrap().is_none());
        assert!(db.get_response("s", "resp_1", &rows).unwrap().is_some());

        // Byte limit: keep only what fits, newest first.
        let one = db.get_response("s", "resp_4", &rows).unwrap().unwrap();
        let size = serde_json::to_string(&one.input).unwrap().len()
            + serde_json::to_string(&one.response).unwrap().len();
        let bytes = Retention {
            max_bytes: (size * 2) as u64,
            ..rows
        };
        db.prune_responses(&bytes, now_ms()).unwrap();
        assert_eq!(db.responses_stats().unwrap().count, 2);
        assert!(db.responses_stats().unwrap().bytes <= (size * 2) as u64);

        // Age since last use.
        let removed = db.prune_responses(&big, now_ms() + 31 * day).unwrap();
        assert_eq!(removed, 2);
        assert_eq!(
            db.responses_stats().unwrap(),
            StoredResponsesStats::default()
        );

        db.put_response(&resp("resp_x", None, "s", "x", now_ms()), &big)
            .unwrap();
        assert_eq!(db.clear_responses().unwrap(), 1);
    }

    #[test]
    fn deleted_content_does_not_stay_in_the_files() {
        let (dir, db) = db();
        let marker = "HSRESPSECRET4471";
        let r = keep_all();
        db.put_response(&resp("resp_s", None, "s", marker, now_ms()), &r)
            .unwrap();
        let in_files = || {
            std::fs::read_dir(dir.path())
                .unwrap()
                .map(|e| std::fs::read(e.unwrap().path()).unwrap())
                .any(|bytes| bytes.windows(marker.len()).any(|w| w == marker.as_bytes()))
        };
        assert!(in_files(), "stored");
        assert_eq!(db.clear_responses().unwrap(), 1);
        assert!(!in_files(), "overwritten after clear");
    }

    #[test]
    fn response_events_are_ordered_scoped_and_cascade_with_the_response() {
        let (_dir, db) = db();
        let retention = keep_all();
        db.put_response(
            &resp("resp_stream", None, "token:one", "answer", now_ms()),
            &retention,
        )
        .unwrap();
        let first = db
            .append_response_event(
                "token:one",
                "resp_stream",
                json!({"type":"response.created","sequence_number":99}),
                &retention,
            )
            .unwrap()
            .unwrap();
        let second = db
            .append_response_event(
                "token:one",
                "resp_stream",
                json!({"type":"response.completed"}),
                &retention,
            )
            .unwrap()
            .unwrap();
        assert_eq!(first.sequence_number, 0);
        assert_eq!(first.event["sequence_number"], 0);
        assert_eq!(second.sequence_number, 1);
        assert!(db
            .append_response_event(
                "token:two",
                "resp_stream",
                json!({"type":"stolen"}),
                &retention,
            )
            .unwrap()
            .is_none());
        assert!(db
            .response_events_after("token:two", "resp_stream", -1)
            .unwrap()
            .is_empty());
        let replay = db
            .response_events_after("token:one", "resp_stream", 0)
            .unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].event["type"], "response.completed");
        assert!(db.has_response_events("token:one", "resp_stream").unwrap());
        db.delete_response("token:one", "resp_stream").unwrap();
        assert!(db
            .response_events_after("token:one", "resp_stream", -1)
            .unwrap()
            .is_empty());
    }
}
