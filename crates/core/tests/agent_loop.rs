//! Agent loop against a scripted Meta API (wiremock) and a mock platform. OWNER: agent "agent".

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use copilot_core::agent::{Agent, AgentOutcome, ApprovalAnswer, CancellationToken, Gate, Task};
use copilot_core::clock::SystemClock;
use copilot_core::events::{Bus, UiEvent};
use copilot_core::memory::Memory;
use copilot_core::meta::MetaClient;
use copilot_core::platform::{Frame, Input, PhysPoint, PhysRect, Platform, Screen, UiElement, UiTree, WindowInfo};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

// ---------- mock platform ----------

#[derive(Default)]
struct MockPlatform {
    actions: Mutex<Vec<String>>,
    ui: Mutex<Vec<(PhysRect, UiElement)>>,
    /// Foreground window (None = unknown, like Linux).
    foreground: Mutex<Option<WindowInfo>>,
    /// Element with keyboard focus (None = unknown).
    focus: Mutex<Option<UiElement>>,
    /// Real screenshot for live tests; None = plain grey 1000x800.
    screen: Option<image::RgbaImage>,
}

impl MockPlatform {
    fn actions(&self) -> Vec<String> {
        self.actions.lock().unwrap().clone()
    }
    fn typed(&self) -> String {
        self.actions().iter().filter_map(|a| a.strip_prefix("type ")).collect()
    }
    fn clicks(&self) -> Vec<String> {
        self.actions().into_iter().filter(|a| a.starts_with("click")).collect()
    }
    fn set_ui(&self, rect: PhysRect, name: &str, is_password: bool) {
        let el = UiElement { name: name.into(), control: "Button".into(), is_password };
        self.ui.lock().unwrap().push((rect, el));
    }
}

impl Screen for MockPlatform {
    fn capture_primary(&self) -> anyhow::Result<Frame> {
        // 1000x800 light-grey "screen": API image = frame (no downscale), origin 0,0.
        let image = self
            .screen
            .clone()
            .unwrap_or_else(|| image::RgbaImage::from_pixel(1000, 800, image::Rgba([240, 240, 240, 255])));
        Ok(Frame { image, origin: PhysPoint { x: 0, y: 0 }, scale: 1.0 })
    }
}

impl Input for MockPlatform {
    fn move_to(&self, p: PhysPoint) -> anyhow::Result<()> {
        self.actions.lock().unwrap().push(format!("move {},{}", p.x, p.y));
        Ok(())
    }
    fn click(&self, p: PhysPoint) -> anyhow::Result<()> {
        self.actions.lock().unwrap().push(format!("click {},{}", p.x, p.y));
        Ok(())
    }
    fn type_text(&self, s: &str) -> anyhow::Result<()> {
        self.actions.lock().unwrap().push(format!("type {s}"));
        Ok(())
    }
    fn key(&self, combo: &str) -> anyhow::Result<()> {
        self.actions.lock().unwrap().push(format!("key {combo}"));
        Ok(())
    }
    fn scroll(&self, p: PhysPoint, dy: i32) -> anyhow::Result<()> {
        self.actions.lock().unwrap().push(format!("scroll {},{} {dy}", p.x, p.y));
        Ok(())
    }
}

impl UiTree for MockPlatform {
    fn element_at(&self, p: PhysPoint) -> Option<UiElement> {
        self.ui.lock().unwrap().iter().find(|(r, _)| r.contains(p)).map(|(_, e)| e.clone())
    }
    fn foreground(&self) -> Option<WindowInfo> {
        self.foreground.lock().unwrap().clone()
    }
    fn focused(&self) -> Option<UiElement> {
        self.focus.lock().unwrap().clone()
    }
    fn find_by_name(&self, name: &str) -> Vec<(UiElement, PhysRect)> {
        let n = name.to_lowercase();
        self.ui
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, e)| e.name.to_lowercase().contains(&n))
            .map(|(r, e)| (e.clone(), *r))
            .collect()
    }
}

// ---------- scripted Meta API ----------

#[derive(Default)]
struct Script {
    /// Agent turns (Muse Spark with the agent tools). Empty -> a `wait 0` call.
    agent: Mutex<VecDeque<Value>>,
    /// SAM output texts. Empty -> no boxes.
    sam: Mutex<VecDeque<String>>,
    /// `choose` answers. Empty -> choice 1.
    pick: Mutex<VecDeque<Value>>,
    /// `locate` answers. Empty -> not found.
    locate: Mutex<VecDeque<Value>>,
    /// `label` answers (label reader for elements without a UI name). Empty -> no text, not final.
    label: Mutex<VecDeque<Value>>,
    /// Bodies of the agent requests, in order.
    agent_requests: Mutex<Vec<Value>>,
    /// While > 0, SAM and `locate` calls answer 503 (each call counts down).
    fail_ground: Mutex<u32>,
    /// While > 0, agent calls with a previous_response_id answer 400 "referenced response not found".
    lose_conversation: Mutex<u32>,
}

#[derive(Clone)]
struct Responder(Arc<Script>);

impl Respond for Responder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&req.body).unwrap();
        let s = &self.0;
        let grounding = body["model"] == "sam-3.1" || body["tools"].to_string().contains("\"locate\"");
        if grounding {
            let mut f = s.fail_ground.lock().unwrap();
            if *f > 0 {
                *f -= 1;
                return ResponseTemplate::new(503).set_body_string("busy");
            }
        }
        if body["model"] == "sam-3.1" {
            let text = s.sam.lock().unwrap().pop_front().unwrap_or_default();
            return ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse(&text));
        }
        let tool_names: Vec<String> = body["tools"]
            .as_array()
            .map(|a| a.iter().filter_map(|t| t["name"].as_str().map(String::from)).collect())
            .unwrap_or_default();
        if tool_names.iter().any(|n| n == "locate") {
            let v = s.locate.lock().unwrap().pop_front();
            let v = v.unwrap_or_else(|| calls("p_loc", &[("locate", json!({"found": false, "x": 0, "y": 0}))]));
            return ResponseTemplate::new(200).set_body_json(v);
        }
        if tool_names.iter().any(|n| n == "label") {
            let v = s.label.lock().unwrap().pop_front();
            let v = v.unwrap_or_else(|| calls("p_lbl", &[("label", json!({"text": "", "final_action": false}))]));
            return ResponseTemplate::new(200).set_body_json(v);
        }
        if tool_names.iter().any(|n| n == "choose") {
            let v = s.pick.lock().unwrap().pop_front();
            let v = v.unwrap_or_else(|| calls("p_pick", &[("choose", json!({"choice": 1}))]));
            return ResponseTemplate::new(200).set_body_json(v);
        }
        if body["previous_response_id"].is_string() {
            let mut l = s.lose_conversation.lock().unwrap();
            if *l > 0 {
                *l -= 1;
                return ResponseTemplate::new(400)
                    .set_body_string(r#"{"error":{"message":"referenced response not found or expired"}}"#);
            }
        }
        s.agent_requests.lock().unwrap().push(body);
        let v =
            s.agent.lock().unwrap().pop_front().unwrap_or_else(|| calls("r_wait", &[("wait", json!({"seconds": 0}))]));
        ResponseTemplate::new(200).set_body_json(v)
    }
}

fn sse(text: &str) -> String {
    let delta = json!({"type": "response.output_text.delta", "delta": text});
    let done = json!({"type": "response.completed", "response": {"id": "r_sam", "status": "completed",
        "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}]}});
    format!(
        "event: response.output_text.delta\ndata: {delta}\n\nevent: response.completed\ndata: {done}\n\ndata: [DONE]\n\n"
    )
}

fn usage() -> Value {
    json!({"input_tokens": 100, "output_tokens": 10, "total_tokens": 110})
}

/// A response with function calls (arguments as JSON strings, like the live API).
fn calls(id: &str, list: &[(&str, Value)]) -> Value {
    let output: Vec<Value> = list
        .iter()
        .enumerate()
        .map(|(i, (name, args))| {
            json!({"type": "function_call", "id": format!("fc_{i}"), "call_id": format!("call_{id}_{i}"),
                   "name": name, "arguments": args.to_string(), "status": "completed"})
        })
        .collect();
    json!({"id": id, "object": "response", "status": "completed", "output": output, "usage": usage()})
}

fn text_reply(id: &str, text: &str) -> Value {
    json!({"id": id, "object": "response", "status": "completed", "usage": usage(), "output": [
        {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}]})
}

fn sam_box(x1: u32, y1: u32, x2: u32, y2: u32) -> String {
    format!("<|box;x1={x1};y1={y1};x2={x2};y2={y2};w=1000;h=800|><|mask;abc|>")
}

fn done(summary: &str) -> (&'static str, Value) {
    ("done", json!({"summary": summary}))
}

fn click(desc: &str, is_commit: bool) -> (&'static str, Value) {
    (
        "click",
        json!({"sam_query": "blue button", "description": desc, "is_commit": is_commit,
                     "commit_what": if is_commit { json!("deine Krankschreibung") } else { json!("null") },
                     "commit_to": if is_commit { json!("Gesundkasse") } else { Value::Null }}),
    )
}

// ---------- harness ----------

struct Harness {
    agent: Agent,
    bus: Bus,
    platform: Arc<MockPlatform>,
    memory: Arc<Memory>,
    script: Arc<Script>,
    opened: Arc<Mutex<Vec<String>>>,
    events: tokio::sync::broadcast::Receiver<UiEvent>,
    _server: MockServer,
}

impl Harness {
    async fn new(demo: bool) -> Self {
        let server = MockServer::start().await;
        let script = Arc::new(Script::default());
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(Responder(script.clone()))
            .mount(&server)
            .await;
        let meta = Arc::new(MetaClient::new(server.uri(), "test-key"));
        let platform = Arc::new(MockPlatform::default());
        let p = Platform { screen: platform.clone(), input: platform.clone(), ui: platform.clone() };
        let memory = Arc::new(Memory::open_in_memory().unwrap());
        let (bus, _rx) = Bus::new();
        let events = bus.to_ui.subscribe();
        let opened = Arc::new(Mutex::new(Vec::new()));
        let o = opened.clone();
        let gate = Gate::new(demo).with_fixtures(&["C:/demo/fixtures"]);
        let agent = Agent::new(meta, p, memory.clone(), gate, bus.clone(), Arc::new(SystemClock))
            .with_pace(Duration::ZERO)
            .with_approval_timeout(Duration::from_secs(5))
            .with_url_opener(Arc::new(move |u: &str| {
                o.lock().unwrap().push(u.to_string());
                Ok(())
            }));
        Self { agent, bus, platform, memory, script, opened, events, _server: server }
    }

    fn agent_turns(&self, turns: Vec<Value>) {
        self.script.agent.lock().unwrap().extend(turns);
    }
    fn sam(&self, texts: &[String]) {
        self.script.sam.lock().unwrap().extend(texts.iter().cloned());
    }
    fn pick(&self, v: Value) {
        self.script.pick.lock().unwrap().push_back(v);
    }
    fn locate(&self, x: f64, y: f64) {
        let v = calls("p_loc", &[("locate", json!({"found": true, "x": x, "y": y}))]);
        self.script.locate.lock().unwrap().push_back(v);
    }

    /// Answers every approval card with `approval` and every question with `answer`.
    fn auto_answer(&self, approval: ApprovalAnswer, answer: &'static str) {
        let mut rx = self.bus.to_ui.subscribe();
        let pending = self.bus.pending.clone();
        tokio::spawn(async move {
            while let Ok(ev) = rx.recv().await {
                match ev {
                    UiEvent::ApprovalRequest { card } => {
                        pending.resolve_approval(card.id, approval);
                    }
                    UiEvent::Bubble { question_id: Some(q), .. } => {
                        pending.resolve_answer(q, answer.to_string());
                    }
                    _ => {}
                }
            }
        });
    }

    async fn run(&self, task: &str) -> AgentOutcome {
        self.agent.run(Task::Text(task.into()), CancellationToken::new()).await.unwrap()
    }

    fn drain(&mut self) -> Vec<UiEvent> {
        let mut out = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(e) => out.push(e),
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
        out
    }

    fn requests(&self) -> Vec<Value> {
        self.script.agent_requests.lock().unwrap().clone()
    }
}

fn bubbles(events: &[UiEvent]) -> Vec<String> {
    events.iter().filter_map(|e| if let UiEvent::Bubble { text, .. } = e { Some(text.clone()) } else { None }).collect()
}
fn progress(events: &[UiEvent]) -> Vec<String> {
    events.iter().filter_map(|e| if let UiEvent::Progress { text } = e { Some(text.clone()) } else { None }).collect()
}
fn cards(events: &[UiEvent]) -> Vec<copilot_core::agent::ApprovalCard> {
    events
        .iter()
        .filter_map(|e| if let UiEvent::ApprovalRequest { card } = e { Some(card.clone()) } else { None })
        .collect()
}
fn outputs(req: &Value) -> Vec<String> {
    req["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["type"] == "function_call_output")
        .map(|i| i["output"].as_str().unwrap().to_string())
        .collect()
}

// ---------- tests ----------

#[tokio::test]
async fn fills_a_field_then_finishes() {
    let mut h = Harness::new(false).await;
    h.agent_turns(vec![
        calls(
            "r1",
            &[(
                "type_text",
                json!({"text": "Erika Beispiel", "what": "deinen Namen", "into_sam_query": "text input field"}),
            )],
        ),
        calls("r2", &[done("Dein Name steht im Formular.")]),
    ]);
    h.sam(&[sam_box(100, 100, 300, 140)]);
    let out = h.run("Trag meinen Namen ein").await;
    assert_eq!(out, AgentOutcome::Done("Dein Name steht im Formular.".into()));
    assert_eq!(h.platform.clicks(), vec!["click 200,120"]);
    assert_eq!(h.platform.typed(), "Erika Beispiel");

    let ev = h.drain();
    assert!(progress(&ev).contains(&"Ich trage deinen Namen ein.".to_string()), "{:?}", progress(&ev));
    assert!(bubbles(&ev).contains(&"Erledigt. Ich merke mir: Dein Name steht im Formular.".to_string()));
    assert!(ev.iter().any(|e| matches!(e, UiEvent::PointAt { rect, .. } if rect.w == 200 && rect.h == 40)));

    let reqs = h.requests();
    assert_eq!(reqs.len(), 2);
    for r in &reqs {
        assert!(r["instructions"].as_str().unwrap().contains("Never type passwords"));
        assert_eq!(r["tools"].as_array().unwrap().len(), 13);
        assert_eq!(r["reasoning"]["effort"], "low");
    }
    assert!(reqs[0]["input"].to_string().contains("Trag meinen Namen ein"));
    assert_eq!(reqs[1]["previous_response_id"], "r1");
    assert_eq!(outputs(&reqs[1]), vec!["typed deinen Namen"]);
    assert_eq!(reqs[1]["input"][0]["call_id"], "call_r1_0");
    // model calls are logged with tokens
    assert!(h.memory.tokens_today().unwrap() >= 220);
    assert_eq!(h.memory.recent_events(1).unwrap()[0].kind, "done");
}

#[tokio::test]
async fn commit_click_waits_for_yes_and_uses_spark_pick() {
    let mut h = Harness::new(true).await;
    h.agent_turns(vec![
        calls("r1", &[("open_url", json!({"url": "file:///C:/demo/fixtures/demo-kasse/index.html"}))]),
        calls("r2", &[click("den Knopf „Absenden“ unten rechts", false)]),
        calls("r3", &[done("Krankschreibung ist bei der Kasse.")]),
    ]);
    h.sam(&[format!("<0f>1{}2{}", sam_box(100, 600, 200, 640), sam_box(700, 600, 900, 640))]);
    h.pick(calls("p1", &[("choose", json!({"choice": 2}))]));
    h.auto_answer(ApprovalAnswer::Yes, "");
    let out = h.run("Schick es ab").await;
    assert_eq!(out, AgentOutcome::Done("Krankschreibung ist bei der Kasse.".into()));
    assert_eq!(h.opened.lock().unwrap().as_slice(), ["file:///C:/demo/fixtures/demo-kasse/index.html"]);
    assert_eq!(h.platform.clicks(), vec!["click 800,620"]);
    let ev = h.drain();
    let c = cards(&ev);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].title, "Darf ich das abschicken?");
    assert_eq!(c[0].yes_label, "Ja, abschicken");
    assert_eq!(c[0].no_label, "Noch nicht");
    assert_eq!(c[0].to, "An: Seite auf diesem Computer", "the real page, not only the model's word");
    assert!(c[0].crop_png_b64.is_some());
    assert!(ev.iter().any(|e| matches!(e, UiEvent::ApprovalClosed { id } if *id == c[0].id)));
}

#[tokio::test]
async fn send_card_stays_a_send_card_when_what_names_the_file() {
    // Live run 2026-10-26: commit_what "deine Krankschreibung (Datei au_erika_erst.pdf)" on "Absenden" made the
    // card say "Darf ich das anhängen?" / "Ja, anhängen". The card must say what really happens: sending.
    let mut h = Harness::new(false).await;
    let send = (
        "click",
        json!({"sam_query": "green button", "description": "den grünen Knopf „Absenden“ unten rechts",
               "is_commit": true, "commit_what": "deine Krankschreibung (Datei au_erika_erst.pdf)",
               "commit_to": "Gesundkasse (Kontaktformular)"}),
    );
    h.agent_turns(vec![calls("r1", &[send])]);
    h.sam(&[sam_box(700, 600, 900, 640)]);
    h.auto_answer(ApprovalAnswer::No, "");
    assert_eq!(h.run("Schick es ab").await, AgentOutcome::Stopped);
    assert!(h.platform.clicks().is_empty());
    let c = cards(&h.drain());
    assert_eq!(c[0].title, "Darf ich das abschicken?");
    assert_eq!(c[0].yes_label, "Ja, abschicken");
}

#[tokio::test]
async fn commit_no_means_stop_and_no_click() {
    let mut h = Harness::new(false).await;
    h.agent_turns(vec![calls("r1", &[click("den blauen Knopf", true)])]);
    h.sam(&[sam_box(700, 600, 900, 640)]);
    h.auto_answer(ApprovalAnswer::No, "");
    let out = h.run("Schick es ab").await;
    assert_eq!(out, AgentOutcome::Stopped);
    assert!(h.platform.clicks().is_empty());
    let ev = h.drain();
    let c = cards(&ev);
    assert_eq!(c[0].what, "Was: deine Krankschreibung");
    assert_eq!(c[0].to, "An: Gesundkasse");
    assert!(bubbles(&ev).contains(&"Ich habe aufgehört. Nichts wurde abgeschickt.".to_string()));
}

#[tokio::test]
async fn approval_timeout_is_no() {
    let mut h = Harness::new(false).await;
    h.agent = h.agent.with_approval_timeout(Duration::from_millis(50));
    h.agent_turns(vec![calls("r1", &[click("Jetzt bezahlen", false)])]);
    h.sam(&[sam_box(700, 600, 900, 640)]);
    let out = h.run("Bezahl das").await;
    assert!(matches!(out, AgentOutcome::NeedsUser(ref t) if t.starts_with("Du warst gerade nicht da")), "{out:?}");
    assert!(h.platform.clicks().is_empty());
    let ev = h.drain();
    assert_eq!(cards(&ev).len(), 1);
    // The answer stays on screen with a way to go on (the user was away, that is why it timed out).
    let stay = ev.iter().find_map(|e| match e {
        UiEvent::Bubble { text, buttons, .. } if text.starts_with("Du warst gerade nicht da") => Some(buttons.clone()),
        _ => None,
    });
    let ids: Vec<String> = stay.expect("timeout bubble").iter().map(|b| b.id.clone()).collect();
    assert_eq!(ids, ["cmd:resume", "close"]);
    assert!(h.agent.can_resume());
}

#[tokio::test]
async fn ui_tree_name_triggers_the_gate() {
    let mut h = Harness::new(false).await;
    h.platform.set_ui(PhysRect { x: 700, y: 600, w: 200, h: 40 }, "Jetzt kaufen", false);
    h.agent_turns(vec![calls("r1", &[click("den blauen Knopf", false)]), calls("r2", &[done("Gekauft.")])]);
    h.sam(&[sam_box(700, 600, 900, 640)]);
    h.auto_answer(ApprovalAnswer::Yes, "");
    let out = h.run("Mach weiter").await;
    assert_eq!(out, AgentOutcome::Done("Gekauft.".into()));
    assert_eq!(cards(&h.drain()).len(), 1, "UI name 'Jetzt kaufen' must need approval");
    assert_eq!(h.platform.clicks().len(), 1);
}

#[tokio::test]
async fn plain_click_needs_no_card() {
    let mut h = Harness::new(false).await;
    h.agent_turns(vec![calls("r1", &[click("den Knopf „Weiter“", false)]), calls("r2", &[done("Weiter.")])]);
    h.sam(&[sam_box(700, 600, 900, 640)]);
    h.run("Weiter").await;
    assert!(cards(&h.drain()).is_empty());
    assert_eq!(h.platform.clicks(), vec!["click 800,620"]);
}

#[tokio::test]
async fn demo_mode_blocks_commit_on_unknown_page() {
    let mut h = Harness::new(true).await;
    h.agent_turns(vec![calls("r1", &[click("den Knopf „Absenden“", true)]), calls("r2", &[done("Nichts geschickt.")])]);
    h.sam(&[sam_box(700, 600, 900, 640)]);
    h.run("Schick es ab").await;
    assert!(cards(&h.drain()).is_empty());
    assert!(h.platform.clicks().is_empty());
    assert!(outputs(&h.requests()[1])[0].starts_with("blocked: demo mode"));
}

#[tokio::test]
async fn open_url_is_checked() {
    let h = Harness::new(true).await;
    h.agent_turns(vec![
        calls("r1", &[("open_url", json!({"url": "https://www.example-kasse.de/kontakt"}))]),
        calls("r2", &[done("x")]),
    ]);
    h.run("Öffne die Seite").await;
    assert!(h.opened.lock().unwrap().is_empty());
    assert!(outputs(&h.requests()[1])[0].starts_with("blocked"));
}

#[tokio::test]
async fn password_field_is_never_typed() {
    let mut h = Harness::new(false).await;
    h.platform.set_ui(PhysRect { x: 100, y: 100, w: 200, h: 40 }, "Kennwort", true);
    h.agent_turns(vec![
        calls(
            "r1",
            &[("type_text", json!({"text": "geheim123", "what": "das Feld", "into_sam_query": "text input field"}))],
        ),
        calls("r2", &[done("Angemeldet.")]),
    ]);
    h.sam(&[sam_box(100, 100, 300, 140)]);
    h.auto_answer(ApprovalAnswer::Yes, "Weiter");
    let out = h.run("Melde mich an").await;
    assert_eq!(out, AgentOutcome::Done("Angemeldet.".into()));
    assert_eq!(h.platform.typed(), "");
    assert!(h.platform.clicks().is_empty());
    assert!(bubbles(&h.drain()).iter().any(|b| b.starts_with("Passwörter tippe ich nicht ein.")));
    assert!(outputs(&h.requests()[1])[0].starts_with("refused"));
}

#[tokio::test]
async fn ask_user_answer_goes_back_to_the_model() {
    let h = Harness::new(false).await;
    h.agent_turns(vec![
        calls("r1", &[("ask_user", json!({"question": "Welche Kasse?", "options": ["Gesundkasse", "Andere"]}))]),
        calls(
            "r2",
            &[
                ("remember_fact", json!({"key": "krankenkasse", "value": "Gesundkasse"})),
                done("Deine Kasse ist Gesundkasse."),
            ],
        ),
    ]);
    h.auto_answer(ApprovalAnswer::Yes, "Gesundkasse");
    let out = h.run("Merk dir meine Kasse").await;
    assert_eq!(out, AgentOutcome::Done("Deine Kasse ist Gesundkasse.".into()));
    assert_eq!(outputs(&h.requests()[1]), vec!["The user answered: Gesundkasse"]);
    assert_eq!(h.memory.profile_get("krankenkasse").unwrap().as_deref(), Some("Gesundkasse"));
    // the next run sees the fact in its context
    h.agent_turns(vec![calls("r3", &[done("ok")])]);
    h.run("Was weißt du?").await;
    assert!(h.requests()[2]["input"].to_string().contains("krankenkasse: Gesundkasse"));
}

#[tokio::test]
async fn stop_while_waiting_ends_at_once() {
    let mut h = Harness::new(false).await;
    h.agent_turns(vec![calls("r1", &[("ask_user", json!({"question": "Weiter?", "options": []}))])]);
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    let mut rx = h.bus.to_ui.subscribe();
    tokio::spawn(async move {
        while let Ok(ev) = rx.recv().await {
            if matches!(ev, UiEvent::Bubble { question_id: Some(_), .. }) {
                c.cancel();
            }
        }
    });
    let out = tokio::time::timeout(Duration::from_secs(3), h.agent.run(Task::Text("x".into()), cancel))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(out, AgentOutcome::Stopped);
    assert!(bubbles(&h.drain()).contains(&"Ich habe aufgehört. Nichts wurde abgeschickt.".to_string()));
}

#[tokio::test]
async fn invalid_arguments_go_back_to_the_model() {
    let h = Harness::new(false).await;
    h.agent_turns(vec![
        calls("r1", &[("scroll", json!({"direction": "down", "amount": 99}))]),
        calls("r2", &[("scroll", json!({"direction": "down", "amount": 3}))]),
        calls("r3", &[done("ok")]),
    ]);
    h.run("scroll").await;
    let r = h.requests();
    assert!(outputs(&r[1])[0].contains("amount must be 1..10"), "{:?}", outputs(&r[1]));
    assert_eq!(h.platform.actions(), vec!["scroll 500,400 3"]);
}

#[tokio::test]
async fn plain_text_answer_ends_the_run() {
    let mut h = Harness::new(false).await;
    h.agent_turns(vec![text_reply("r1", "Deine Frist endet am 2. November.")]);
    let out = h.run("Wann endet meine Frist?").await;
    assert_eq!(out, AgentOutcome::Done("Deine Frist endet am 2. November.".into()));
    let b = bubbles(&h.drain());
    assert_eq!(b, vec!["Deine Frist endet am 2. November."]);
}

#[tokio::test]
async fn step_limit_ends_the_run() {
    let mut h = Harness::new(false).await;
    // empty script -> the model keeps calling `wait 0`
    let out = h.run("endlos").await;
    assert!(matches!(out, AgentOutcome::Failed(ref t) if t.starts_with("Das dauert zu lange")), "{out:?}");
    assert_eq!(h.requests().len(), copilot_core::agent::MAX_STEPS as usize);
    assert!(h.drain().iter().any(|e| matches!(e, UiEvent::Error { retry: true, .. })));
}

#[tokio::test]
async fn not_found_falls_back_to_spark_xy() {
    let h = Harness::new(false).await;
    h.agent_turns(vec![calls("r1", &[click("den Link „Kontakt“", false)]), calls("r2", &[done("ok")])]);
    h.sam(&[String::new()]); // no SAM hit
    h.locate(0.25, 0.5);
    h.run("Kontakt").await;
    assert_eq!(h.platform.clicks(), vec!["click 250,400"]);
}

#[tokio::test]
async fn spark_point_snaps_to_the_sam_box_around_it() {
    let mut h = Harness::new(false).await;
    h.agent_turns(vec![calls("r1", &[click("den Knopf „Weiter“", false)]), calls("r2", &[done("ok")])]);
    h.sam(&[format!("{}{}", sam_box(100, 100, 200, 140), sam_box(600, 700, 800, 760))]);
    h.locate(0.61, 0.9); // (610, 720): inside the second box, off its center
    h.run("Weiter").await;
    assert_eq!(h.platform.clicks(), vec!["click 700,730"]);
    let ev = h.drain();
    assert!(
        ev.iter()
            .any(|e| matches!(e, UiEvent::PointAt { rect, .. } if *rect == PhysRect { x: 600, y: 700, w: 200, h: 60 }))
    );
}

#[tokio::test]
async fn nothing_found_tells_the_model() {
    let h = Harness::new(false).await;
    h.agent_turns(vec![calls("r1", &[click("den Knopf „Weiter“", false)]), calls("r2", &[done("ok")])]);
    h.sam(&[format!("{}{}", sam_box(100, 100, 200, 140), sam_box(300, 100, 400, 140))]);
    h.pick(calls("p1", &[("choose", json!({"choice": 0}))]));
    h.run("Weiter").await;
    assert!(h.platform.clicks().is_empty());
    assert!(outputs(&h.requests()[1])[0].starts_with("not found"));
}

#[tokio::test]
async fn add_deadline_saves_with_reminders() {
    let h = Harness::new(false).await;
    let due = (chrono::Local::now() + chrono::Duration::days(20)).date_naive();
    h.agent_turns(vec![
        calls(
            "r1",
            &[(
                "add_deadline",
                json!({"title": "Widerspruch Bescheid", "due": due.to_string(), "why": "Sonst gilt er."}),
            )],
        ),
        calls("r2", &[done("Frist gespeichert.")]),
    ]);
    h.run("Merk dir die Frist").await;
    let open = h.memory.open_deadlines().unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].due, due);
    assert_eq!(open[0].remind_at.len(), 3);
}

#[tokio::test]
async fn send_latest_document_without_letter_asks_for_it() {
    let h = Harness::new(false).await;
    let out = h
        .agent
        .run(
            Task::SendLatestDocument { doc_type: copilot_core::docs::DocType::AuBescheinigung },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(matches!(out, AgentOutcome::NeedsUser(_)));
    assert!(h.requests().is_empty(), "no model call without a letter");
}

#[tokio::test]
async fn retries_once_on_server_error() {
    let h = Harness::new(false).await;
    // A 503 on the first call, then the normal script.
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(503).set_body_string("busy"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&h._server)
        .await;
    h.agent_turns(vec![calls("r1", &[done("ok")])]);
    let out = h.run("x").await;
    assert_eq!(out, AgentOutcome::Done("ok".into()));
}

#[tokio::test]
async fn enter_in_a_form_needs_approval() {
    let mut h = Harness::new(false).await;
    h.agent_turns(vec![calls(
        "r1",
        &[
            ("type_text", json!({"text": "Erika", "what": "deinen Namen", "into_sam_query": "text input field"})),
            ("key", json!({"combo": "enter", "is_commit": false, "commit_what": null, "commit_to": null})),
        ],
    )]);
    h.sam(&[sam_box(100, 100, 300, 140)]);
    h.auto_answer(ApprovalAnswer::No, "");
    let out = h.run("Trag meinen Namen ein").await;
    assert_eq!(out, AgentOutcome::Stopped);
    assert!(!h.platform.actions().iter().any(|a| a.starts_with("key")), "{:?}", h.platform.actions());
    let c = cards(&h.drain());
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].what, "Was: das ausgefüllte Formular");
}

fn store_letter(h: &Harness, path: &str) -> i64 {
    let x: copilot_core::docs::DocumentExtract = serde_json::from_value(json!({
        "doc_type": "au_bescheinigung", "sender": "Praxis Dr. Beispiel", "recipient": "Gesundkasse",
        "date": "2026-10-20", "reference": null, "summary_simple": "Krankschreibung", "requests": [], "deadlines": [],
        "au": {"from": "2026-10-20", "until": "2026-10-30", "first_or_follow": "erst", "issued": null},
        "consequences_if_ignored": null, "confidence": 0.95}))
    .unwrap();
    h.memory.insert_document(std::path::Path::new(path), "sha-au", &x, "Krankschreibung").unwrap()
}

fn file_dialog() -> WindowInfo {
    WindowInfo { title: "Öffnen".into(), class: "#32770".into(), address: None }
}

#[tokio::test]
async fn one_enter_confirms_the_file_dialog_after_an_approved_attach() {
    let mut h = Harness::new(false).await;
    store_letter(&h, "C:\\Briefe\\au.pdf");
    *h.platform.foreground.lock().unwrap() = Some(file_dialog());
    let enter = ("key", json!({"combo": "enter", "is_commit": false, "commit_what": null, "commit_to": null}));
    h.agent_turns(vec![
        calls("r1", &[click("den Knopf „Datei auswählen“", false)]),
        calls(
            "r2",
            &[
                ("type_text", json!({"text": "C:\\Briefe\\au.pdf", "what": "den Dateinamen", "into_sam_query": null})),
                enter.clone(),
            ],
        ),
        calls("r3", &[enter]),
        calls("r4", &[done("Angehängt.")]),
    ]);
    h.sam(&[sam_box(100, 600, 300, 640)]);
    h.auto_answer(ApprovalAnswer::Yes, "");
    let out = h.run("Häng die Krankschreibung an").await;
    assert_eq!(out, AgentOutcome::Done("Angehängt.".into()));
    let c = cards(&h.drain());
    assert_eq!(c.len(), 2, "attach card, then a card for the second Enter only");
    assert!(c[0].title.contains("anhängen"), "{}", c[0].title);
    assert_eq!(h.platform.actions().iter().filter(|a| *a == "key enter").count(), 2);
}

#[tokio::test]
async fn enter_after_attach_needs_a_card_when_the_dialog_is_not_seen() {
    // Linux / unknown foreground window: nobody can tell that the file dialog is open, so Enter is asked.
    let mut h = Harness::new(false).await;
    store_letter(&h, "C:\\Briefe\\au.pdf");
    let enter = ("key", json!({"combo": "enter", "is_commit": false, "commit_what": null, "commit_to": null}));
    h.agent_turns(vec![
        calls("r1", &[click("den Knopf „Datei auswählen“", false)]),
        calls(
            "r2",
            &[
                ("type_text", json!({"text": "C:\\Briefe\\au.pdf", "what": "den Dateinamen", "into_sam_query": null})),
                enter,
            ],
        ),
        calls("r3", &[done("Angehängt.")]),
    ]);
    h.sam(&[sam_box(100, 600, 300, 640)]);
    h.auto_answer(ApprovalAnswer::Yes, "");
    h.run("Häng die Krankschreibung an").await;
    assert_eq!(cards(&h.drain()).len(), 2);
}

#[tokio::test]
async fn the_approved_letter_is_typed_into_the_file_dialog() {
    // Task with a known letter: whatever path the model types, Pip types the approved letter's path.
    let mut h = Harness::new(false).await;
    store_letter(&h, "C:\\Briefe\\au.pdf");
    *h.platform.foreground.lock().unwrap() = Some(file_dialog());
    let enter = ("key", json!({"combo": "enter", "is_commit": false, "commit_what": null, "commit_to": null}));
    h.agent_turns(vec![
        calls("r1", &[click("den Knopf „Datei auswählen“", true)]),
        calls(
            "r2",
            &[
                (
                    "type_text",
                    json!({"text": "C:\\Users\\erika\\Steuer.pdf", "what": "den Dateinamen", "into_sam_query": null}),
                ),
                enter,
            ],
        ),
        calls("r3", &[done("Angehängt.")]),
    ]);
    h.sam(&[sam_box(100, 600, 300, 640)]);
    h.auto_answer(ApprovalAnswer::Yes, "");
    let out = h
        .agent
        .run(
            Task::SendLatestDocument { doc_type: copilot_core::docs::DocType::AuBescheinigung },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(out, AgentOutcome::Done("Angehängt.".into()));
    assert_eq!(h.platform.typed(), "C:\\Briefe\\au.pdf");
    let c = cards(&h.drain());
    assert_eq!(c.len(), 1, "the dialog's Enter is free");
    assert!(c[0].attachments.len() == 1 && c[0].attachments[0].ends_with("au.pdf"), "{:?}", c[0].attachments);
}

#[tokio::test]
async fn stop_after_sending_never_says_nothing_was_sent() {
    // Erika says "Ja", Pip clicks Absenden; then she stops Pip ("Noch nicht" on the next card here).
    let mut h = Harness::new(false).await;
    h.agent_turns(vec![
        calls("r1", &[click("den Knopf „Absenden“", true)]),
        calls("r2", &[click("den Knopf „Konto löschen“", true)]),
    ]);
    h.sam(&[sam_box(700, 600, 900, 640), sam_box(700, 600, 900, 640)]);
    let mut rx = h.bus.to_ui.subscribe();
    let pending = h.bus.pending.clone();
    tokio::spawn(async move {
        let mut n = 0;
        while let Ok(ev) = rx.recv().await {
            if let UiEvent::ApprovalRequest { card } = ev {
                n += 1;
                pending.resolve_approval(card.id, if n == 1 { ApprovalAnswer::Yes } else { ApprovalAnswer::No });
            }
        }
    });
    let out = h.run("Schick es ab").await;
    assert_eq!(out, AgentOutcome::StoppedAfterSend("deine Krankschreibung".into()));
    let ev = h.drain();
    let b = bubbles(&ev);
    assert!(b.contains(&"Ich habe aufgehört. Abgeschickt habe ich schon: deine Krankschreibung.".to_string()), "{b:?}");
    assert!(!b.iter().any(|t| t.contains("Nichts wurde abgeschickt")), "{b:?}");
    assert!(!h.agent.can_resume(), "no Weiter after a send");
    let c = cards(&ev);
    assert_eq!(c[1].title, "Darf ich das löschen?");
}

#[tokio::test]
async fn weiter_continues_the_stopped_run_with_what_was_done() {
    let mut h = Harness::new(false).await;
    h.agent = h.agent.with_approval_timeout(Duration::from_millis(50));
    h.agent_turns(vec![
        calls(
            "r1",
            &[("type_text", json!({"text": "Erika", "what": "deinen Namen", "into_sam_query": "text input field"}))],
        ),
        calls("r2", &[click("den Knopf „Absenden“", true)]),
    ]);
    h.sam(&[sam_box(100, 100, 300, 140), sam_box(700, 600, 900, 640)]);
    assert!(matches!(h.run("Füll das aus").await, AgentOutcome::NeedsUser(_)));
    assert!(h.agent.can_resume());
    h.agent_turns(vec![calls("r3", &[done("Fertig.")])]);
    let out = h.agent.resume(CancellationToken::new()).await.unwrap();
    assert_eq!(out, Some(AgentOutcome::Done("Fertig.".into())));
    let reqs = h.requests();
    let last = reqs.last().unwrap();
    assert!(last["previous_response_id"].is_null(), "fresh conversation");
    let text = last["input"].to_string();
    assert!(text.contains("EARLIER IN THIS TASK") && text.contains("typed deinen Namen"), "{text}");
    assert_eq!(h.platform.typed(), "Erika", "nothing typed twice");
    let _ = h.drain();
}

#[tokio::test]
async fn card_lists_what_was_typed_and_speaks_the_whole_question() {
    let mut h = Harness::new(false).await;
    h.agent_turns(vec![
        calls(
            "r1",
            &[(
                "type_text",
                json!({"text": "Erika Beispiel", "what": "deinen Namen", "into_sam_query": "text input field"}),
            )],
        ),
        calls("r2", &[click("den Knopf „Absenden“", true)]),
    ]);
    h.sam(&[sam_box(100, 100, 300, 140), sam_box(700, 600, 900, 640)]);
    h.auto_answer(ApprovalAnswer::No, "");
    h.run("Schick es ab").await;
    let ev = h.drain();
    let c = cards(&ev);
    assert_eq!(c[0].filled, ["Namen: Erika Beispiel"]);
    let spoken: Vec<String> =
        ev.iter().filter_map(|e| if let UiEvent::Speak { text } = e { Some(text.clone()) } else { None }).collect();
    assert!(spoken.contains(&"Darf ich deine Krankschreibung an Gesundkasse abschicken?".to_string()), "{spoken:?}");
}

#[tokio::test]
async fn type_text_never_clicks_a_send_button() {
    let h = Harness::new(false).await;
    h.platform.set_ui(PhysRect { x: 700, y: 600, w: 200, h: 40 }, "Absenden", false);
    h.agent_turns(vec![
        calls("r1", &[("type_text", json!({"text": "x", "what": "etwas", "into_sam_query": "blue button"}))]),
        calls("r2", &[done("ok")]),
    ]);
    h.sam(&[sam_box(700, 600, 900, 640)]);
    h.run("Tipp was").await;
    assert!(h.platform.actions().is_empty(), "{:?}", h.platform.actions());
    assert!(outputs(&h.requests()[1])[0].starts_with("refused"));
}

#[tokio::test]
async fn ask_without_options_still_has_a_button() {
    let mut h = Harness::new(false).await;
    h.agent_turns(vec![
        calls("r1", &[("ask_user", json!({"question": "Bitte melde dich an.", "options": []}))]),
        calls("r2", &[done("ok")]),
    ]);
    h.auto_answer(ApprovalAnswer::Yes, "Weiter");
    h.run("Anmelden").await;
    let ev = h.drain();
    let b = ev.iter().find_map(|e| match e {
        UiEvent::Bubble { question_id: Some(_), buttons, .. } => Some(buttons.clone()),
        _ => None,
    });
    assert_eq!(b.unwrap().iter().map(|b| b.label.as_str()).collect::<Vec<_>>(), vec!["Weiter"]);
}

#[tokio::test]
async fn network_error_mid_step_still_answers_every_call() {
    let h = Harness::new(false).await;
    *h.script.fail_ground.lock().unwrap() = 2; // SAM and locate both fail once
    h.agent_turns(vec![
        calls("r1", &[click("den Knopf „Weiter“", false), ("say", json!({"text": "Gleich."}))]),
        calls("r2", &[done("ok")]),
    ]);
    let out = h.run("Weiter").await;
    assert_eq!(out, AgentOutcome::Done("ok".into()));
    let r = h.requests();
    assert_eq!(r[1]["previous_response_id"], "r1");
    let ids: Vec<&str> = r[1]["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["type"] == "function_call_output")
        .map(|i| i["call_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["call_r1_0", "call_r1_1"]);
}

// ---------- live tests (real Meta API, cost a few cents): cargo test -p copilot-core --test agent_loop -- --ignored ----------

/// Screenshot of fixtures/demo-kasse/index.html at 1600x1000 (env AGENT_LIVE_PNG, default fixtures/screens/kasse_form.png).
fn live_png() -> Option<image::RgbaImage> {
    let path = std::env::var("AGENT_LIVE_PNG")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/screens/kasse_form.png").to_string());
    match image::open(&path) {
        Ok(img) => Some(img.to_rgba8()),
        Err(e) => {
            eprintln!("skipping live test: cannot open {path}: {e}");
            None
        }
    }
}

fn live_agent(png: image::RgbaImage, demo: bool) -> (Agent, Arc<MockPlatform>, Bus) {
    let meta = Arc::new(MetaClient::from_env().expect("MODEL_API_KEY"));
    let platform = Arc::new(MockPlatform { screen: Some(png), ..Default::default() });
    let p = Platform { screen: platform.clone(), input: platform.clone(), ui: platform.clone() };
    let memory = Arc::new(Memory::open_in_memory().unwrap());
    memory.profile_set("name", "Erika Beispiel").unwrap();
    memory.profile_set("versichertennummer", "G123456789").unwrap();
    let (bus, _rx) = Bus::new();
    let agent = Agent::new(meta, p, memory, Gate::new(demo), bus.clone(), Arc::new(SystemClock))
        .with_pace(Duration::ZERO)
        .with_approval_timeout(Duration::from_secs(5))
        .with_url_opener(Arc::new(|_: &str| Ok(())));
    (agent, platform, bus)
}

/// Rects of the demo form at 1600x1000 (measured on the Firefox render).
const NAME_FIELD: PhysRect = PhysRect { x: 307, y: 477, w: 313, h: 53 };
const NUMBER_FIELD: PhysRect = PhysRect { x: 639, y: 477, w: 313, h: 53 };
const FILE_BUTTON: PhysRect = PhysRect { x: 327, y: 767, w: 231, h: 53 };
const SEND_BUTTON: PhysRect = PhysRect { x: 766, y: 911, w: 186, h: 59 };

#[tokio::test]
#[ignore]
async fn live_ground_demo_form() {
    let Some(png) = live_png() else { return };
    let (agent, platform, _bus) = live_agent(png, false);
    let mut frame = platform.capture_primary().unwrap();
    frame.mask(&[]);
    let api = frame.for_api(copilot_core::agent::API_MAX_SIDE).unwrap();
    let cases = [
        ("text input field", "das Eingabefeld „Vor- und Nachname“", NAME_FIELD),
        ("text input field", "das Eingabefeld „Versichertennummer“", NUMBER_FIELD),
        ("button", "den Knopf „Datei auswählen“", FILE_BUTTON),
        ("green button", "den Knopf „Absenden“ unten rechts", SEND_BUTTON),
    ];
    let mut hits = 0;
    for (q, desc, rect) in cases {
        let t = std::time::Instant::now();
        let p = agent.ground(&api, q, desc).await.unwrap();
        let hit = p.is_some_and(|p| rect.contains(p));
        hits += usize::from(hit);
        eprintln!("ground {desc:?} -> {p:?} hit={hit} ({} ms)", t.elapsed().as_millis());
    }
    assert!(hits >= 3, "only {hits}/4 targets hit");
}

#[tokio::test]
#[ignore]
async fn live_agent_fills_the_form() {
    let Some(png) = live_png() else { return };
    let (agent, platform, bus) = live_agent(png, false);
    let mut rx = bus.to_ui.subscribe();
    let pending = bus.pending.clone();
    tokio::spawn(async move {
        while let Ok(ev) = rx.recv().await {
            match &ev {
                UiEvent::ApprovalRequest { card } => {
                    pending.resolve_approval(card.id, ApprovalAnswer::No);
                }
                UiEvent::Bubble { question_id: Some(q), .. } => {
                    pending.resolve_answer(*q, "Weiter".into());
                }
                _ => {}
            }
            eprintln!("ui: {}", serde_json::to_string(&ev).unwrap().chars().take(160).collect::<String>());
        }
    });
    let mut state = copilot_core::agent::RunState::new(Task::Text(
        "Trag meinen Namen und meine Versichertennummer in das Formular ein. Nicht abschicken.".into(),
    ));
    let cancel = CancellationToken::new();
    // The fake screen never changes, so look at the first two steps only.
    for _ in 0..2 {
        let r = agent.step(&mut state, &cancel).await.unwrap();
        eprintln!("step -> {r:?}; actions so far {:?}", platform.actions());
        if r != copilot_core::agent::StepResult::Continue {
            break;
        }
    }
    let acts = platform.actions();
    let clicks: Vec<PhysPoint> = acts
        .iter()
        .filter_map(|a| a.strip_prefix("click "))
        .map(|s| {
            let (x, y) = s.split_once(',').unwrap();
            PhysPoint { x: x.parse().unwrap(), y: y.parse().unwrap() }
        })
        .collect();
    let typed = platform.typed();
    eprintln!("clicks {clicks:?} typed {typed:?} tokens {}", state.tokens);
    assert!(typed.contains("Erika Beispiel"), "name typed");
    assert!(clicks.iter().any(|p| NAME_FIELD.contains(*p)), "clicked into the name field");
    assert!(!clicks.iter().any(|p| SEND_BUTTON.contains(*p)), "never clicked Absenden");
}

#[tokio::test]
#[ignore]
async fn live_send_needs_approval() {
    let Some(png) = live_png() else { return };
    let (agent, platform, bus) = live_agent(png, false);
    let mut rx = bus.to_ui.subscribe();
    let pending = bus.pending.clone();
    let cards = Arc::new(Mutex::new(Vec::new()));
    let c2 = cards.clone();
    tokio::spawn(async move {
        while let Ok(ev) = rx.recv().await {
            if let UiEvent::ApprovalRequest { card } = ev {
                eprintln!("card: {} | {} | {}", card.title, card.what, card.to);
                pending.resolve_approval(card.id, ApprovalAnswer::No);
                c2.lock().unwrap().push(card);
            }
        }
    });
    let out = agent
        .run(Task::Text("Das Formular ist fertig. Klick auf Absenden.".into()), CancellationToken::new())
        .await
        .unwrap();
    eprintln!("outcome {out:?} actions {:?}", platform.actions());
    assert_eq!(cards.lock().unwrap().len(), 1, "approval card shown");
    assert_eq!(out, AgentOutcome::Stopped);
    assert!(platform.clicks().is_empty(), "no click after 'Noch nicht'");
}

#[tokio::test]
async fn ui_tree_finds_the_named_button_without_sam() {
    // Live miss 26.10.: SAM found no box for „Datei auswählen“ and Spark's point was 50 px off.
    let mut h = Harness::new(false).await;
    h.platform.set_ui(PhysRect { x: 120, y: 590, w: 180, h: 40 }, "Datei auswählen", false);
    h.agent_turns(vec![calls("r1", &[click("den Knopf „Datei auswählen“", true)]), calls("r2", &[done("ok")])]);
    h.auto_answer(ApprovalAnswer::Yes, "");
    h.run("Häng es an").await;
    assert_eq!(h.platform.clicks(), vec!["click 210,610"]);
    let c = cards(&h.drain());
    assert!(c[0].title.contains("anhängen") && c[0].crop_png_b64.is_some());
    let log = h.memory.recent_actions(20).unwrap();
    assert!(log.iter().any(|a| a.tool == "ground" && a.args_redacted == "uia"), "{log:?}");
}

#[tokio::test]
async fn space_on_a_focused_button_and_typing_into_a_button_are_caught() {
    let mut h = Harness::new(false).await;
    *h.platform.focus.lock().unwrap() =
        Some(UiElement { name: "Weiter".into(), control: "Button".into(), is_password: false });
    let space = ("key", json!({"combo": "space", "is_commit": false, "commit_what": null, "commit_to": null}));
    h.agent_turns(vec![
        calls("r1", &[("type_text", json!({"text": "x", "what": "etwas", "into_sam_query": null}))]),
        calls("r2", &[space]),
    ]);
    h.auto_answer(ApprovalAnswer::No, "");
    assert_eq!(h.run("Mach weiter").await, AgentOutcome::Stopped);
    assert_eq!(h.platform.typed(), "", "no typing into a button");
    assert!(!h.platform.actions().contains(&"key space".to_string()));
    assert_eq!(cards(&h.drain()).len(), 1, "space on a focused button needs a card");
    let reqs = h.requests();
    assert!(outputs(&reqs[1])[0].starts_with("refused: the keyboard focus is on a Button"), "{:?}", outputs(&reqs[1]));
}

#[tokio::test]
async fn remembering_a_new_address_needs_a_yes() {
    let h = Harness::new(false).await;
    h.agent_turns(vec![
        calls("r1", &[("remember_fact", json!({"key": "hobby", "value": "Garten"}))]),
        calls("r2", &[("remember_fact", json!({"key": "kasse_mail", "value": "post@gesundkasse.example"}))]),
        calls("r3", &[done("ok")]),
    ]);
    h.auto_answer(ApprovalAnswer::Yes, "");
    h.run("Merk dir das").await;
    assert_eq!(h.memory.profile_get("hobby").unwrap().as_deref(), Some("Garten"));
    assert_eq!(h.memory.profile_get("kasse_mail").unwrap().as_deref(), Some("post@gesundkasse.example"));
    let log = h.memory.recent_actions(20).unwrap();
    assert!(log.iter().any(|a| a.tool == "remember_fact" && a.approved == Some(true)));
}

#[tokio::test]
async fn open_url_asks_for_unknown_hosts_once() {
    let h = Harness::new(false).await;
    h.agent_turns(vec![
        calls("r1", &[("open_url", json!({"url": "https://www.gesundkasse.example/kontakt"}))]),
        calls("r2", &[("open_url", json!({"url": "https://www.gesundkasse.example/upload"}))]),
        calls("r3", &[("open_url", json!({"url": "http://localhost:8000/"}))]),
        calls("r4", &[done("ok")]),
    ]);
    let mut rx = h.bus.to_ui.subscribe();
    h.auto_answer(ApprovalAnswer::Yes, "");
    h.run("Öffne die Kasse").await;
    assert_eq!(h.opened.lock().unwrap().len(), 3);
    let mut n = 0;
    while let Ok(ev) = rx.try_recv() {
        if let UiEvent::ApprovalRequest { card } = ev {
            n += 1;
            assert_eq!(card.title, "Darf ich diese Seite öffnen?");
            assert!(card.what.contains("www.gesundkasse.example"));
        }
    }
    assert_eq!(n, 1, "one card for the new host, none for the same host or localhost");
}

#[tokio::test]
async fn lost_conversation_starts_fresh_with_history() {
    let h = Harness::new(true).await;
    *h.script.lose_conversation.lock().unwrap() = 1;
    h.agent_turns(vec![
        calls("r1", &[("type_text", json!({"text": "Erika Beispiel", "what": "deinen Namen", "into_sam_query": null}))]),
        calls("r2", &[done("Fertig.")]),
    ]);
    let outcome = h.run("Trag meinen Namen ein").await;
    assert!(matches!(outcome, AgentOutcome::Done(_)), "{outcome:?}");
    let reqs = h.requests();
    let last = reqs.last().unwrap();
    assert!(last["previous_response_id"].is_null(), "fresh conversation after the loss");
    let text = last["input"].to_string();
    assert!(text.contains("EARLIER IN THIS TASK") && text.contains("deinen Namen"), "{text}");
    assert_eq!(h.platform.typed(), "Erika Beispiel", "nothing typed twice");
}
