//! Ordered SQL migrations for `analytics.db`. Migration `i` (0-based) upgrades the schema
//! from `user_version = i` to `i + 1`. Never edit a released migration; append a new one.

pub(crate) const MIGRATIONS: &[&str] = &[V1, V2, V3, V4, V5, V6, V7];

/// Cost of a request at a BYOK cloud provider, USD (NULL for local models).
const V7: &str = r#"
ALTER TABLE requests ADD COLUMN cost_usd REAL;
CREATE INDEX requests_cost ON requests(started_at_ms DESC) WHERE cost_usd IS NOT NULL;
"#;

/// Tool-call diagnosis on `requests` (T4 · Araç çağrısı teşhisi, `gateway::tool_diag`). Added
/// with ALTER TABLE so existing rows keep their data (they read as "no tools").
///
/// * `tool_involved` — 1 when the request offered tools, carried tool results or the response
///   made a call.
/// * `tools_offered` / `tool_calls` — how many tools the request declared / calls the response
///   made (the measurement behind the per-model tool level).
/// * `tool_diag_category` — who is at fault: `runtime`, `context`, `model`, `client`; NULL for a
///   clean round.
/// * `tool_diag_code` — the exact finding (`call_left_in_content`, `repeated_call`...).
/// * `tool_diag_detail` — JSON evidence (tool name, excerpt, schema path, linked request...).
const V5: &str = r#"
ALTER TABLE requests ADD COLUMN tool_involved      INTEGER NOT NULL DEFAULT 0;
ALTER TABLE requests ADD COLUMN tools_offered      INTEGER NOT NULL DEFAULT 0;
ALTER TABLE requests ADD COLUMN tool_calls         INTEGER NOT NULL DEFAULT 0;
ALTER TABLE requests ADD COLUMN tool_diag_category TEXT;
ALTER TABLE requests ADD COLUMN tool_diag_code     TEXT;
ALTER TABLE requests ADD COLUMN tool_diag_detail   TEXT;
CREATE INDEX requests_tool ON requests(started_at_ms DESC) WHERE tool_involved = 1;
CREATE INDEX requests_tool_issue ON requests(started_at_ms DESC) WHERE tool_diag_category IS NOT NULL;
"#;

/// The tool log (`tools::ToolService`): one row per tool call, from any client (Playground,
/// CLI, Control API). Arguments are stored shortened (long texts such as file contents are
/// replaced by their length), never the result text.
const V4: &str = r#"
CREATE TABLE tool_calls (
    id              TEXT PRIMARY KEY NOT NULL,
    started_at_ms   INTEGER NOT NULL,
    day             INTEGER NOT NULL,
    client          TEXT NOT NULL,
    conversation_id TEXT,
    model           TEXT,
    tool            TEXT NOT NULL,
    class           TEXT NOT NULL,
    source          TEXT NOT NULL,
    args            TEXT NOT NULL,
    approved        INTEGER NOT NULL DEFAULT 0,
    error_code      TEXT,
    duration_ms     INTEGER NOT NULL,
    result_chars    INTEGER NOT NULL,
    truncated       INTEGER NOT NULL DEFAULT 0,
    network_target  TEXT
);
CREATE INDEX tool_calls_time ON tool_calls(started_at_ms DESC);
CREATE INDEX tool_calls_tool ON tool_calls(tool, started_at_ms DESC);
CREATE INDEX tool_calls_day ON tool_calls(day);
"#;

/// Per-token rate-limit counters that survive a restart (`gateway::limits`) and usage of the
/// llama-server ports that bypass the gateway (`analytics::direct`).
const V3: &str = r#"
CREATE TABLE rate_counters (
    token_id  TEXT NOT NULL,
    kind      TEXT NOT NULL,
    level     REAL NOT NULL,
    capacity  INTEGER NOT NULL,
    at_ms     INTEGER NOT NULL,
    rev       INTEGER NOT NULL,
    PRIMARY KEY (token_id, kind)
) WITHOUT ROWID;

CREATE TABLE direct_usage (
    minute_ms          INTEGER NOT NULL,
    day                INTEGER NOT NULL,
    server_id          TEXT NOT NULL,
    label              TEXT NOT NULL,
    prompt_tokens      INTEGER NOT NULL,
    completion_tokens  INTEGER NOT NULL,
    PRIMARY KEY (minute_ms, server_id)
) WITHOUT ROWID;
CREATE INDEX direct_usage_day ON direct_usage(day);

CREATE TABLE direct_daily (
    day                INTEGER NOT NULL,
    server_id          TEXT NOT NULL,
    label              TEXT NOT NULL,
    prompt_tokens      INTEGER NOT NULL,
    completion_tokens  INTEGER NOT NULL,
    PRIMARY KEY (day, server_id)
) WITHOUT ROWID;
"#;

/// Slot count and queue reason per request (slot recommendation from the queue history).
const V2: &str = r#"
ALTER TABLE requests ADD COLUMN slots INTEGER;
ALTER TABLE requests ADD COLUMN waited TEXT;
CREATE INDEX requests_waited ON requests(definition_id, started_at_ms) WHERE waited IS NOT NULL;
"#;

const V1: &str = r#"
CREATE TABLE requests (
    id                    TEXT PRIMARY KEY NOT NULL,
    started_at_ms         INTEGER NOT NULL,
    day                   INTEGER NOT NULL,
    endpoint              TEXT NOT NULL,
    model                 TEXT,
    alias                 TEXT,
    definition_id         TEXT,
    server_id             TEXT,
    client                TEXT,
    user_agent            TEXT,
    harness_client        TEXT,
    token_name            TEXT,
    stream                INTEGER NOT NULL DEFAULT 0,
    status                INTEGER NOT NULL,
    error_code            TEXT,
    oom                   INTEGER NOT NULL DEFAULT 0,
    cancelled             INTEGER NOT NULL DEFAULT 0,
    queue_wait_ms         INTEGER NOT NULL DEFAULT 0,
    load_ms               INTEGER,
    evicted_json          TEXT,
    ttft_ms               INTEGER,
    total_ms              INTEGER NOT NULL,
    prompt_tokens         INTEGER,
    completion_tokens     INTEGER,
    cached_tokens         INTEGER,
    prompt_ms             REAL,
    predicted_ms          REAL,
    prompt_per_second     REAL,
    predicted_per_second  REAL,
    draft_n               INTEGER,
    draft_accepted        INTEGER,
    finish_reason         TEXT,
    ctx_size              INTEGER,
    context_fill          REAL,
    vram_peak_bytes       INTEGER,
    ram_peak_bytes        INTEGER,
    backend               TEXT,
    compat_json           TEXT,
    content_captured      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX requests_time ON requests(started_at_ms DESC);
CREATE INDEX requests_alias ON requests(alias, started_at_ms DESC);
CREATE INDEX requests_day ON requests(day);

CREATE TABLE daily_rollup (
    day                 INTEGER NOT NULL,
    alias               TEXT NOT NULL,
    client              TEXT NOT NULL,
    endpoint            TEXT NOT NULL,
    requests            INTEGER NOT NULL,
    errors              INTEGER NOT NULL,
    ooms                INTEGER NOT NULL,
    cancelled           INTEGER NOT NULL,
    prompt_tokens       INTEGER NOT NULL,
    completion_tokens   INTEGER NOT NULL,
    cached_tokens       INTEGER NOT NULL,
    total_ms_sum        INTEGER NOT NULL,
    ttft_ms_sum         INTEGER NOT NULL,
    ttft_n              INTEGER NOT NULL,
    queue_ms_sum        INTEGER NOT NULL,
    load_n              INTEGER NOT NULL,
    load_ms_sum         INTEGER NOT NULL,
    gen_tokens_timed    REAL NOT NULL,
    gen_ms_sum          REAL NOT NULL,
    prompt_tokens_timed REAL NOT NULL,
    prompt_ms_sum       REAL NOT NULL,
    vram_peak_max       INTEGER,
    vram_peak_sum       INTEGER NOT NULL,
    vram_n              INTEGER NOT NULL,
    ram_peak_max        INTEGER,
    draft_n             INTEGER NOT NULL,
    draft_accepted      INTEGER NOT NULL,
    last_used_ms        INTEGER,
    definition_id       TEXT,
    PRIMARY KEY (day, alias, client, endpoint)
) WITHOUT ROWID;

CREATE TABLE request_content (
    request_id          TEXT PRIMARY KEY NOT NULL,
    captured_at_ms      INTEGER NOT NULL,
    expires_at_ms       INTEGER NOT NULL,
    request_body        TEXT NOT NULL,
    response_body       TEXT NOT NULL,
    request_truncated   INTEGER NOT NULL DEFAULT 0,
    response_truncated  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX request_content_expiry ON request_content(expires_at_ms);
"#;

// Client attribution of direct requests; old metrics-only rows keep their anonymous label.
const V6: &str = r#"
ALTER TABLE direct_usage RENAME TO direct_usage_old;
DROP INDEX direct_usage_day;
CREATE TABLE direct_usage (
 minute_ms INTEGER NOT NULL, day INTEGER NOT NULL, server_id TEXT NOT NULL,
 label TEXT NOT NULL, client TEXT NOT NULL DEFAULT 'direct_usage', requests INTEGER NOT NULL DEFAULT 0,
 prompt_tokens INTEGER NOT NULL, completion_tokens INTEGER NOT NULL,
 PRIMARY KEY(minute_ms, server_id, client)
) WITHOUT ROWID;
INSERT INTO direct_usage(minute_ms,day,server_id,label,prompt_tokens,completion_tokens)
 SELECT minute_ms,day,server_id,label,prompt_tokens,completion_tokens FROM direct_usage_old;
DROP TABLE direct_usage_old;
CREATE INDEX direct_usage_day ON direct_usage(day);
ALTER TABLE direct_daily RENAME TO direct_daily_old;
CREATE TABLE direct_daily (
 day INTEGER NOT NULL, server_id TEXT NOT NULL, label TEXT NOT NULL,
 client TEXT NOT NULL DEFAULT 'direct_usage', requests INTEGER NOT NULL DEFAULT 0,
 prompt_tokens INTEGER NOT NULL, completion_tokens INTEGER NOT NULL,
 PRIMARY KEY(day,server_id,client)
) WITHOUT ROWID;
INSERT INTO direct_daily(day,server_id,label,prompt_tokens,completion_tokens)
 SELECT day,server_id,label,prompt_tokens,completion_tokens FROM direct_daily_old;
DROP TABLE direct_daily_old;
"#;
