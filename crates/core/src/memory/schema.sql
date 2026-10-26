-- Papierkram-Copilot memory. OWNER: agent "memory-rules".
-- SPEC §8 plus documents.text, actions_log.tokens, deadlines.why/source.
-- Migration 1 (PRAGMA user_version = 1). Pragmas (WAL, foreign_keys) are set in Memory::open.
-- All timestamps: RFC 3339 in UTC with seconds and "Z" (e.g. 2026-10-29T17:00:00Z), so text
-- comparison = time comparison across summer/winter time. Dates (due): YYYY-MM-DD.
-- documents_fts is contentless; rowid = documents.id.

CREATE TABLE IF NOT EXISTS profile (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS documents (
    id           INTEGER PRIMARY KEY,
    path         TEXT NOT NULL,
    sha256       TEXT NOT NULL UNIQUE,
    doc_type     TEXT NOT NULL,
    extract_json TEXT NOT NULL,
    text         TEXT NOT NULL DEFAULT '',
    created_at   TEXT NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
    text, summary, content='', tokenize='unicode61 remove_diacritics 2'
);

CREATE TABLE IF NOT EXISTS deadlines (
    id        INTEGER PRIMARY KEY,
    title     TEXT NOT NULL,
    due       TEXT NOT NULL,              -- YYYY-MM-DD
    rule_id   TEXT,
    doc_id    INTEGER REFERENCES documents(id),
    status    TEXT NOT NULL DEFAULT 'open', -- open | done | dismissed
    remind_at TEXT NOT NULL DEFAULT '[]', -- JSON array of RFC 3339 times (planned reminders)
    why       TEXT NOT NULL DEFAULT '',
    source    TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS reminders (
    id          INTEGER PRIMARY KEY,
    deadline_id INTEGER NOT NULL REFERENCES deadlines(id),
    at          TEXT NOT NULL,            -- RFC 3339 UTC
    fired_at    TEXT,
    answer      TEXT
);
CREATE INDEX IF NOT EXISTS reminders_due ON reminders(fired_at, at);
CREATE INDEX IF NOT EXISTS deadlines_doc ON deadlines(doc_id, rule_id, due);

CREATE TABLE IF NOT EXISTS events (
    id   INTEGER PRIMARY KEY,
    at   TEXT NOT NULL,
    kind TEXT NOT NULL,
    text TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS actions_log (
    id            INTEGER PRIMARY KEY,
    at            TEXT NOT NULL,
    tool          TEXT NOT NULL,
    args_redacted TEXT NOT NULL,
    risk          TEXT NOT NULL,          -- free | act | commit
    approved      INTEGER,                -- NULL = no approval needed
    result        TEXT NOT NULL,
    tokens        INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS actions_log_at ON actions_log(at);
