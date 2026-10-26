//! MetaClient against a local mock server (no network, no key). OWNER: agent "meta".

use std::time::Duration;

use pip_core::i18n::Lang;
use pip_core::meta::{
    Content, Effort, InputItem, MetaClient, MetaError, ResponseEvent, ResponseRequest, RetryPolicy, ToolDef,
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn client(server: &MockServer) -> MetaClient {
    MetaClient::new(format!("{}/v1", server.uri()), "test-key").with_retry(RetryPolicy {
        max_attempts: 3,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
    })
}

fn tiny_png(w: u32, h: u32) -> Vec<u8> {
    let mut c = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::new(w, h)).write_to(&mut c, image::ImageFormat::Png).unwrap();
    c.into_inner()
}

fn answer(id: &str, out: Value) -> Value {
    json!({"id": id, "object": "response", "status": "completed", "error": null, "output": out,
           "usage": {"input_tokens": 100, "output_tokens": 7}})
}

#[tokio::test]
async fn respond_sends_auth_and_parses_calls() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(header("authorization", "Bearer test-key"))
        .and(body_partial_json(
            json!({"model":"muse-spark-1.3","stream":false,"tool_choice":"auto","reasoning":{"effort":"low"}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(answer(
            "resp_1",
            json!([{"type":"function_call","call_id":"c1","name":"say","arguments":"{\"text\":\"Hallo\"}"}]),
        )))
        .expect(2)
        .mount(&server)
        .await;
    let c = client(&server);
    let mut req = ResponseRequest::new("muse-spark-1.3", "x", vec![InputItem::user(vec![Content::text("hi")])]);
    req.tools.push(ToolDef { name: "say".into(), description: "d".into(), parameters: json!({"type":"object"}) });
    let out = c.respond(&req).await.unwrap();
    assert_eq!(out.calls[0].arguments["text"], "Hallo");
    assert_eq!(c.usage_total().input_tokens, 100);
    // clones share the counter
    let c2 = c.clone();
    c2.respond(&req).await.unwrap();
    assert_eq!(c.usage_total().output_tokens, 14);
}

#[tokio::test]
async fn retries_503_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(answer(
            "resp_ok",
            json!([{"type":"message","content":[{"type":"output_text","text":"ok"}]}]),
        )))
        .mount(&server)
        .await;
    let out = client(&server).respond(&ResponseRequest::new("m", "", vec![])).await.unwrap();
    assert_eq!(out.text, "ok");
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn gives_up_after_max_attempts_and_does_not_retry_400() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/responses"))
        .and(body_partial_json(json!({"model":"busy"})))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .mount(&server)
        .await;
    Mock::given(path("/v1/responses"))
        .and(body_partial_json(json!({"model":"bad"})))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error":{"message":"effort none not allowed"}})))
        .mount(&server)
        .await;
    let c = client(&server);
    let err = c.respond(&ResponseRequest::new("busy", "", vec![])).await.unwrap_err();
    assert!(matches!(MetaError::find(&err), Some(MetaError::Http { status: 429, .. })));
    assert!(MetaError::find(&err).unwrap().is_retryable());
    assert_eq!(server.received_requests().await.unwrap().len(), 3);

    let err = c.respond(&ResponseRequest::new("bad", "", vec![])).await.unwrap_err();
    let me = MetaError::find(&err).unwrap();
    assert!(!me.is_retryable());
    assert!(err.to_string().contains("effort none not allowed"));
    assert!(!format!("{err:?}").contains("test-key"));
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[tokio::test]
async fn network_error_is_retryable() {
    // Nothing listens on port 9.
    let c = MetaClient::new("http://127.0.0.1:9/v1", "k").with_retry(RetryPolicy {
        max_attempts: 2,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(1),
    });
    let err = c.respond(&ResponseRequest::new("m", "", vec![])).await.unwrap_err();
    assert!(matches!(MetaError::find(&err), Some(e @ MetaError::Network(_)) if e.is_retryable()));
}

const SSE: &str = "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"r\"}}\n\n\
event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Ich \"}\n\n\
event: response.output_text.delta\r\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"schaue \\u00fcber den Brief.\"}\r\n\r\n\
event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"c9\",\"name\":\"done\",\"arguments\":\"{\\\"summary\\\":\\\"ok\\\"}\"}}\n\n\
event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_s\",\"output\":[],\"usage\":{\"input_tokens\":5,\"output_tokens\":3}}}\n\n\
data: [DONE]\n\n";

#[tokio::test]
async fn stream_yields_deltas_call_completed() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/responses"))
        .and(body_partial_json(json!({"stream":true})))
        .respond_with(
            ResponseTemplate::new(200).insert_header("content-type", "text/event-stream").set_body_string(SSE),
        )
        .mount(&server)
        .await;
    let c = client(&server);
    let mut req = ResponseRequest::new("m", "i", vec![]);
    req.effort = Effort::Medium;
    let events: Vec<_> = c.respond_stream(&req).collect().await;
    let events: Vec<ResponseEvent> = events.into_iter().map(Result::unwrap).collect();
    let text: String = events
        .iter()
        .filter_map(|e| if let ResponseEvent::TextDelta(t) = e { Some(t.as_str()) } else { None })
        .collect();
    assert_eq!(text, "Ich schaue über den Brief.");
    assert!(events.iter().any(|e| matches!(e, ResponseEvent::Call(c) if c.call_id == "c9")));
    assert!(matches!(events.last(), Some(ResponseEvent::Completed(o)) if o.id == "resp_s"));
    assert_eq!(c.usage_total().output_tokens, 3);
}

#[tokio::test]
async fn stream_http_error_is_item_error() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/responses")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
    let events: Vec<_> = client(&server).respond_stream(&ResponseRequest::new("m", "", vec![])).collect().await;
    assert_eq!(events.len(), 1);
    assert!(events[0].is_err());
}

#[tokio::test]
async fn sam_segment_parses_and_rescales() {
    let server = MockServer::start().await;
    let line = "<0f>0<|box;x1=520;y1=380;x2=700;y2=440;w=800;h=500|><|mask;x=0;y=0;data=61,181,~!!!9I|>";
    let body = format!(
        "data: {}\n\ndata: {}\n\ndata: {}\n\n",
        json!({"type":"response.output_text.delta","delta": &line[..30]}),
        json!({"type":"response.output_text.delta","delta": &line[30..]}),
        json!({"type":"response.completed","response":{"id":"r_sam","error":null,"output":[]}}),
    );
    Mock::given(path("/v1/responses"))
        .and(body_partial_json(json!({"model":"sam-3.1","stream":true})))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;
    // We send 1600x1000; the mock claims 800x500 -> boxes doubled.
    let boxes = client(&server).sam_segment(&tiny_png(1600, 1000), "button").await.unwrap();
    assert_eq!(boxes.len(), 1);
    assert_eq!((boxes[0].x1, boxes[0].y1, boxes[0].x2, boxes[0].y2), (1040, 760, 1400, 880));
    let req: Value = server.received_requests().await.unwrap()[0].body_json().unwrap();
    assert_eq!(req["input"][0]["content"][0], json!({"type":"input_text","text":"button"}));
    assert!(req["input"][0]["content"][1]["image_url"].as_str().unwrap().starts_with("data:image/png;base64,"));
    assert!(req.get("reasoning").is_none());
}

#[tokio::test]
async fn transcribe_multipart() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/asr/transcribe"))
        .and(header("accept", "application/json"))
        .respond_with(move |req: &Request| {
            let ct = req.headers.get("content-type").unwrap().to_str().unwrap().to_string();
            let body = String::from_utf8_lossy(&req.body).to_string();
            let ok = ct.starts_with("multipart/form-data")
                && body.contains("name=\"request\"")
                && body.contains("\"languageBias\":[\"German\"]")
                && body.contains("name=\"audio\"")
                && body.contains("audio/wav")
                && body.contains("RIFF");
            if ok {
                ResponseTemplate::new(200)
                    .set_body_json(json!({"sessionId":"s","transcript":" Hallo Pip ","audioDurationMs":900,"turns":[]}))
            } else {
                ResponseTemplate::new(400)
            }
        })
        .mount(&server)
        .await;
    let wav = pip_core::meta::wav_from_pcm16(&[0i16; 1600], 16_000).unwrap();
    let t = client(&server).transcribe(wav, Lang::De).await.unwrap();
    assert_eq!(t, "Hallo Pip");
}

#[tokio::test]
async fn generate_and_edit_image() {
    use base64::Engine;
    let server = MockServer::start().await;
    let png = tiny_png(4, 4);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    Mock::given(path("/v1/images/generations"))
        .and(body_partial_json(json!({"model":"muse-image-1.0","output_format":"png","size":"1024x1024"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"b64_json": b64}]})))
        .mount(&server)
        .await;
    Mock::given(path("/v1/images/edits"))
        .and(body_partial_json(json!({"model":"muse-image-1.0","output_format":"png"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"b64_json": b64}]})))
        .mount(&server)
        .await;
    let c = client(&server);
    assert_eq!(c.generate_image("a drop", "1024x1024").await.unwrap(), png);
    assert_eq!(c.edit_image("wave", &png).await.unwrap(), png);
    let reqs = server.received_requests().await.unwrap();
    let edit: Value = reqs[1].body_json().unwrap();
    assert!(edit["images"][0]["image_url"].as_str().unwrap().starts_with("data:image/png;base64,"));
}

#[tokio::test]
async fn stream_cut_before_completed_is_retryable_error() {
    let server = MockServer::start().await;
    // Connection closes after half of the SAM line: the boxes must not be used.
    let body = format!("data: {}\n\n", json!({"type":"response.output_text.delta","delta":"<0f>0<|box;x1=520;y1=380"}));
    Mock::given(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;
    let err = client(&server).sam_segment(&tiny_png(800, 500), "button").await.unwrap_err();
    let meta = MetaError::find(&err).expect("MetaError in chain");
    assert!(matches!(meta, MetaError::StreamCut));
    assert!(meta.is_retryable());
}

#[tokio::test]
async fn error_body_never_echoes_the_key() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({"error":{"message":"invalid key sk-test-secret-123"}})),
        )
        .mount(&server)
        .await;
    let c = MetaClient::new(format!("{}/v1", server.uri()), "sk-test-secret-123");
    let req = ResponseRequest::new("m", "", vec![]);
    let err = c.respond(&req).await.unwrap_err();
    let text = format!("{err:#} {err:?}");
    assert!(!text.contains("sk-test-secret-123"), "{text}");
    assert!(text.contains("<redacted>"));
}
