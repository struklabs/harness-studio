//! Rate-limit counters of the gateway tokens in `analytics.db` (`rate_counters`).
//!
//! The in-memory buckets of `gateway::limits` write themselves here every couple of seconds
//! (only what changed) so a restart does not hand every token a full budget again, and so
//! several Harness processes on one data directory see the same budget.
//!
//! One row per (token, bucket kind): the level of the bucket at `at_ms` (wall clock), its
//! capacity and a revision that grows with every write. A bucket refills at `capacity / 60`
//! per second, so a row is only ever read forward in time: whatever window has passed since
//! `at_ms` is refilled on load (a counter whose window expired is full again; rows older than
//! a day are deleted).
//!
//! [`AnalyticsDb::rate_counters_sync`] is the only writer. One call is one immediate
//! transaction over all buckets of the caller:
//!
//! * the row still has the revision the caller wrote last: nobody else touched it, the
//!   caller's level is written as is (so a single process loses nothing to rounding);
//! * the revision differs: another process wrote in between. The stored level (refilled to
//!   now) is taken as the base and what this process consumed since its last sync
//!   (`pending`) is subtracted from it. Both processes' usage ends up in the row.

use rusqlite::{params, OptionalExtension, TransactionBehavior};

use super::db::{db_err, AnalyticsDb, DAY_MS};

/// Rows idle this long are deleted when the table is loaded (any bucket is full again after
/// a minute; a day also covers a token that was removed).
pub const COUNTER_FORGET_MS: u64 = DAY_MS;

/// A stored counter.
#[derive(Debug, Clone, PartialEq)]
pub struct CounterRow {
    pub token_id: String,
    pub kind: String,
    pub level: f64,
    pub capacity: u32,
    pub at_ms: u64,
    pub rev: i64,
}

/// One in-memory bucket handed to [`AnalyticsDb::rate_counters_sync`].
#[derive(Debug, Clone, PartialEq)]
pub struct CounterWrite {
    pub token_id: String,
    pub kind: &'static str,
    pub capacity: u32,
    /// Level now.
    pub level: f64,
    /// Units taken since the last sync (0 = nothing to write).
    pub pending: f64,
    /// Revision this process wrote or loaded last (0 = never).
    pub known_rev: i64,
}

/// What the bucket should hold after the sync.
#[derive(Debug, Clone, PartialEq)]
pub struct CounterOutcome {
    pub token_id: String,
    pub kind: &'static str,
    /// Level at the time of the sync.
    pub level: f64,
    pub rev: i64,
}

/// `level` refilled over `elapsed_ms` at `capacity / 60` per second, capped at `capacity`.
pub fn refilled(level: f64, capacity: u32, elapsed_ms: u64) -> f64 {
    let cap = f64::from(capacity);
    (level + elapsed_ms as f64 / 1000.0 * cap / 60.0).min(cap)
}

impl AnalyticsDb {
    /// Every stored counter; rows idle for [`COUNTER_FORGET_MS`] are deleted first.
    pub fn rate_counters_load(&self, now_ms: u64) -> Result<Vec<CounterRow>, String> {
        let conn = self.lock();
        if !self.is_read_only() {
            conn.execute(
                "DELETE FROM rate_counters WHERE at_ms < ?1",
                [now_ms.saturating_sub(COUNTER_FORGET_MS) as i64],
            )
            .map_err(db_err)?;
        }
        let mut st = conn
            .prepare("SELECT token_id, kind, level, capacity, at_ms, rev FROM rate_counters")
            .map_err(db_err)?;
        let rows = st
            .query_map([], |r| {
                Ok(CounterRow {
                    token_id: r.get(0)?,
                    kind: r.get(1)?,
                    level: r.get(2)?,
                    capacity: r.get::<_, i64>(3)?.clamp(0, i64::from(u32::MAX)) as u32,
                    at_ms: r.get::<_, i64>(4)?.max(0) as u64,
                    rev: r.get(5)?,
                })
            })
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Writes what changed and reads what other processes changed; see the module docs.
    pub fn rate_counters_sync(
        &self,
        items: &[CounterWrite],
        now_ms: u64,
    ) -> Result<Vec<CounterOutcome>, String> {
        self.writable()?;
        let mut conn = self.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let mut out = Vec::with_capacity(items.len());
        for it in items {
            let row: Option<(f64, i64, i64)> = tx
                .query_row(
                    "SELECT level, at_ms, rev FROM rate_counters WHERE token_id = ?1 AND kind = ?2",
                    params![it.token_id, it.kind],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(db_err)?;
            let dirty = it.pending > 0.0;
            let (level, rev, write) = match row {
                None => (it.level, it.known_rev + 1, dirty),
                Some((_, _, rev)) if rev == it.known_rev => (it.level, rev + 1, dirty),
                Some((stored, at_ms, rev)) => {
                    let base = refilled(
                        stored,
                        it.capacity,
                        now_ms.saturating_sub(at_ms.max(0) as u64),
                    );
                    let level = (base - it.pending).min(f64::from(it.capacity));
                    (level, if dirty { rev + 1 } else { rev }, dirty)
                }
            };
            if write {
                tx.execute(
                    "INSERT INTO rate_counters (token_id, kind, level, capacity, at_ms, rev)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(token_id, kind) DO UPDATE SET level = ?3, capacity = ?4,
                        at_ms = ?5, rev = ?6",
                    params![
                        it.token_id,
                        it.kind,
                        level,
                        i64::from(it.capacity),
                        now_ms as i64,
                        rev
                    ],
                )
                .map_err(db_err)?;
            }
            // Nothing stored and nothing to write: the bucket keeps what it has.
            let rev = match (&row, write) {
                (None, false) => it.known_rev,
                (Some((_, _, stored_rev)), false) if *stored_rev == it.known_rev => it.known_rev,
                _ => rev,
            };
            out.push(CounterOutcome {
                token_id: it.token_id.clone(),
                kind: it.kind,
                level,
                rev,
            });
        }
        tx.commit().map_err(db_err)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(token: &str, level: f64, pending: f64, known_rev: i64) -> CounterWrite {
        CounterWrite {
            token_id: token.into(),
            kind: "rpm",
            capacity: 60,
            level,
            pending,
            known_rev,
        }
    }

    #[test]
    fn writes_merges_foreign_changes_and_forgets_idle_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db = AnalyticsDb::open(&dir.path().join("a.db")).unwrap();
        let now = 10 * DAY_MS;
        // Nothing to write and nothing stored: nothing appears.
        let out = db
            .rate_counters_sync(&[write("a", 60.0, 0.0, 0)], now)
            .unwrap();
        assert_eq!((out[0].level, out[0].rev), (60.0, 0));
        assert!(db.rate_counters_load(now).unwrap().is_empty());
        // First write.
        let out = db
            .rate_counters_sync(&[write("a", 50.0, 10.0, 0)], now)
            .unwrap();
        assert_eq!((out[0].level, out[0].rev), (50.0, 1));
        // Same revision: the caller's level is stored as is.
        let out = db
            .rate_counters_sync(&[write("a", 45.0, 5.0, 1)], now + 1000)
            .unwrap();
        assert_eq!((out[0].level, out[0].rev), (45.0, 2));
        // Another process (still at rev 0): the stored level, refilled by 10 s at 1/s, is the
        // base and its own 3 units come off it.
        let out = db
            .rate_counters_sync(&[write("a", 57.0, 3.0, 0)], now + 11_000)
            .unwrap();
        assert_eq!(out[0].level, 45.0 + 10.0 - 3.0);
        assert_eq!(out[0].rev, 3);
        // A reader with nothing pending takes the stored level over without writing.
        let out = db
            .rate_counters_sync(&[write("a", 1.0, 0.0, 2)], now + 11_000)
            .unwrap();
        assert_eq!((out[0].level, out[0].rev), (52.0, 3));
        let rows = db.rate_counters_load(now + 11_000).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].rev, rows[0].capacity), (3, 60));
        // Rows untouched for a day are deleted on load.
        db.rate_counters_sync(&[write("old", 0.0, 1.0, 0)], now - 2 * DAY_MS)
            .unwrap();
        let rows = db.rate_counters_load(now + 11_000).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].token_id, "a");
    }
}
