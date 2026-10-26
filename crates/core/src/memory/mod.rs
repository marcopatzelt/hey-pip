//! SQLite memory: profile, letters (+FTS5), deadlines, reminders, events, action log.
//!
//! OWNER: agent "memory-rules" (crates/core/src/memory/**, crates/core/src/rules/**, crates/core/src/clock.rs).

pub mod card;
pub mod db;
pub mod demo;
pub mod learn;
pub mod recall;

pub use card::{Tally, card_html, share_text, tally};
pub use db::{
    ActionLog, Deadline, DeadlineStatus, DocumentRow, Event, Knowledge, Memory, NewDeadline, Playbook, ProfileFact,
    Reminder, default_db_path,
};
pub use demo::{DEMO_LETTER_FILES, seed_demo_profile};
pub use learn::{
    Learned, PROFILE_TRUSTED_SEND, RunSummary, SENSITIVE_REFUSAL, is_protected_key, is_sensitive_key,
    is_sensitive_value, learn_from_letter, learn_from_run, set_fact, source_rank, trusted_sites, untrust_site,
};
pub use recall::{RecallQuery, recall};

/// Schema, run on every open (idempotent: CREATE ... IF NOT EXISTS).
pub const SCHEMA_SQL: &str = include_str!("schema.sql");
/// Migration 2: profile source/confidence/uses, playbooks (learning memory).
pub const SCHEMA_2_SQL: &str = include_str!("schema_2.sql");
