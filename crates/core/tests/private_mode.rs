//! Private mode against two mocked servers (no network, no cost): `cloud` behind MetaClient
//! (must see NOTHING) and `local` behind OpenWeightsClient (Muse Glimmer / Muse Spark Open,
//! OpenAI-compatible /chat/completions). OWNER: agent "docs-cli". Fixtures: fixtures/letters.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Local, NaiveDate, TimeZone};
use pip_core::clock::{Clock, FixedClock};
use pip_core::docs::{DocError, DocPipeline};
use pip_core::i18n::{Lang, t};
use pip_core::memory::Memory;
use pip_core::meta::{MODEL_GLIMMER, MetaClient, MetaError, OpenWeightsClient};
use pip_core::rules::RuleEngine;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/letters").join(name)
}

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

struct Setup {
    cloud: MockServer,
    local: MockServer,
    memory: Arc<Memory>,
    pipeline: DocPipeline,
}

async fn setup(key: Option<&str>) -> Setup {
    let cloud = MockServer::start().await;
    let local = MockServer::start().await;
    let meta = Arc::new(MetaClient::new(cloud.uri(), "cloud-key"));
    let private =
        Arc::new(OpenWeightsClient::new(format!("{}/v1/", local.uri()), key.map(str::to_string), MODEL_GLIMMER));
    let memory = Arc::new(Memory::open_in_memory().unwrap());
    let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(Local.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap()));
    let rules = Arc::new(RuleEngine::new(memory.clone(), clock));
    let pipeline = DocPipeline::new(meta, memory.clone(), rules).with_private(private);
    Setup { cloud, local, memory, pipeline }
}

/// The same sick-note answer as docs_pipeline.rs (`au_args`).
fn au_args() -> Value {
    json!({
        "doc_type": "au_bescheinigung",
        "sender": "Dr. med. Max Beispiel",
        "recipient": "Evergreen Health",
        "date": "2026-10-20",
        "reference": "G123456789",
        "summary_simple": "Du bist vom 20.10. bis 30.10.2026 krankgeschrieben.",
        "requests": [],
        "deadlines": [],
        "au": {"from": "2026-10-20", "until": "30.10.2026", "first_or_follow": "erst", "issued": "2026-10-20"},
        "consequences_if_ignored": null,
        "confidence": 0.95,
        "letter_text": "Arbeitsunfähigkeitsbescheinigung Beispiel, Erika arbeitsunfähig seit 20.10.2026"
    })
}

/// A local model that wraps its JSON in a code fence (common with small models).
fn chat_answer(args: &Value) -> ResponseTemplate {
    let content = format!("```json\n{}\n```", serde_json::to_string_pretty(args).unwrap());
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "chatcmpl-1",
        "object": "chat.completion",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": content}, "finish_reason": "stop"}]
    }))
}

async fn mount_local(server: &MockServer, answer: ResponseTemplate) {
    Mock::given(method("POST")).and(path("/v1/chat/completions")).respond_with(answer).mount(server).await;
}

#[tokio::test]
async fn photo_is_read_locally_and_cloud_sees_nothing() {
    let s = setup(None).await;
    assert!(s.pipeline.is_private());
    mount_local(&s.local, chat_answer(&au_args())).await;

    let r = s.pipeline.ingest(&fixture("au_erika_erst_foto.jpg"), Lang::En).await.unwrap();
    let au = r.extract.au.as_ref().expect("sick note");
    assert_eq!(au.until, d("2026-10-30"));
    assert!(r.private && !r.duplicate);
    assert!(r.bubble.ends_with(t("doc_private_note", Lang::En)), "{}", r.bubble);
    assert!(r.bubble.starts_with("Sick note from 20 Oct to 30 Oct"), "{}", r.bubble);

    // Nothing went to the cloud: no SAM crop, no Muse Spark.
    assert!(s.cloud.received_requests().await.unwrap().is_empty(), "the cloud API must not see the letter");

    let reqs = s.local.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    assert!(reqs[0].headers.get("authorization").is_none(), "no key, no Authorization header");
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["model"], "muse-glimmer");
    assert_eq!(body["temperature"], 0);
    assert_eq!(body["stream"], false);
    assert_eq!(body["messages"][0]["role"], "system");
    assert!(body["messages"][0]["content"].as_str().unwrap().contains("JSON"));
    let user = &body["messages"][1];
    assert_eq!(user["role"], "user");
    let parts = user["content"].as_array().unwrap();
    assert_eq!(parts[0]["text"], "Read this letter.");
    let url = parts[1]["image_url"]["url"].as_str().unwrap();
    assert!(url.starts_with("data:image/png;base64,"), "resized PNG, not the original JPEG");

    // The rest of the pipeline is unchanged: stored, learned, event written.
    let events = s.memory.recent_events(5).unwrap();
    assert!(!events.is_empty());
    assert!(s.memory.document_by_sha(&sha_of("au_erika_erst_foto.jpg")).unwrap().is_some());

    // Same photo again: known, still nothing to the cloud and no second local call.
    let again = s.pipeline.ingest(&fixture("au_erika_erst_foto.jpg"), Lang::En).await.unwrap();
    assert!(again.duplicate && again.private);
    assert_eq!(s.local.received_requests().await.unwrap().len(), 1);
    assert!(s.cloud.received_requests().await.unwrap().is_empty());
}

fn sha_of(name: &str) -> String {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(fixture(name)).unwrap();
    Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[tokio::test]
async fn pdf_in_private_mode_is_refused_before_any_call() {
    let s = setup(None).await;
    mount_local(&s.local, chat_answer(&au_args())).await;
    let err = s.pipeline.ingest(&fixture("insurer_letter.pdf"), Lang::En).await.unwrap_err();
    let doc = err.downcast_ref::<DocError>();
    assert!(matches!(doc, Some(DocError::PrivatePdf)), "{err:#}");
    assert_eq!(
        doc.unwrap().user_text(Lang::En),
        "In private mode I can read photos and scans. For a PDF, please take a picture of the letter, or turn private mode off."
    );
    assert!(!doc.unwrap().user_text(Lang::De).contains("doc_"));
    assert!(s.cloud.received_requests().await.unwrap().is_empty());
    assert!(s.local.received_requests().await.unwrap().is_empty());
    assert!(s.memory.recent_events(5).unwrap().is_empty());
}

#[tokio::test]
async fn bearer_header_only_with_a_key() {
    let s = setup(Some("k-test")).await;
    mount_local(&s.local, chat_answer(&au_args())).await;
    s.pipeline.ingest(&fixture("au_erika_erst_foto.jpg"), Lang::En).await.unwrap();
    let reqs = s.local.received_requests().await.unwrap();
    assert_eq!(reqs[0].headers.get("authorization").unwrap().to_str().unwrap(), "Bearer k-test");
    assert!(s.cloud.received_requests().await.unwrap().is_empty());

    // Direct call without a key: no header at all.
    let local = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(chat_answer(&json!({})))
        .mount(&local)
        .await;
    let c = OpenWeightsClient::new(format!("{}/v1", local.uri()), None, "m");
    let text = c.chat_json("sys", "hi", None).await.unwrap();
    assert!(text.contains("```json"));
    let reqs = local.received_requests().await.unwrap();
    assert!(reqs[0].headers.get("authorization").is_none());
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["messages"][1]["content"].as_array().unwrap().len(), 1, "no image part without an image");
}

#[tokio::test]
async fn errors_are_friendly() {
    let s = setup(Some("k-very-secret-key")).await;
    mount_local(
        &s.local,
        ResponseTemplate::new(500).set_body_json(json!({"error": {"message": "bad key k-very-secret-key"}})),
    )
    .await;
    let err = s.pipeline.ingest(&fixture("au_erika_erst_foto.jpg"), Lang::En).await.unwrap_err();
    let meta = MetaError::find(&err).expect("a MetaError for the UI's friendly sentence");
    assert!(matches!(meta, MetaError::Http { status: 500, .. }), "{meta:?}");
    assert!(!format!("{err:#} {err:?}").contains("k-very-secret-key"), "key leaked: {err:#}");
    assert!(s.cloud.received_requests().await.unwrap().is_empty());

    // An answer that is not the letter schema: the friendly "take a sharper photo".
    let s = setup(None).await;
    let answer = ResponseTemplate::new(200)
        .set_body_json(json!({"choices": [{"message": {"role": "assistant", "content": "I cannot read this."}}]}));
    mount_local(&s.local, answer).await;
    let err = s.pipeline.ingest(&fixture("au_erika_erst_foto.jpg"), Lang::En).await.unwrap_err();
    assert!(matches!(err.downcast_ref::<DocError>(), Some(DocError::Unreadable)), "{err:#}");

    // Nothing listening: a network error, also a MetaError.
    let c = OpenWeightsClient::new("http://127.0.0.1:1/v1", None, MODEL_GLIMMER);
    let err = c.chat_json("sys", "hi", None).await.unwrap_err();
    assert!(matches!(MetaError::find(&err), Some(MetaError::Network(_))), "{err:#}");

    // Through the pipeline the same case is "my private model isn't answering", never
    // "check the Wi-Fi": the DocError comes first in the chain, the MetaError stays behind it.
    let s = setup(None).await;
    let dead = Arc::new(OpenWeightsClient::new("http://127.0.0.1:1/v1", None, MODEL_GLIMMER));
    let meta = Arc::new(MetaClient::new(s.cloud.uri(), "cloud-key"));
    let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(Local.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap()));
    let rules = Arc::new(RuleEngine::new(s.memory.clone(), clock));
    let pipeline = DocPipeline::new(meta, s.memory.clone(), rules).with_private(dead);
    let err = pipeline.ingest(&fixture("au_erika_erst_foto.jpg"), Lang::En).await.unwrap_err();
    let first = err.chain().next().unwrap().downcast_ref::<DocError>();
    assert!(matches!(first, Some(DocError::PrivateOffline(_))), "{err:#}");
    assert_eq!(
        first.unwrap().user_text(Lang::En),
        "My private reading model isn't answering. Please start it, or turn private mode off."
    );
    assert!(matches!(MetaError::find(&err), Some(MetaError::Network(_))), "{err:#}");
    assert!(s.cloud.received_requests().await.unwrap().is_empty());
    assert!(s.memory.recent_events(5).unwrap().is_empty());
}

/// Live: reads the demo photo through the real endpoint in PRIVATE_MODEL_BASE (llama.cpp or Baseten).
#[tokio::test]
#[ignore = "needs PRIVATE_MODEL_BASE"]
async fn live_private_read() {
    pip_core::meta::load_dotenv();
    let cfg = pip_core::config::Config::from_lookup(|k| std::env::var(k).ok());
    let Some(local) = OpenWeightsClient::from_config(&cfg) else {
        eprintln!("PRIVATE_MODEL_BASE not set (or PIP_PRIVATE=0): skipped");
        return;
    };
    let memory = Arc::new(Memory::open_in_memory().unwrap());
    let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(Local.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap()));
    let rules = Arc::new(RuleEngine::new(memory.clone(), clock));
    // The cloud client points nowhere: private mode must never need it.
    let meta = Arc::new(MetaClient::new("http://127.0.0.1:9", "unused"));
    let pipeline = DocPipeline::new(meta, memory, rules).with_private(Arc::new(local));
    let started = std::time::Instant::now();
    let r = pipeline.ingest(&fixture("au_erika_erst_foto.jpg"), Lang::En).await.unwrap();
    println!("{:.1} s: {}", started.elapsed().as_secs_f32(), r.bubble);
    assert_eq!(r.extract.au.map(|a| a.until), Some(d("2026-10-30")));
}
