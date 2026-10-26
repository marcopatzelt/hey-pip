//! The agent: screenshot -> Muse Spark plans (tool calls) -> SAM 3.1 grounds -> act -> check.
//! Approval gate before anything that sends, attaches, pays or deletes.
//!
//! OWNER: agent "agent" (crates/core/src/agent/**).

pub mod gate;
pub mod loop_;
pub mod prompt;
pub mod tools;
pub mod typing;

pub use gate::{ApprovalAnswer, ApprovalCard, COMMIT_WORDS, Gate, Risk};
pub use loop_::{
    API_MAX_SIDE, Agent, AgentOutcome, DISCOVER_STEPS, DISCOVER_TIME, PROFILE_LANG, PROFILE_SERVICES, Phase, RunState,
    StepResult, StopReason, Task, UrlOpener, system_open, task_from_words,
};
pub use tools::{DISCOVER_TOOLS, ScrollDir, Tool, tool_defs};

/// Re-export so callers need only `agent::CancellationToken`.
pub use tokio_util::sync::CancellationToken;

pub const MAX_STEPS: u32 = 25;
pub const MAX_RUN: std::time::Duration = std::time::Duration::from_secs(180);
pub const APPROVAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
