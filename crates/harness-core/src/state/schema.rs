//! Ordered SQL migrations for `state.db`. Migration `i` (0-based) upgrades the schema
//! from `user_version = i` to `i + 1`. Never edit a released migration; append a new one.

pub(crate) const MIGRATIONS: &[&str] = &[V1, V2, V3, V4, V5, V6, V7, V8, V9, V10, V11];

const V1: &str = r#"
CREATE TABLE folders (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    parent_id   TEXT REFERENCES folders(id) ON DELETE SET NULL,
    sort        INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);
CREATE INDEX folders_parent ON folders(parent_id, sort);

CREATE TABLE conversations (
    rid                          INTEGER PRIMARY KEY,
    id                           TEXT NOT NULL UNIQUE,
    title                        TEXT NOT NULL DEFAULT '',
    title_source                 TEXT NOT NULL DEFAULT 'none'
                                 CHECK (title_source IN ('none', 'fallback', 'auto', 'user')),
    folder_id                    TEXT REFERENCES folders(id) ON DELETE SET NULL,
    pinned                       INTEGER NOT NULL DEFAULT 0,
    favorite                     INTEGER NOT NULL DEFAULT 0,
    created_at                   INTEGER NOT NULL,
    updated_at                   INTEGER NOT NULL,
    last_opened_at               INTEGER,
    server_id                    TEXT,
    model_alias                  TEXT,
    preset_id                    TEXT,
    system_prompt                TEXT NOT NULL DEFAULT '',
    settings_json                TEXT NOT NULL DEFAULT '{}',
    metadata_json                TEXT NOT NULL DEFAULT '{}',
    current_leaf_id              TEXT,
    forked_from_conversation_id  TEXT,
    forked_from_message_id       TEXT
);
CREATE INDEX conversations_list ON conversations(pinned DESC, updated_at DESC);
CREATE INDEX conversations_folder ON conversations(folder_id, updated_at DESC);
CREATE INDEX conversations_opened ON conversations(last_opened_at DESC);

CREATE TABLE messages (
    id                 TEXT PRIMARY KEY NOT NULL,
    conversation_id    TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    parent_id          TEXT REFERENCES messages(id) ON DELETE CASCADE,
    parent_variant_id  TEXT,
    role               TEXT NOT NULL CHECK (role IN ('system', 'user', 'assistant', 'tool')),
    created_at         INTEGER NOT NULL,
    active_variant_id  TEXT
);
CREATE INDEX messages_conversation ON messages(conversation_id, created_at);
CREATE INDEX messages_parent ON messages(parent_id, created_at);

CREATE TABLE message_variants (
    rid                 INTEGER PRIMARY KEY,
    id                  TEXT NOT NULL UNIQUE,
    message_id          TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    content             TEXT NOT NULL DEFAULT '',
    reasoning           TEXT,
    tool_calls_json     TEXT,
    stats_json          TEXT,
    finish_reason       TEXT,
    stopped             INTEGER NOT NULL DEFAULT 0,
    tool_limit_reached  INTEGER NOT NULL DEFAULT 0,
    error               TEXT,
    model_alias         TEXT,
    created_at          INTEGER NOT NULL
);
CREATE INDEX message_variants_message ON message_variants(message_id, created_at);

CREATE TABLE attachments (
    id          TEXT PRIMARY KEY NOT NULL,
    message_id  TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,
    name        TEXT NOT NULL,
    mime        TEXT,
    size        INTEGER,
    sha256      TEXT,
    path        TEXT,
    created_at  INTEGER NOT NULL
);
CREATE INDEX attachments_message ON attachments(message_id);

CREATE TABLE app_state (
    key    TEXT PRIMARY KEY NOT NULL,
    value  TEXT NOT NULL
);

-- Titles use rowid = -conversations.rid, variant contents rowid = message_variants.rid.
CREATE VIRTUAL TABLE search_index USING fts5(
    conversation_id UNINDEXED,
    text,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER conversations_ai AFTER INSERT ON conversations BEGIN
    INSERT INTO search_index(rowid, conversation_id, text) VALUES (-NEW.rid, NEW.id, NEW.title);
END;
CREATE TRIGGER conversations_au AFTER UPDATE OF title ON conversations BEGIN
    UPDATE search_index SET text = NEW.title WHERE rowid = -NEW.rid;
END;
CREATE TRIGGER conversations_ad AFTER DELETE ON conversations BEGIN
    DELETE FROM search_index WHERE rowid = -OLD.rid;
END;

CREATE TRIGGER message_variants_ai AFTER INSERT ON message_variants BEGIN
    INSERT INTO search_index(rowid, conversation_id, text)
        SELECT NEW.rid, m.conversation_id, NEW.content FROM messages m WHERE m.id = NEW.message_id;
END;
CREATE TRIGGER message_variants_au AFTER UPDATE OF content ON message_variants BEGIN
    UPDATE search_index SET text = NEW.content WHERE rowid = NEW.rid;
END;
CREATE TRIGGER message_variants_ad AFTER DELETE ON message_variants BEGIN
    DELETE FROM search_index WHERE rowid = OLD.rid;
END;
"#;

/// v2: per-file library state (My Models): favorite, last use, when it arrived.
/// `model_id` is `LocalModel.id` (the launch path; shard 1 of split models).
const V2: &str = r#"
CREATE TABLE model_state (
    model_id            TEXT PRIMARY KEY NOT NULL,
    favorite            INTEGER NOT NULL DEFAULT 0,
    last_used_at        INTEGER,
    last_definition_id  TEXT,
    last_preset_id      TEXT,
    added_at            INTEGER
);
CREATE INDEX model_state_last_used ON model_state(last_used_at DESC);
"#;

/// v3: measured memory footprint of successful loads (`vram::Measurement` as JSON), keyed by
/// model file and the settings that decide it. `model_size` invalidates rows of a replaced file.
const V3: &str = r#"
CREATE TABLE vram_measurements (
    model_key     TEXT NOT NULL,
    settings_key  TEXT NOT NULL,
    model_size    INTEGER NOT NULL,
    measured_at   INTEGER NOT NULL,
    data_json     TEXT NOT NULL,
    PRIMARY KEY (model_key, settings_key)
);
CREATE INDEX vram_measurements_recent ON vram_measurements(model_key, measured_at DESC);
"#;

/// v4: free-form metadata of a variant (`meta_json`, a JSON object owned by the client). The
/// Playground split view stores which side of a comparison a variant is (`compare`: group,
/// side, model, preset...), so both answers of a split turn live as variants of one message.
const V4: &str = r#"
ALTER TABLE message_variants ADD COLUMN meta_json TEXT;
"#;

/// v5: documents attached to a conversation for local RAG (see `crate::rag`). A document keeps
/// its extracted text (`content`, pages marked `[Page N]`) and is split into `rag_chunks`;
/// each chunk's embedding is a little-endian f32 BLOB, L2-normalized, made by
/// `rag_documents.embed_model` (alias) with `embed_dims` dimensions (NULL until embedded).
/// `rag_fts` indexes chunk text for keyword search (external content, kept by triggers;
/// cascaded deletes from a conversation fire them too).
const V5: &str = r#"
CREATE TABLE rag_documents (
    rid              INTEGER PRIMARY KEY,
    id               TEXT NOT NULL UNIQUE,
    conversation_id  TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    name             TEXT NOT NULL,
    kind             TEXT NOT NULL DEFAULT 'text',
    source_path      TEXT,
    size             INTEGER NOT NULL DEFAULT 0,
    sha256           TEXT NOT NULL DEFAULT '',
    pages            INTEGER,
    content          TEXT NOT NULL DEFAULT '',
    chars            INTEGER NOT NULL DEFAULT 0,
    status           TEXT NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending', 'extracting', 'embedding', 'ready', 'failed', 'cancelled')),
    error            TEXT,
    mode             TEXT NOT NULL DEFAULT 'auto' CHECK (mode IN ('auto', 'full', 'retrieval')),
    embed_model      TEXT,
    embed_dims       INTEGER,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);
CREATE INDEX rag_documents_conversation ON rag_documents(conversation_id, created_at);

CREATE TABLE rag_chunks (
    rid          INTEGER PRIMARY KEY,
    document_id  TEXT NOT NULL REFERENCES rag_documents(id) ON DELETE CASCADE,
    seq          INTEGER NOT NULL,
    text         TEXT NOT NULL,
    page_start   INTEGER,
    page_end     INTEGER,
    heading      TEXT,
    embedding    BLOB,
    UNIQUE (document_id, seq)
);

CREATE VIRTUAL TABLE rag_fts USING fts5(
    text,
    content = 'rag_chunks',
    content_rowid = 'rid',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER rag_chunks_ai AFTER INSERT ON rag_chunks BEGIN
    INSERT INTO rag_fts(rowid, text) VALUES (NEW.rid, NEW.text);
END;
CREATE TRIGGER rag_chunks_ad AFTER DELETE ON rag_chunks BEGIN
    INSERT INTO rag_fts(rag_fts, rowid, text) VALUES ('delete', OLD.rid, OLD.text);
END;
CREATE TRIGGER rag_chunks_au AFTER UPDATE OF text ON rag_chunks BEGIN
    INSERT INTO rag_fts(rag_fts, rowid, text) VALUES ('delete', OLD.rid, OLD.text);
    INSERT INTO rag_fts(rowid, text) VALUES (NEW.rid, NEW.text);
END;
"#;

/// v6: stored Responses of the gateway (`store: true`, `previous_response_id`; see
/// `crate::gateway::responses`). One row per response: `input_json` is the request's own input
/// items (normalized to an array, not the prepended history), `response_json` the response
/// object as the client received it. `scope` is the caller (`token:<id>`, `harness` or
/// `anonymous`); a caller never sees another scope's rows. `last_used_at` moves when a
/// response is continued, and retention (age, count, bytes) drops the least recently used.
const V6: &str = r#"
CREATE TABLE gateway_responses (
    rid            INTEGER PRIMARY KEY,
    id             TEXT NOT NULL UNIQUE,
    scope          TEXT NOT NULL,
    previous_id    TEXT,
    model          TEXT NOT NULL DEFAULT '',
    created_at     INTEGER NOT NULL,
    last_used_at   INTEGER NOT NULL,
    input_json     TEXT NOT NULL,
    response_json  TEXT NOT NULL,
    bytes          INTEGER NOT NULL
);
CREATE INDEX gateway_responses_used ON gateway_responses(last_used_at);
"#;

/// v7: replayable SSE events for Responses created with both `background` and `stream`.
/// Ownership remains on `gateway_responses`; reads and writes join through that row's scope.
/// Cascading deletion keeps event retention exactly aligned with its response.
const V7: &str = r#"
CREATE TABLE gateway_response_events (
    response_id     TEXT NOT NULL REFERENCES gateway_responses(id) ON DELETE CASCADE,
    sequence_number INTEGER NOT NULL,
    event_json      TEXT NOT NULL,
    PRIMARY KEY (response_id, sequence_number)
);
CREATE INDEX gateway_response_events_response
    ON gateway_response_events(response_id, sequence_number);
"#;

/// v8: knowledge bases for RAG (see `crate::rag::kb`). A knowledge base is a named, reusable set
/// of documents. Its documents are ordinary `rag_documents` of a hidden conversation row whose id
/// is the knowledge base id (so extraction, chunks, vectors, keyword index and indexing jobs are
/// shared with conversation documents; the row is left out of the conversation list). Deleting
/// the knowledge base deletes that conversation and with it every document and chunk.
/// `embed_model` / `embed_dims` record the embedding model of the index: another model than the
/// one in use marks the index stale. `index_ms` is the total time spent indexing,
/// `rag_conversation_kbs` attaches knowledge bases to a conversation.
const V8: &str = r#"
CREATE TABLE rag_kbs (
    id           TEXT PRIMARY KEY NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    folder       TEXT,
    embed_model  TEXT,
    embed_dims   INTEGER,
    index_ms     INTEGER NOT NULL DEFAULT 0,
    indexed_at   INTEGER,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);
CREATE TABLE rag_conversation_kbs (
    conversation_id  TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    kb_id            TEXT NOT NULL REFERENCES rag_kbs(id) ON DELETE CASCADE,
    created_at       INTEGER NOT NULL,
    PRIMARY KEY (conversation_id, kb_id)
);
"#;

/// v9: projects. A project groups conversations (`conversations.project_id`; deleting a project
/// keeps its chats, unassigned) and the files made for it (`project_outputs`, metadata only: the
/// file stays where it was written). `settings_json` holds the folders tools and agents may use
/// and the attached tools, skills and MCP servers (see `crate::projects`).
const V9: &str = r#"
CREATE TABLE projects (
    rid            INTEGER PRIMARY KEY,
    id             TEXT NOT NULL UNIQUE,
    name           TEXT NOT NULL,
    description    TEXT NOT NULL DEFAULT '',
    folder         TEXT,
    settings_json  TEXT NOT NULL DEFAULT '{}',
    archived       INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);
ALTER TABLE conversations ADD COLUMN project_id TEXT REFERENCES projects(id) ON DELETE SET NULL;
CREATE INDEX conversations_project ON conversations(project_id, updated_at);

CREATE TABLE project_outputs (
    rid              INTEGER PRIMARY KEY,
    id               TEXT NOT NULL UNIQUE,
    project_id       TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path             TEXT NOT NULL,
    kind             TEXT NOT NULL DEFAULT 'file',
    source           TEXT NOT NULL DEFAULT 'manual',
    model            TEXT,
    conversation_id  TEXT REFERENCES conversations(id) ON DELETE SET NULL,
    agent_task_id    TEXT,
    tool             TEXT,
    size             INTEGER NOT NULL DEFAULT 0,
    created_at       INTEGER NOT NULL,
    UNIQUE (project_id, path)
);
CREATE INDEX project_outputs_project ON project_outputs(project_id, created_at);
"#;

const V10: &str = r#"
-- OCR for scanned PDFs: the knowledge base's OCR model (alias; NULL = none) and the pages of a
-- document that were recognized with it (comma separated page numbers; NULL = none).
ALTER TABLE rag_kbs ADD COLUMN ocr_model TEXT;
ALTER TABLE rag_documents ADD COLUMN ocr_pages TEXT;
"#;

/// v11: task runs (Projects: agent tasks and analysis sessions that can pause, step back and
/// resume). Everything needed to continue lives here, never live RAM state: the run (goal,
/// policy, file references, token/cost tally, a heartbeat that tells a crashed run from a live
/// one), its steps with their outputs, a checkpoint after every finished step and an event log
/// (fallback reasons, retries, recovery). See `crate::taskflow`.
const V11: &str = r#"
CREATE TABLE task_runs (
    rid            INTEGER PRIMARY KEY,
    id             TEXT NOT NULL UNIQUE,
    project_id     TEXT REFERENCES projects(id) ON DELETE CASCADE,
    kind           TEXT NOT NULL DEFAULT 'agent_task',
    title          TEXT NOT NULL,
    goal           TEXT NOT NULL DEFAULT '',
    status         TEXT NOT NULL DEFAULT 'draft',
    mode           TEXT NOT NULL DEFAULT 'auto',
    policy_json    TEXT NOT NULL DEFAULT '{}',
    file_refs_json TEXT NOT NULL DEFAULT '[]',
    tokens_in      INTEGER NOT NULL DEFAULT 0,
    tokens_out     INTEGER NOT NULL DEFAULT 0,
    cost_micro     INTEGER NOT NULL DEFAULT 0,
    last_model     TEXT,
    error          TEXT,
    imported_from  TEXT,
    heartbeat_at   INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    finished_at    INTEGER
);
CREATE INDEX task_runs_project ON task_runs(project_id, updated_at);

CREATE TABLE task_steps (
    rid             INTEGER PRIMARY KEY,
    id              TEXT NOT NULL UNIQUE,
    run_id          TEXT NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
    idx             INTEGER NOT NULL,
    title           TEXT NOT NULL,
    prompt          TEXT NOT NULL,
    gate            INTEGER NOT NULL DEFAULT 0,
    approval        TEXT NOT NULL DEFAULT 'none',
    status          TEXT NOT NULL DEFAULT 'pending',
    output          TEXT NOT NULL DEFAULT '',
    model           TEXT,
    fallback_reason TEXT,
    attempts        INTEGER NOT NULL DEFAULT 0,
    tokens_in       INTEGER NOT NULL DEFAULT 0,
    tokens_out      INTEGER NOT NULL DEFAULT 0,
    cost_micro      INTEGER NOT NULL DEFAULT 0,
    error           TEXT,
    started_at      INTEGER,
    finished_at     INTEGER,
    UNIQUE (run_id, idx)
);

CREATE TABLE task_checkpoints (
    rid          INTEGER PRIMARY KEY,
    id           TEXT NOT NULL UNIQUE,
    run_id       TEXT NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
    step_idx     INTEGER NOT NULL,
    label        TEXT NOT NULL DEFAULT '',
    model        TEXT,
    created_at   INTEGER NOT NULL
);
CREATE INDEX task_checkpoints_run ON task_checkpoints(run_id, step_idx);

CREATE TABLE task_events (
    rid       INTEGER PRIMARY KEY,
    run_id    TEXT NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
    step_idx  INTEGER,
    at        INTEGER NOT NULL,
    kind      TEXT NOT NULL,
    message   TEXT NOT NULL DEFAULT ''
);
CREATE INDEX task_events_run ON task_events(run_id, rid);
"#;
