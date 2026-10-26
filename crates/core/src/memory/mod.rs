//! SQLite memory: profile, letters (+FTS5), deadlines, reminders, events, action log.
//!
//! OWNER: agent "memory-rules" (crates/core/src/memory/**, crates/core/src/rules/**, crates/core/src/clock.rs).

pub mod db;
pub mod demo;

pub use db::{ActionLog, Deadline, DeadlineStatus, DocumentRow, Event, Memory, NewDeadline, Reminder, default_db_path};
pub use demo::{DEMO_LETTER_FILES, seed_demo_profile};

/// Schema, run on every open (idempotent: CREATE ... IF NOT EXISTS).
pub const SCHEMA_SQL: &str = include_str!("schema.sql");
