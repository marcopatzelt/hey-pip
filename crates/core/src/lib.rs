//! pip-core: everything that does not touch the operating system.
//! Meta API client, agent loop, memory (SQLite), deadline rules, document pipeline,
//! the event bus to the UI, UI strings. Builds and tests on Linux.
//!
//! OWNER: orchestrator (frozen). Module owners are named at the top of each module.

pub mod agent;
pub mod clock;
pub mod config;
pub mod docs;
pub mod events;
pub mod i18n;
pub mod memory;
pub mod meta;
pub mod platform;
pub mod rules;

pub use anyhow::Result;
