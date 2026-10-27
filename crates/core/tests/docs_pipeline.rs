//! Document pipeline end to end against a mocked Meta API (no network, no cost).
//! OWNER: agent "docs-cli". Fixtures: fixtures/letters.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Local, NaiveDate, TimeZone};
use pip_core::clock::{Clock, FixedClock};
use pip_core::docs::{AuKind, DocError, DocPipeline, DocType};
use pip_core::i18n::Lang;
use pip_core::memory::Memory;
use pip_core::meta::MetaClient;
use pip_core::rules::RuleEngine;
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/letters").join(name)
}

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

struct Setup {
    server: MockServer,
    memory: Arc<Memory>,
    pipeline: DocPipeline,
}

async fn setup() -> Setup {
    let server = MockServer::start().await;
    let meta = Arc::new(MetaClient::new(server.uri(), "test-key"));
    let memory = Arc::new(Memory::open_in_memory().unwrap());
    let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(Local.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap()));
    let rules = Arc::new(RuleEngine::new(memory.clone(), clock));
    let pipeline = DocPipeline::new(meta, memory.clone(), rules);
    Setup { server, memory, pipeline }
}

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

fn spark_tool_answer(args: &Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "resp_1",
        "output": [
            {"type": "reasoning", "id": "rs_1", "summary": []},
            {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "save_letter",
             "arguments": args.to_string(), "status": "completed"}
        ],
        "usage": {"input_tokens": 3000, "output_tokens": 300}
    }))
}

/// SAM answer as server-sent events; the photo is sent as 1600x1200.
fn sam_answer(boxes: &str) -> ResponseTemplate {
    let delta = json!({"type": "response.output_text.delta", "delta": boxes});
    let done = json!({"type": "response.completed", "response": {"id": "resp_sam", "output": [], "usage": {"input_tokens": 1, "output_tokens": 1}}});
    let body =
        format!("event: response.output_text.delta\ndata: {delta}\n\nevent: response.completed\ndata: {done}\n\n");
    ResponseTemplate::new(200).insert_header("content-type", "text/event-stream").set_body_string(body)
}

async fn mount_spark(server: &MockServer, answer: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/responses"))
        .and(body_partial_json(json!({"model": "muse-spark-1.3"})))
        .respond_with(answer)
        .mount(server)
        .await;
}

async fn mount_sam(server: &MockServer, boxes: &str) {
    Mock::given(method("POST"))
        .and(path("/responses"))
        .and(body_partial_json(json!({"model": "sam-3.1"})))
        .respond_with(sam_answer(boxes))
        .mount(server)
        .await;
}

async fn bodies(server: &MockServer) -> Vec<Value> {
    server.received_requests().await.unwrap().iter().map(|r| serde_json::from_slice(&r.body).unwrap()).collect()
}

fn content_types(body: &Value) -> Vec<String> {
    body["input"][0]["content"].as_array().unwrap().iter().map(|c| c["type"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn photo_is_cropped_read_stored_and_deduplicated() {
    let s = setup().await;
    mount_sam(&s.server, "<0f>1<|box;x1=456;y1=84;x2=1240;y2=1136;w=1600;h=1200|><|mask;xyz|>\n").await;
    mount_spark(&s.server, spark_tool_answer(&au_args())).await;

    let r = s.pipeline.ingest(&fixture("au_erika_erst_foto.jpg"), Lang::De).await.unwrap();
    assert!(!r.duplicate);
    assert_eq!(r.extract.doc_type, DocType::AuBescheinigung);
    let au = r.extract.au.as_ref().unwrap();
    assert_eq!((au.from, au.until, au.first_or_follow), (d("2026-10-20"), d("2026-10-30"), AuKind::Erst));
    assert_eq!(r.deadlines.len(), 1, "{:?}", r.deadlines);
    assert_eq!(r.deadlines[0].rule_id.as_deref(), Some("au_lueckenlos"));
    assert_eq!(r.deadlines[0].due, d("2026-11-02"));
    assert!(r.bubble.contains("20.10.") && r.bubble.contains("30.10."), "{}", r.bubble);
    assert_eq!(s.memory.profile_get("insurer").unwrap().as_deref(), Some("Evergreen Health"));
    assert_eq!(s.memory.search_documents("arbeitsunfähig", 5).unwrap().len(), 1, "letter text is searchable");
    assert_eq!(s.memory.recent_events(5).unwrap().len(), 1);

    let reqs = bodies(&s.server).await;
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0]["model"], "sam-3.1");
    assert_eq!(reqs[0]["input"][0]["content"][0]["text"], "sheet of paper");
    assert_eq!(reqs[1]["model"], "muse-spark-1.3");
    assert_eq!(reqs[1]["reasoning"]["effort"], "medium");
    assert_eq!(content_types(&reqs[1]), ["input_text", "input_image"]);
    assert_eq!(reqs[1]["tools"][0]["name"], "save_letter");
    // The cropped sheet is narrower than the photo (portrait, not 4:3 landscape).
    let url = reqs[1]["input"][0]["content"][1]["image_url"].as_str().unwrap();
    let png = base64_decode(url.trim_start_matches("data:image/png;base64,"));
    let img = image::load_from_memory(&png).unwrap();
    assert!(img.height() > img.width(), "cropped to the sheet: {}x{}", img.width(), img.height());

    // Same file again: no new API call, same document.
    let again = s.pipeline.ingest(&fixture("au_erika_erst_foto.jpg"), Lang::De).await.unwrap();
    assert!(again.duplicate);
    assert_eq!(again.doc_id, r.doc_id);
    assert_eq!(again.deadlines.len(), 1);
    assert_eq!(bodies(&s.server).await.len(), 2);
}

#[tokio::test]
async fn pdf_goes_straight_to_spark_as_file() {
    let s = setup().await;
    let args = json!({
        "doc_type": "bescheid", "sender": "Stadt Beispielstadt", "recipient": "Erika Beispiel",
        "date": "14.10.2026", "reference": "50.2-WG-2026-04711",
        "summary_simple": "Du bekommst 96 Euro Wohngeld im Monat. Du kannst Widerspruch einlegen.",
        "requests": [], "deadlines": [], "au": null, "consequences_if_ignored": null, "confidence": 0.9,
        "letter_text": "Bescheid über Wohngeld"
    });
    mount_spark(&s.server, spark_tool_answer(&args)).await;
    let r = s.pipeline.ingest(&fixture("bescheid_wohngeld.pdf"), Lang::De).await.unwrap();
    assert_eq!(r.extract.doc_type, DocType::Bescheid);
    assert_eq!(r.extract.date, Some(d("2026-10-14")));
    let w: Vec<_> = r.deadlines.iter().filter(|d| d.rule_id.as_deref() == Some("widerspruch_bescheid")).collect();
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].due, d("2026-11-18"));
    assert!(r.bubble.starts_with("Du bekommst 96 Euro Wohngeld im Monat."), "{}", r.bubble);
    assert_eq!(s.memory.profile_get("insurer").unwrap(), None, "a city office is no health insurer");

    let reqs = bodies(&s.server).await;
    assert_eq!(reqs.len(), 1, "no SAM call for a PDF");
    assert_eq!(content_types(&reqs[0]), ["input_text", "input_file"]);
    assert!(
        reqs[0]["input"][0]["content"][1]["file_data"].as_str().unwrap().starts_with("data:application/pdf;base64,")
    );
}

/// Beispiel Air's delay notice: Spark gives the `flight` block, Rust knows EU 261 (deadline + tip), memory learns
/// the airline and its claim page from the e-mail text.
#[tokio::test]
async fn airline_notice_gives_a_claim_deadline_and_a_may_be_owed_tip() {
    let s = setup().await;
    s.memory.profile_set("lang", "en").unwrap();
    let args = json!({
        "doc_type": "brief", "sender": "Beispiel Air", "recipient": "Erika Beispiel",
        "date": "2026-10-04", "reference": "BX7Q2K", "person": "Erika Beispiel",
        "summary_simple": "Your flight BX 2041 on 3 Oct 2026 arrived 4 h 10 min late.",
        "requests": ["Claim on our page if you like."], "deadlines": [], "au": null,
        "flight": {"airline": "Beispiel Air", "flight_no": "BX 2041", "date": "2026-10-03", "from": "DUS", "to": "TFS",
            "delay_minutes": 250, "cancelled": false, "notice_days": null, "booking_ref": "BX7Q2K"},
        "consequences_if_ignored": null, "confidence": 0.93,
        "letter_text": "Beispiel Air. Your flight BX 2041 arrived 4 h 10 min late. To claim compensation, go to https://www.beispiel-air.example/claim"
    });
    mount_spark(&s.server, spark_tool_answer(&args)).await;
    let r = s.pipeline.ingest(&fixture("flight_delay_bx2041.pdf"), Lang::En).await.unwrap();
    let f = r.extract.flight.as_ref().expect("the flight block");
    assert_eq!((f.flight_no.as_str(), f.delay_minutes, f.date), ("BX 2041", Some(250), d("2026-10-03")));
    assert_eq!(r.deadlines.len(), 1, "{:?}", r.deadlines);
    assert_eq!(r.deadlines[0].rule_id.as_deref(), Some("eu261_claim"));
    assert_eq!(r.deadlines[0].due, d("2029-12-31"));
    assert_eq!(r.deadlines[0].title, "Claim EUR 400 from Beispiel Air for flight BX 2041 (3 Oct 2026)");
    let tip = r.tip.expect("a tip");
    assert_eq!(tip.id, "eu261_delay");
    assert!(tip.text.contains("may be owed EUR 400"), "{}", tip.text);
    assert!(!tip.text.to_lowercase().contains("earned"));
    assert_eq!(s.memory.profile_get("airline").unwrap().as_deref(), Some("Beispiel Air"));
    assert_eq!(
        s.memory.profile_get("airline_claim_form").unwrap().as_deref(),
        Some("https://www.beispiel-air.example/claim")
    );
    assert_eq!(s.memory.profile_get("versichertennummer").unwrap(), None);
    let body = &bodies(&s.server).await[0];
    let text = body.to_string();
    assert!(text.contains("flight: only when the letter or e-mail is an airline's notice"), "the read rule is sent");
}

/// The same notice without a flight block (an older model answer) or with a broken one: no claim, no error.
#[tokio::test]
async fn a_broken_flight_block_is_no_claim_and_no_error() {
    let s = setup().await;
    let args = json!({
        "doc_type": "brief", "sender": "Beispiel Air", "recipient": "Erika Beispiel", "date": "2026-10-04",
        "reference": "BX7Q2K", "summary_simple": "Your flight was late.", "requests": [], "deadlines": [], "au": null,
        "flight": {"airline": "Beispiel Air", "flight_no": "BX 2041", "date": "someday", "delay_minutes": 250},
        "consequences_if_ignored": null, "confidence": 0.8, "letter_text": "x"
    });
    mount_spark(&s.server, spark_tool_answer(&args)).await;
    let r = s.pipeline.ingest(&fixture("flight_delay_bx2041.pdf"), Lang::En).await.unwrap();
    assert!(r.extract.flight.is_none());
    assert!(r.deadlines.iter().all(|d| d.rule_id.as_deref() != Some("eu261_claim")));
    assert!(r.tip.is_none());
}

#[tokio::test]
async fn text_answer_without_tool_call_still_works_and_sam_miss_reads_whole_picture() {
    let s = setup().await;
    mount_sam(&s.server, "").await;
    let text = format!("Hier ist das Ergebnis:\n```json\n{}\n```", au_args());
    mount_spark(
        &s.server,
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "resp_2",
            "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}],
            "usage": {"input_tokens": 1, "output_tokens": 1}
        })),
    )
    .await;
    let r = s.pipeline.ingest(&fixture("au_erika_erst.png"), Lang::En).await.unwrap();
    assert_eq!(r.extract.au.unwrap().until, d("2026-10-30"));
    assert!(r.bubble.contains("30 Oct"), "{}", r.bubble);
}

#[tokio::test]
async fn unreadable_answer_and_unsupported_file_are_typed_errors() {
    let s = setup().await;
    mount_spark(
        &s.server,
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "resp_3",
            "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Ich kann das nicht lesen."}]}]
        })),
    )
    .await;
    let err = s.pipeline.ingest(&fixture("insurer_letter.pdf"), Lang::De).await.unwrap_err();
    assert!(matches!(err.downcast_ref::<DocError>(), Some(DocError::Unreadable)), "{err:#}");

    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("brief.docx");
    std::fs::write(&doc, b"PK\x03\x04 not a letter picture").unwrap();
    let err = s.pipeline.ingest(&doc, Lang::De).await.unwrap_err();
    assert!(matches!(err.downcast_ref::<DocError>(), Some(DocError::Unsupported)), "{err:#}");
    assert!(s.memory.recent_events(5).unwrap().is_empty(), "nothing stored on failure");
}

fn base64_decode(s: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).unwrap()
}
