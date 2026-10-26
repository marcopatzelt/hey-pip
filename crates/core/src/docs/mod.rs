//! Letters in: photo/PDF -> (SAM "sheet of paper" crop) -> Muse Spark JSON -> check -> memory -> rules.
//!
//! OWNER: agent "docs-cli" (crates/core/src/docs/**, crates/cli/**, fixtures/**, eval/**).

pub mod extract;
pub mod pipeline;

pub use extract::{AuInfo, AuKind, DocType, DocumentExtract, ExtractedDeadline, extract_schema, validate};
pub use pipeline::{DocError, DocPipeline, IngestResult};
