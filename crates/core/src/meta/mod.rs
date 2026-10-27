//! Meta Model API client (https://api.meta.ai/v1): Muse Spark responses, SSE,
//! SAM 3.1 boxes, Muse Image, speech to text. Private mode: open_weights.rs (Muse Glimmer /
//! Muse Spark Open behind an OpenAI-compatible endpoint).
//!
//! OWNER: agent "meta" (crates/core/src/meta/**, crates/core/src/config.rs).
//! Methods of `MetaClient` are split over the files: respond/respond_stream in
//! client.rs, sam_segment in sam.rs, images in images.rs, transcribe in asr.rs, web_search in search.rs (Muse Spark + the web search add-on).

pub mod asr;
pub mod client;
pub mod image;
pub mod images;
pub mod open_weights;
pub mod responses;
pub mod sam;
pub mod search;
pub mod sse;

pub use asr::{pcm_f32_to_wav16k, wav_from_pcm16};
pub use client::{MetaClient, MetaError, RetryPolicy, load_dotenv};
pub use image::{FittedImage, MAX_SIDE, fit_image};
pub use open_weights::{MODEL_GLIMMER, MODEL_SPARK_OPEN, OpenWeightsClient};
pub use responses::{
    Content, Effort, FunctionCall, InputItem, ResponseEvent, ResponseOutput, ResponseRequest, Role, SearchOutput,
    ToolDef, UrlCitation, Usage, data_url, parse_response, parse_search,
};
pub use sam::{SamBox, parse_sam_text};
pub use search::WebNote;

/// Model ids (verified 2026-10-26).
pub const MODEL_SPARK: &str = "muse-spark-1.3";
pub const MODEL_SAM: &str = "sam-3.1";
pub const MODEL_IMAGE: &str = "muse-image-1.0";
pub const MODEL_ASR: &str = "muse-voice-transcribe-1.0";
pub const DEFAULT_BASE: &str = "https://api.meta.ai/v1";
