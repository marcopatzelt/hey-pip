-- Migration 2 (PRAGMA user_version = 2): learning memory (SPEC §8).
-- profile rows get where a fact came from and how often Pip used it. Source order (higher wins, a lower one
-- never replaces a higher one): user > typed (confirmed on a card) > letter[:doc_id] > model.
-- playbooks: how a task went on one site, so the next run is faster. Labels and profile KEYS only, never values.

ALTER TABLE profile ADD COLUMN source TEXT NOT NULL DEFAULT 'user';
ALTER TABLE profile ADD COLUMN confidence REAL NOT NULL DEFAULT 1.0;
ALTER TABLE profile ADD COLUMN last_used_at TEXT;
ALTER TABLE profile ADD COLUMN uses INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS playbooks (
    site            TEXT NOT NULL,
    task            TEXT NOT NULL,
    url             TEXT NOT NULL DEFAULT '',
    fields_json     TEXT NOT NULL DEFAULT '[]', -- [[label, profile_key], …]: labels and keys only, never values
    attach_label    TEXT,
    send_label      TEXT,
    steps           INTEGER NOT NULL DEFAULT 0,
    seconds         INTEGER NOT NULL DEFAULT 0,
    runs            INTEGER NOT NULL DEFAULT 1,
    failed_since_ok INTEGER NOT NULL DEFAULT 0,
    updated_at      TEXT NOT NULL,
    PRIMARY KEY (site, task)
);
