//! Per-token rate limits of the gateway. The limits are stored with the named
//! token (`token_policy::RateLimits`); the counters live here, in memory.
//!
//! # Persistence
//! With [`RateLimiter::persistent`] the request and token buckets are also kept in
//! `analytics.db` (`rate_counters`, see `analytics::counters`), so a restart does not give
//! every token a full budget again and several Harness processes on one data directory count
//! against the same budget:
//!
//! * Nothing touches the disk on the request path. The buckets are loaded once at start; a
//!   background thread (`harness-limits`) writes what changed every [`FLUSH_EVERY`] (one
//!   small transaction, nothing when idle) and, in the same transaction, reads what other
//!   processes wrote. The last state is written when the limiter goes away.
//! * The stored level is refilled by wall-clock time on load, so a counter whose minute
//!   window has expired is full again; rows idle for a day are deleted.
//! * Several processes: each one adds its own consumption to the shared row at its next
//!   flush, so they see each other's usage with a delay of up to [`FLUSH_EVERY`] (a burst can
//!   slip through in that time). If a process crashes, at most its last [`FLUSH_EVERY`] of
//!   usage is lost.
//! * The concurrency limit is not shared or stored: it counts requests in flight in this
//!   process.
//!
//! * **Requests per minute:** token bucket of `rpm` requests, refilled at `rpm / 60` per
//!   second. A burst of `rpm` requests passes, then one every `60 / rpm` seconds.
//! * **Tokens per minute:** bucket of `tpm` tokens, refilled at `tpm / 60` per second. A
//!   request is admitted while the bucket is above zero; its prompt + completion tokens are
//!   charged when it ends ([`Permit::complete`]), so the bucket can go negative and the next
//!   request waits until it has recovered.
//! * **Concurrency:** requests of the token in flight; the [`Permit`] holds one slot until it
//!   is dropped (a stream holds it until it ends or the client goes away).
//!
//! A rejected request consumes nothing. [`Rejection::retry_after_secs`] is the whole number of
//! seconds after which the same request would pass (at least 1), for `Retry-After`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use crate::analytics::counters::{refilled, CounterRow, CounterWrite};
use crate::analytics::AnalyticsDb;
use crate::token_policy::RateLimits;

/// How often the counters are written to (and read from) the database.
pub const FLUSH_EVERY: Duration = Duration::from_secs(2);

const KIND_REQUESTS: &str = "rpm";
const KIND_TOKENS: &str = "tpm";

/// Which limit rejected a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    RequestsPerMinute,
    TokensPerMinute,
    Concurrency,
}

impl LimitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LimitKind::RequestsPerMinute => "requests_per_minute",
            LimitKind::TokensPerMinute => "tokens_per_minute",
            LimitKind::Concurrency => "max_concurrent",
        }
    }
}

/// A rejected request.
#[derive(Debug, Clone, PartialEq)]
pub struct Rejection {
    pub kind: LimitKind,
    /// The configured limit that was hit.
    pub limit: u32,
    pub retry_after_secs: u64,
    pub snapshot: Snapshot,
}

/// Limits and what is left of them, for the `x-ratelimit-*` response headers.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Snapshot {
    pub requests_limit: Option<u32>,
    /// Whole requests left right now.
    pub requests_remaining: Option<u64>,
    /// Seconds until the request bucket is full again.
    pub requests_reset_secs: Option<f64>,
    pub tokens_limit: Option<u32>,
    /// Whole tokens left right now (0 while overdrawn).
    pub tokens_remaining: Option<u64>,
    pub tokens_reset_secs: Option<f64>,
    pub concurrent_limit: Option<u32>,
    pub in_flight: u32,
}

/// A token bucket with `capacity` units refilled at `capacity / 60` units per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bucket {
    level: f64,
    at: Instant,
    capacity: u32,
    /// Units taken since the last write to the database.
    pending: f64,
    /// Revision of the stored row this bucket last wrote or read (0 = none).
    rev: i64,
}

impl Bucket {
    pub fn full(capacity: u32, now: Instant) -> Self {
        Self {
            level: f64::from(capacity),
            at: now,
            capacity,
            pending: 0.0,
            rev: 0,
        }
    }

    /// A bucket continued from a stored row: refilled over the wall-clock time since it was
    /// written (`elapsed_ms`), so an expired window is full again.
    fn restored(row: &CounterRow, capacity: u32, elapsed_ms: u64, now: Instant) -> Self {
        Self {
            level: refilled(row.level, capacity, elapsed_ms),
            at: now,
            capacity,
            pending: 0.0,
            rev: row.rev,
        }
    }

    fn rate(capacity: u32) -> f64 {
        f64::from(capacity) / 60.0
    }

    /// Adds what accrued since the last call, capped at `capacity` (a lowered limit also
    /// lowers what is left).
    pub fn refill(&mut self, capacity: u32, now: Instant) {
        let elapsed = now.saturating_duration_since(self.at).as_secs_f64();
        self.at = now;
        self.capacity = capacity;
        self.level = (self.level + elapsed * Self::rate(capacity)).min(f64::from(capacity));
    }

    pub fn level(&self) -> f64 {
        self.level
    }

    /// Seconds until the level reaches `target` (0 when it already has).
    pub fn secs_until(&self, target: f64, capacity: u32) -> f64 {
        if self.level >= target || capacity == 0 {
            return 0.0;
        }
        (target - self.level) / Self::rate(capacity)
    }

    pub fn take(&mut self, units: f64) {
        self.level -= units;
        self.pending += units;
    }
}

/// `Retry-After` value: whole seconds, at least 1.
pub fn retry_secs(secs: f64) -> u64 {
    if !secs.is_finite() {
        return 60;
    }
    (secs.ceil() as u64).max(1)
}

#[derive(Debug)]
struct TokenState {
    requests: Option<Bucket>,
    tokens: Option<Bucket>,
    in_flight: u32,
    last_seen: Instant,
    /// Stored rows (requests, tokens) a bucket is created from the first time its limit is
    /// seen.
    stored: [Option<CounterRow>; 2],
}

impl TokenState {
    fn new(now: Instant, stored: [Option<CounterRow>; 2]) -> Self {
        Self {
            requests: None,
            tokens: None,
            in_flight: 0,
            last_seen: now,
            stored,
        }
    }

    /// Brings both buckets up to `now` under the current limits (a limit that was just set
    /// starts full, or continues from its stored row; a removed one is dropped). `wall_ms`
    /// is the wall-clock time of `now`.
    fn sync(&mut self, limits: &RateLimits, now: Instant, wall_ms: u64) {
        sync_bucket(
            &mut self.requests,
            limits.requests_per_minute,
            now,
            self.stored[0].take(),
            wall_ms,
        );
        sync_bucket(
            &mut self.tokens,
            limits.tokens_per_minute,
            now,
            self.stored[1].take(),
            wall_ms,
        );
        self.last_seen = now;
    }

    fn snapshot(&self, limits: &RateLimits) -> Snapshot {
        let rpm = limits.requests_per_minute;
        let tpm = limits.tokens_per_minute;
        Snapshot {
            requests_limit: rpm,
            requests_remaining: self.requests.map(|b| b.level().max(0.0).floor() as u64),
            requests_reset_secs: self
                .requests
                .zip(rpm)
                .map(|(b, c)| b.secs_until(f64::from(c), c)),
            tokens_limit: tpm,
            tokens_remaining: self.tokens.map(|b| b.level().max(0.0).floor() as u64),
            tokens_reset_secs: self
                .tokens
                .zip(tpm)
                .map(|(b, c)| b.secs_until(f64::from(c), c)),
            concurrent_limit: limits.max_concurrent,
            in_flight: self.in_flight,
        }
    }
}

fn sync_bucket(
    bucket: &mut Option<Bucket>,
    capacity: Option<u32>,
    now: Instant,
    stored: Option<CounterRow>,
    wall_ms: u64,
) {
    match capacity {
        None => *bucket = None,
        Some(c) => match bucket {
            Some(b) => b.refill(c, now),
            None => {
                *bucket = Some(match stored {
                    Some(row) => Bucket::restored(&row, c, wall_ms.saturating_sub(row.at_ms), now),
                    None => Bucket::full(c, now),
                })
            }
        },
    }
}

/// The database side: where the buckets are stored and how an [`Instant`] maps to wall time.
struct Persist {
    db: Arc<AnalyticsDb>,
    anchor: Instant,
    anchor_ms: u64,
    /// Rows read at start, handed to a token's state when it first appears.
    restored: Mutex<HashMap<String, [Option<CounterRow>; 2]>>,
    warned: AtomicBool,
}

impl Persist {
    fn wall_ms(&self, now: Instant) -> u64 {
        self.anchor_ms + now.saturating_duration_since(self.anchor).as_millis() as u64
    }
}

struct Core {
    map: Mutex<HashMap<String, TokenState>>,
    persist: Option<Persist>,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core")
            .field("persistent", &self.persist.is_some())
            .finish()
    }
}

type Shared = Arc<Core>;

impl Core {
    fn wall_ms(&self, now: Instant) -> u64 {
        self.persist.as_ref().map_or(0, |p| p.wall_ms(now))
    }

    fn new_state(&self, token_id: &str, now: Instant) -> TokenState {
        let stored = self
            .persist
            .as_ref()
            .and_then(|p| {
                p.restored
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(token_id)
            })
            .unwrap_or_default();
        TokenState::new(now, stored)
    }

    /// Writes what changed and takes over what other processes wrote (see
    /// `analytics::counters`). On an error nothing is lost: what was taken stays pending.
    fn flush(&self, now: Instant) -> Result<(), String> {
        let Some(p) = &self.persist else {
            return Ok(());
        };
        let wall = p.wall_ms(now);
        let items: Vec<CounterWrite> = {
            let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
            let mut items = Vec::new();
            for (id, state) in map.iter_mut() {
                for (kind, bucket) in [
                    (KIND_REQUESTS, state.requests.as_mut()),
                    (KIND_TOKENS, state.tokens.as_mut()),
                ] {
                    let Some(b) = bucket else { continue };
                    b.refill(b.capacity, now);
                    items.push(CounterWrite {
                        token_id: id.clone(),
                        kind,
                        capacity: b.capacity,
                        level: b.level,
                        pending: std::mem::take(&mut b.pending),
                        known_rev: b.rev,
                    });
                }
            }
            items
        };
        if items.is_empty() {
            return Ok(());
        }
        let result = p.db.rate_counters_sync(&items, wall);
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        fn bucket_of<'a>(
            map: &'a mut HashMap<String, TokenState>,
            id: &str,
            kind: &str,
        ) -> Option<&'a mut Bucket> {
            let state = map.get_mut(id)?;
            if kind == KIND_REQUESTS {
                state.requests.as_mut()
            } else {
                state.tokens.as_mut()
            }
        }
        match result {
            Ok(outcomes) => {
                p.warned.store(false, Ordering::Relaxed);
                for (item, out) in items.iter().zip(&outcomes) {
                    if let Some(b) = bucket_of(&mut map, &item.token_id, item.kind) {
                        b.level += out.level - item.level;
                        b.rev = out.rev;
                    }
                }
                Ok(())
            }
            Err(e) => {
                for item in &items {
                    if let Some(b) = bucket_of(&mut map, &item.token_id, item.kind) {
                        b.pending += item.pending;
                    }
                }
                Err(e)
            }
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        // The last write; a failure here has nobody left to tell.
        let _ = self.flush(Instant::now());
    }
}

/// The counters of every token (one per gateway; cheap to clone).
#[derive(Clone)]
pub struct RateLimiter {
    inner: Shared,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self {
            inner: Arc::new(Core {
                map: Mutex::new(HashMap::new()),
                persist: None,
            }),
        }
    }
}

impl std::fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateLimiter")
            .field("persistent", &self.inner.persist.is_some())
            .finish()
    }
}

/// Entries idle this long (and with nothing in flight) are forgotten.
const IDLE_FORGET: Duration = Duration::from_secs(600);

impl RateLimiter {
    /// Counters in memory only (they start empty).
    pub fn new() -> Self {
        Self::default()
    }

    /// Counters continued from `db` and written back every [`FLUSH_EVERY`] by a background
    /// thread. When the stored counters cannot be read the limiter still works, in memory.
    pub fn persistent(db: Arc<AnalyticsDb>) -> Self {
        let limiter = Self::with_store(db, Instant::now(), crate::analytics::now_ms());
        let weak: Weak<Core> = Arc::downgrade(&limiter.inner);
        let spawned = std::thread::Builder::new()
            .name("harness-limits".into())
            .spawn(move || loop {
                std::thread::sleep(FLUSH_EVERY);
                let Some(core) = weak.upgrade() else { return };
                if let Err(e) = core.flush(Instant::now()) {
                    if let Some(p) = &core.persist {
                        if !p.warned.swap(true, Ordering::Relaxed) {
                            eprintln!("[Harness Limits] {e}");
                        }
                    }
                }
            });
        if let Err(e) = spawned {
            eprintln!("[Harness Limits] {e}");
        }
        limiter
    }

    /// [`persistent`](Self::persistent) without the thread and with `anchor` (an instant) and
    /// `anchor_ms` (its wall-clock time) given: [`flush_at`](Self::flush_at) then decides when
    /// the database is touched (tests, and two limiters standing in for two processes).
    pub fn with_store(db: Arc<AnalyticsDb>, anchor: Instant, anchor_ms: u64) -> Self {
        let mut restored: HashMap<String, [Option<CounterRow>; 2]> = HashMap::new();
        match db.rate_counters_load(anchor_ms) {
            Ok(rows) => {
                for row in rows {
                    let slot = match row.kind.as_str() {
                        KIND_REQUESTS => 0,
                        KIND_TOKENS => 1,
                        _ => continue,
                    };
                    let id = row.token_id.clone();
                    restored.entry(id).or_default()[slot] = Some(row);
                }
            }
            Err(e) => eprintln!("[Harness Limits] {e}"),
        }
        Self {
            inner: Arc::new(Core {
                map: Mutex::new(HashMap::new()),
                persist: Some(Persist {
                    db,
                    anchor,
                    anchor_ms,
                    restored: Mutex::new(restored),
                    warned: AtomicBool::new(false),
                }),
            }),
        }
    }

    /// Writes the counters to the database now (and takes over what other processes wrote).
    pub fn flush(&self) -> Result<(), String> {
        self.inner.flush(Instant::now())
    }

    pub fn flush_at(&self, now: Instant) -> Result<(), String> {
        self.inner.flush(now)
    }

    /// Admits one request of `token_id` under `limits` now.
    pub fn admit(&self, token_id: &str, limits: &RateLimits) -> Result<Permit, Rejection> {
        self.admit_at(token_id, limits, Instant::now())
    }

    /// [`admit`](Self::admit) at a given instant (the math is tested with explicit times).
    pub fn admit_at(
        &self,
        token_id: &str,
        limits: &RateLimits,
        now: Instant,
    ) -> Result<Permit, Rejection> {
        let mut map = self.inner.map.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() > 64 {
            // A bucket with usage not written yet stays until the next flush.
            map.retain(|_, s| {
                s.in_flight > 0
                    || s.requests.is_some_and(|b| b.pending > 0.0)
                    || s.tokens.is_some_and(|b| b.pending > 0.0)
                    || now.saturating_duration_since(s.last_seen) < IDLE_FORGET
            });
        }
        let state = map
            .entry(token_id.to_string())
            .or_insert_with(|| self.inner.new_state(token_id, now));
        state.sync(limits, now, self.inner.wall_ms(now));

        // Every limit is checked before anything is consumed; the longest wait is reported.
        let mut rejection: Option<(LimitKind, u32, f64)> = None;
        let mut reject = |kind, limit, secs: f64| match rejection {
            Some((_, _, s)) if s >= secs => {}
            _ => rejection = Some((kind, limit, secs)),
        };
        if let Some(cap) = limits.max_concurrent {
            if state.in_flight >= cap {
                // Nothing to compute: a slot frees when a running request ends.
                reject(LimitKind::Concurrency, cap, 1.0);
            }
        }
        if let (Some(rpm), Some(b)) = (limits.requests_per_minute, state.requests.as_ref()) {
            if b.level() < 1.0 {
                reject(LimitKind::RequestsPerMinute, rpm, b.secs_until(1.0, rpm));
            }
        }
        if let (Some(tpm), Some(b)) = (limits.tokens_per_minute, state.tokens.as_ref()) {
            if b.level() <= 0.0 {
                // Strictly above zero: a tiny epsilon past the exact refill instant.
                let secs = b.secs_until(0.0, tpm) + 1e-6;
                reject(LimitKind::TokensPerMinute, tpm, secs);
            }
        }
        if let Some((kind, limit, secs)) = rejection {
            return Err(Rejection {
                kind,
                limit,
                retry_after_secs: retry_secs(secs),
                snapshot: state.snapshot(limits),
            });
        }
        if let Some(b) = state.requests.as_mut() {
            b.take(1.0);
        }
        state.in_flight += 1;
        let snapshot = state.snapshot(limits);
        drop(map);
        Ok(Permit {
            inner: Some(self.inner.clone()),
            token_id: token_id.to_string(),
            limits: *limits,
            snapshot,
        })
    }

    /// Current state of a token (for tests and diagnostics).
    pub fn snapshot(&self, token_id: &str, limits: &RateLimits) -> Snapshot {
        self.snapshot_at(token_id, limits, Instant::now())
    }

    pub fn snapshot_at(&self, token_id: &str, limits: &RateLimits, now: Instant) -> Snapshot {
        let mut map = self.inner.map.lock().unwrap_or_else(|e| e.into_inner());
        let state = map
            .entry(token_id.to_string())
            .or_insert_with(|| self.inner.new_state(token_id, now));
        state.sync(limits, now, self.inner.wall_ms(now));
        state.snapshot(limits)
    }
}

/// One admitted request. Holds a concurrency slot until dropped; [`complete`](Self::complete)
/// charges the tokens it used.
#[derive(Debug)]
pub struct Permit {
    inner: Option<Shared>,
    token_id: String,
    limits: RateLimits,
    /// State right after admission (response headers).
    pub snapshot: Snapshot,
}

impl Permit {
    /// Charges `tokens` (prompt + completion) against the tokens-per-minute budget and frees
    /// the slot.
    pub fn complete(self, tokens: u64) {
        self.complete_at(tokens, Instant::now());
    }

    pub fn complete_at(mut self, tokens: u64, now: Instant) {
        if let Some(inner) = self.inner.as_ref() {
            let mut map = inner.map.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(state) = map.get_mut(&self.token_id) {
                state.sync(&self.limits, now, inner.wall_ms(now));
                if let Some(b) = state.tokens.as_mut() {
                    b.take(tokens as f64);
                }
            }
        }
        self.release();
    }

    fn release(&mut self) {
        if let Some(inner) = self.inner.take() {
            let mut map = inner.map.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(state) = map.get_mut(&self.token_id) {
                state.in_flight = state.in_flight.saturating_sub(1);
            }
        }
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(rpm: Option<u32>, tpm: Option<u32>, conc: Option<u32>) -> RateLimits {
        RateLimits {
            requests_per_minute: rpm,
            tokens_per_minute: tpm,
            max_concurrent: conc,
        }
    }

    #[test]
    fn bucket_math() {
        let t0 = Instant::now();
        let mut b = Bucket::full(60, t0);
        assert_eq!(b.level(), 60.0);
        b.take(60.0);
        assert_eq!(b.secs_until(1.0, 60), 1.0, "60/min refills one per second");
        b.refill(60, t0 + Duration::from_millis(500));
        assert!((b.level() - 0.5).abs() < 1e-9);
        assert!((b.secs_until(1.0, 60) - 0.5).abs() < 1e-9);
        b.refill(60, t0 + Duration::from_secs(3600));
        assert_eq!(b.level(), 60.0, "capped at capacity");
        // A lowered limit lowers what is left.
        b.refill(10, t0 + Duration::from_secs(3600));
        assert_eq!(b.level(), 10.0);
        // 6/min: one every 10 s.
        let mut b = Bucket::full(6, t0);
        b.take(6.0);
        assert_eq!(b.secs_until(1.0, 6), 10.0);
        assert_eq!(retry_secs(0.2), 1);
        assert_eq!(retry_secs(1.0), 1);
        assert_eq!(retry_secs(1.01), 2);
        assert_eq!(retry_secs(0.0), 1);
        assert_eq!(retry_secs(f64::INFINITY), 60);
    }

    #[test]
    fn requests_per_minute_burst_then_steady() {
        let rl = RateLimiter::new();
        let l = limits(Some(3), None, None);
        let t0 = Instant::now();
        for i in 0..3 {
            let p = rl.admit_at("a", &l, t0).unwrap();
            assert_eq!(p.snapshot.requests_remaining, Some(2 - i));
            assert_eq!(p.snapshot.requests_limit, Some(3));
        }
        let r = rl.admit_at("a", &l, t0).unwrap_err();
        assert_eq!(r.kind, LimitKind::RequestsPerMinute);
        assert_eq!(r.limit, 3);
        assert_eq!(
            r.retry_after_secs, 20,
            "3/min refills one request every 20 s"
        );
        assert_eq!(r.snapshot.requests_remaining, Some(0));
        // A rejection consumes nothing: 20 s later exactly one passes.
        assert!(rl.admit_at("a", &l, t0 + Duration::from_secs(19)).is_err());
        let r = rl
            .admit_at("a", &l, t0 + Duration::from_secs(19))
            .unwrap_err();
        assert_eq!(r.retry_after_secs, 1);
        assert!(rl.admit_at("a", &l, t0 + Duration::from_secs(20)).is_ok());
        assert!(rl.admit_at("a", &l, t0 + Duration::from_secs(20)).is_err());
        // Other tokens have their own bucket.
        assert!(rl.admit_at("b", &l, t0).is_ok());
    }

    #[test]
    fn tokens_per_minute_is_charged_at_the_end() {
        let rl = RateLimiter::new();
        let l = limits(None, Some(600), None);
        let t0 = Instant::now();
        let p = rl.admit_at("a", &l, t0).unwrap();
        assert_eq!(p.snapshot.tokens_remaining, Some(600));
        // A second request may start while the first runs (nothing charged yet).
        let p2 = rl.admit_at("a", &l, t0).unwrap();
        p.complete_at(500, t0);
        p2.complete_at(400, t0);
        // 600 - 900 = -300: blocked until the bucket is positive again (600/min = 10/s).
        let r = rl.admit_at("a", &l, t0).unwrap_err();
        assert_eq!(r.kind, LimitKind::TokensPerMinute);
        assert_eq!(r.limit, 600);
        assert_eq!(
            r.retry_after_secs, 31,
            "300 tokens at 10/s = 30 s, strictly above zero"
        );
        assert_eq!(r.snapshot.tokens_remaining, Some(0));
        assert!(rl.admit_at("a", &l, t0 + Duration::from_secs(30)).is_err());
        assert!(rl.admit_at("a", &l, t0 + Duration::from_secs(31)).is_ok());
    }

    #[test]
    fn concurrency_slot_is_held_until_the_permit_goes() {
        let rl = RateLimiter::new();
        let l = limits(None, None, Some(2));
        let t0 = Instant::now();
        let a = rl.admit_at("a", &l, t0).unwrap();
        let b = rl.admit_at("a", &l, t0).unwrap();
        assert_eq!(b.snapshot.in_flight, 2);
        let r = rl.admit_at("a", &l, t0).unwrap_err();
        assert_eq!(r.kind, LimitKind::Concurrency);
        assert_eq!(r.limit, 2);
        assert_eq!(r.retry_after_secs, 1);
        drop(a);
        let c = rl.admit_at("a", &l, t0).unwrap();
        b.complete_at(10, t0);
        drop(c);
        assert_eq!(rl.snapshot_at("a", &l, t0).in_flight, 0);
    }

    #[test]
    fn longest_wait_wins_and_nothing_is_consumed_on_rejection() {
        let rl = RateLimiter::new();
        // 1 request/min and 60 tokens/min: after one request of 120 tokens both block.
        let l = limits(Some(1), Some(60), None);
        let t0 = Instant::now();
        rl.admit_at("a", &l, t0).unwrap().complete_at(120, t0);
        let r = rl.admit_at("a", &l, t0).unwrap_err();
        // Requests: 60 s; tokens: 60 below zero at 1/s = 60 s + epsilon -> 61.
        assert_eq!(r.kind, LimitKind::TokensPerMinute);
        assert_eq!(r.retry_after_secs, 61);
        let s = rl.snapshot_at("a", &l, t0);
        assert_eq!(s.requests_remaining, Some(0));
        assert_eq!(s.in_flight, 0);
    }

    #[test]
    fn changed_limits_apply_to_existing_counters() {
        let rl = RateLimiter::new();
        let t0 = Instant::now();
        let wide = limits(Some(100), None, None);
        for _ in 0..10 {
            rl.admit_at("a", &wide, t0).unwrap();
        }
        // Lowered to 5/min: 90 left is capped to 5.
        let narrow = limits(Some(5), None, None);
        for _ in 0..5 {
            rl.admit_at("a", &narrow, t0).unwrap();
        }
        assert!(rl.admit_at("a", &narrow, t0).is_err());
        // Limit removed: unlimited again.
        let none = RateLimits::default();
        for _ in 0..50 {
            rl.admit_at("a", &none, t0).unwrap();
        }
        assert_eq!(rl.snapshot_at("a", &none, t0), Snapshot::default());
    }

    #[test]
    fn concurrent_admissions_never_exceed_the_cap() {
        let rl = RateLimiter::new();
        let l = limits(None, None, Some(3));
        let held = std::sync::Arc::new(Mutex::new(Vec::new()));
        let max_seen = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let rl = rl.clone();
                let held = held.clone();
                let max_seen = max_seen.clone();
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        if let Ok(p) = rl.admit("t", &l) {
                            max_seen.fetch_max(
                                p.snapshot.in_flight,
                                std::sync::atomic::Ordering::Relaxed,
                            );
                            held.lock().unwrap().push(p);
                            let mut h = held.lock().unwrap();
                            if h.len() > 1 {
                                h.remove(0);
                            }
                        }
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert!(max_seen.load(std::sync::atomic::Ordering::Relaxed) <= 3);
        held.lock().unwrap().clear();
        assert_eq!(rl.snapshot("t", &l).in_flight, 0);
    }

    fn store() -> (tempfile::TempDir, Arc<AnalyticsDb>) {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(AnalyticsDb::open(&dir.path().join("analytics.db")).unwrap());
        (dir, db)
    }

    const T0_MS: u64 = 1_800_000_000_000;

    #[test]
    fn a_restart_keeps_the_budget_and_an_expired_window_resets() {
        let (_dir, db) = store();
        let t0 = Instant::now();
        let l = limits(Some(3), Some(600), None);
        {
            let rl = RateLimiter::with_store(db.clone(), t0, T0_MS);
            for _ in 0..3 {
                rl.admit_at("a", &l, t0).unwrap().complete_at(150, t0);
            }
            // No explicit flush: the last state is written when the limiter goes away.
        }
        // 5 s later, a new process: 3 requests/min refill 1 per 20 s -> still empty; the
        // 450 tokens used are refilled by 50.
        let t1 = t0 + Duration::from_secs(5);
        let rl = RateLimiter::with_store(db.clone(), t1, T0_MS + 5_000);
        let r = rl.admit_at("a", &l, t1).unwrap_err();
        assert_eq!(r.kind, LimitKind::RequestsPerMinute);
        assert_eq!(r.retry_after_secs, 15);
        let s = rl.snapshot_at("a", &l, t1);
        assert_eq!(s.tokens_remaining, Some(200), "600 - 450 + 5 s * 10/s");
        // Another token starts full.
        assert_eq!(rl.snapshot_at("b", &l, t1).requests_remaining, Some(3));
        drop(rl);
        // An hour later every window has expired: full again.
        let t2 = t0 + Duration::from_secs(3600);
        let rl = RateLimiter::with_store(db, t2, T0_MS + 3_600_000);
        let s = rl.snapshot_at("a", &l, t2);
        assert_eq!(s.requests_remaining, Some(3));
        assert_eq!(s.tokens_remaining, Some(600));
    }

    #[test]
    fn a_stored_counter_follows_a_changed_limit() {
        let (_dir, db) = store();
        let t0 = Instant::now();
        {
            let rl = RateLimiter::with_store(db.clone(), t0, T0_MS);
            let l = limits(Some(100), None, None);
            for _ in 0..40 {
                rl.admit_at("a", &l, t0).unwrap();
            }
        }
        // The limit was lowered to 10 while the app was off: the stored 60 is capped.
        let rl = RateLimiter::with_store(db, t0, T0_MS);
        let s = rl.snapshot_at("a", &limits(Some(10), None, None), t0);
        assert_eq!(s.requests_remaining, Some(10));
    }

    #[test]
    fn two_processes_count_against_one_budget() {
        let (_dir, db) = store();
        let t0 = Instant::now();
        let l = limits(Some(10), None, None);
        // Two limiters on one database stand in for two processes.
        let a = RateLimiter::with_store(db.clone(), t0, T0_MS);
        let b = RateLimiter::with_store(db.clone(), t0, T0_MS);
        for _ in 0..4 {
            a.admit_at("t", &l, t0).unwrap();
        }
        for _ in 0..3 {
            b.admit_at("t", &l, t0).unwrap();
        }
        a.flush_at(t0).unwrap();
        b.flush_at(t0).unwrap();
        a.flush_at(t0).unwrap();
        // 10 - 4 - 3 on both sides.
        assert_eq!(a.snapshot_at("t", &l, t0).requests_remaining, Some(3));
        assert_eq!(b.snapshot_at("t", &l, t0).requests_remaining, Some(3));
        // The merged budget is enforced: 3 more pass in total, wherever they come from.
        for _ in 0..2 {
            a.admit_at("t", &l, t0).unwrap();
        }
        b.admit_at("t", &l, t0).unwrap();
        a.flush_at(t0).unwrap();
        b.flush_at(t0).unwrap();
        a.flush_at(t0).unwrap();
        assert!(a.admit_at("t", &l, t0).is_err());
        assert!(b.admit_at("t", &l, t0).is_err());
        // A single process does not drift: no foreign writer, its own level is written as is.
        let c = RateLimiter::with_store(db, t0, T0_MS);
        let t1 = t0 + Duration::from_secs(60);
        for i in 0..5u64 {
            let now = t1 + Duration::from_secs(i * 6);
            c.admit_at("solo", &l, now).unwrap();
            c.flush_at(now).unwrap();
        }
        // 5 taken, 24 s of refill at 10/min = 4: 9 left, nothing lost to the writes.
        let s = c.snapshot_at("solo", &l, t1 + Duration::from_secs(24));
        assert_eq!(s.requests_remaining, Some(9));
    }

    #[test]
    fn a_failed_write_keeps_what_was_used() {
        let (dir, db) = store();
        let path = dir.path().join("analytics.db");
        let t0 = Instant::now();
        let l = limits(Some(5), None, None);
        drop(db);
        // A newer schema makes the database read-only: writes fail.
        let ro = {
            let c = rusqlite::Connection::open(&path).unwrap();
            c.pragma_update(None, "user_version", crate::analytics::SCHEMA_VERSION + 1)
                .unwrap();
            Arc::new(AnalyticsDb::open(&path).unwrap())
        };
        assert!(ro.is_read_only());
        let broken = RateLimiter::with_store(ro, t0, T0_MS);
        broken.admit_at("a", &l, t0).unwrap();
        assert!(broken.flush_at(t0).is_err());
        // Still counted in memory, and still pending for the next try.
        assert_eq!(broken.snapshot_at("a", &l, t0).requests_remaining, Some(4));
        assert!(broken.flush_at(t0).is_err());
        assert_eq!(broken.snapshot_at("a", &l, t0).requests_remaining, Some(4));
    }
}
