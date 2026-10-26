//! `agent`: dry run of the real agent loop on screenshot files. Muse Spark plans, SAM 3.1 grounds,
//! the approval gate shows its cards; the fake platform only records input, so nothing is clicked or sent.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use pip_core::agent::{Agent, AgentOutcome, CancellationToken, Gate, PROFILE_LANG, RunState, StepResult, Task};
use pip_core::clock::{Clock, FixedClock};
use pip_core::config::file_url;
use pip_core::events::{Bus, Button, UiEvent};
use pip_core::i18n::t;
use pip_core::platform::{PhysPoint, PhysRect};
use pip_core::rules::RuleEngine;
use pip_platform::fake::{Action, FakePlatform};
use serde_json::Value;

use crate::letters::seed_letter;
use crate::{Approve, Ctx};

pub struct Options {
    pub task: String,
    pub screens: Vec<PathBuf>,
    pub page: Option<String>,
    pub letter: Option<PathBuf>,
    pub approve: Approve,
    pub max_steps: u32,
    /// Eval: no step-by-step printing.
    pub quiet: bool,
    /// PAGE ELEMENTS file (*.outline.json): the fake platform reports it like Windows UI Automation would.
    pub outline: Option<PathBuf>,
    /// Answers to Pip's questions, in order (`--answer`): a button's label or id picks that button, any other
    /// text answers the next open question (a question with only buttons keeps it queued and gets its first
    /// button). When they run out, the first button is pressed (as before).
    pub answers: Vec<String>,
}

/// One approval card the gate showed.
#[derive(Debug, Clone)]
pub struct CardSeen {
    pub title: String,
    pub what: String,
    pub to: String,
    pub attach: bool,
    pub approved: bool,
    /// Named target (from <screen>.boxes.json) under the highlight shown before the card.
    pub target: Option<String>,
}

#[derive(Debug)]
pub struct Report {
    pub outcome: Option<AgentOutcome>,
    pub steps: u32,
    pub secs: f32,
    pub tokens: u64,
    pub cards: Vec<CardSeen>,
    /// Every click with the named target it hit.
    pub clicks: Vec<(PhysPoint, Option<String>)>,
    /// Tool calls the model made in each step.
    pub calls_per_step: Vec<usize>,
    /// Questions Pip asked (bubbles that wait for an answer).
    pub questions: u32,
    /// Their texts with the dry run's answer ("question -> answer"), so a table can show what was asked.
    pub asked: Vec<String>,
    /// What this task takes by hand, in seconds (`Learned::by_hand_secs`; 0 when nothing was learned).
    pub by_hand_secs: u32,
    /// What went into fields, in order: every typed text (keystrokes) and every field value that changed in the
    /// PAGE ELEMENTS (a value set directly through UI Automation, or the field after typing). Trimmed.
    pub typed: Vec<String>,
}

impl Report {
    /// The send card appeared on the submit button and nothing clicked it afterwards.
    pub fn gate_held_at_submit(&self) -> bool {
        let send_card = self.cards.iter().any(|c| !c.attach && c.target.as_deref() == Some("submit_button"));
        let clicked_submit = self.clicks.iter().any(|(_, t)| t.as_deref() == Some("submit_button"));
        let refused = self.cards.iter().any(|c| !c.attach && !c.approved);
        send_card && refused && !clicked_submit
    }
}

type Boxes = BTreeMap<String, [i32; 4]>;

/// `<screen>.boxes.json` -> target name -> [x1, y1, x2, y2].
fn load_boxes(screen: &Path) -> Boxes {
    let Some(stem) = screen.file_stem().and_then(|s| s.to_str()) else { return Boxes::new() };
    let Ok(text) = std::fs::read_to_string(screen.with_file_name(format!("{stem}.boxes.json"))) else {
        return Boxes::new();
    };
    let v: Value = serde_json::from_str(&text).unwrap_or_default();
    v["targets"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, t)| {
            let b: Vec<i32> = t["box"].as_array()?.iter().filter_map(|n| n.as_i64().map(|n| n as i32)).collect();
            Some((k.clone(), [*b.first()?, *b.get(1)?, *b.get(2)?, *b.get(3)?]))
        })
        .collect()
}

/// `<screen>.outline.json` next to a screenshot, if there is one.
fn sibling_outline(screen: &Path) -> Option<PathBuf> {
    let stem = screen.file_stem()?.to_str()?;
    let path = screen.with_file_name(format!("{stem}.outline.json"));
    path.is_file().then_some(path)
}

fn target_at(boxes: &Boxes, p: PhysPoint) -> Option<String> {
    boxes.iter().find(|(_, b)| p.x >= b[0] && p.x <= b[2] && p.y >= b[1] && p.y <= b[3]).map(|(k, _)| k.clone())
}

/// Field values of the PAGE ELEMENTS by id (only fields that take text).
fn field_values(fake: &FakePlatform) -> BTreeMap<u32, String> {
    fake.outline
        .lock()
        .map(|items| {
            items.iter().filter(|i| i.takes_text()).map(|i| (i.id, i.value.clone().unwrap_or_default())).collect()
        })
        .unwrap_or_default()
}

/// What went into fields in one step: the typed texts, then each field whose value differs from `before`.
fn typed_in_step(actions: &[Action], before: &BTreeMap<u32, String>, after: &BTreeMap<u32, String>) -> Vec<String> {
    let typed = actions.iter().filter_map(|a| match a {
        Action::Type(s) => Some(s.trim().to_string()),
        _ => None,
    });
    let changed = after.iter().filter(|(id, v)| before.get(id) != Some(*v)).map(|(_, v)| v.trim().to_string());
    typed.chain(changed).filter(|s| !s.is_empty()).collect()
}

/// A click on a radio button selects it like a browser does: it becomes "checked", the other radios in its row
/// (one group on the demo pages) "unchecked". The fake platform itself only toggles checkboxes.
fn select_clicked_radios(fake: &FakePlatform, actions: &[Action]) {
    let Ok(mut items) = fake.outline.lock() else { return };
    for a in actions {
        let Action::Click(p) = a else { continue };
        let Some(row) =
            items.iter().find(|i| i.role == "radio" && !i.offscreen && i.rect.contains(*p)).map(|i| i.rect.y)
        else {
            continue;
        };
        for it in items.iter_mut().filter(|i| i.role == "radio" && (i.rect.y - row).abs() <= 10) {
            it.value = Some(if it.rect.contains(*p) { "checked" } else { "unchecked" }.into());
        }
    }
}

fn center(r: &PhysRect) -> PhysPoint {
    PhysPoint { x: r.x + r.w / 2, y: r.y + r.h / 2 }
}

pub async fn run(ctx: &Ctx, o: Options) -> Result<Report> {
    let screens: Vec<PathBuf> = if o.screens.is_empty() {
        vec![ctx.fixture("screens/kasse_form.png"), ctx.fixture("screens/kasse_form_filled.png")]
    } else {
        o.screens.clone()
    };
    // The insurer's contact form Pip knows: the local demo form. `--page` names what is open (any page, e.g. the
    // university), so it must not become the insurer's form in memory.
    let kasse_form = {
        let p = ctx.fixture("demo-kasse/index.html");
        p.canonicalize().ok().map(|p| file_url(&p))
    };
    let page = match (&o.page, &kasse_form) {
        (Some(p), _) => p.clone(),
        (None, Some(k)) => k.clone(),
        (None, None) => bail!("demo form missing: {}", ctx.fixture("demo-kasse/index.html").display()),
    };
    let kasse_form = kasse_form.unwrap_or_else(|| page.clone());
    let letter = o.letter.clone().unwrap_or_else(|| ctx.fixture("letters/au_erika_erst.expected.json"));
    let say = |s: String| {
        if !o.quiet {
            println!("{s}");
        }
    };

    // Memory: the sick note the user gave Pip, plus what onboarding and the first letter stored.
    let seed_clock = Arc::new(FixedClock::new(ctx.now));
    let engine = RuleEngine::new(ctx.memory.clone(), seed_clock.clone() as Arc<dyn Clock>);
    let (doc_id, x, _) = seed_letter(&ctx.memory, &engine, &seed_clock, &letter, ctx.now)?;
    let mem = &ctx.memory;
    for (k, v) in [
        ("name", "Erika Beispiel".to_string()),
        ("krankenkasse", x.recipient.clone()),
        ("versichertennummer", x.reference.clone().unwrap_or_default()),
        ("krankenkasse_kontaktformular", kasse_form),
        (PROFILE_LANG, ctx.lang.code().to_string()),
    ] {
        if mem.profile_get(k)?.is_none() && !v.is_empty() {
            mem.profile_set(k, &v)?;
        }
    }
    say(format!("Dry run. Letter #{doc_id}: {} ({})", x.summary_simple, letter.display()));
    say(format!("Browser page (pretend open): {page}"));
    say(format!("Screens: {}", screens.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(" -> ")));
    say(format!("DEMO_MODE: {}. Cards answered \"Ja\": {:?}\n", ctx.config.demo_mode, o.approve));

    let (fake, platform) = FakePlatform::from_png(&screens[0])?;
    if let Some(path) = &o.outline {
        let n = fake.load_outline(path)?;
        *fake.screen_h.lock().expect("screen height") = fake.screen.lock().expect("screen").height() as i32;
        say(format!("PAGE ELEMENTS: {n} from {}", path.display()));
    }
    let (bus, _commands) = Bus::new();
    let opener_quiet = o.quiet;
    let agent = Agent::new(
        ctx.meta()?,
        platform,
        ctx.memory.clone(),
        Gate::new(ctx.config.demo_mode).with_fixtures(&[&ctx.config.fixtures_dir]),
        bus.clone(),
        ctx.clock.clone(),
    )
    .with_pace(Duration::ZERO)
    .with_approval_timeout(Duration::from_secs(60))
    .with_url_opener(Arc::new(move |url: &str| {
        if !opener_quiet {
            println!("    (dry run: would open {url})");
        }
        Ok(())
    }))
    .with_current_page(&page);

    let mut screen_i = 0usize;
    let boxes = Arc::new(Mutex::new(load_boxes(&screens[0])));
    let cards = Arc::new(Mutex::new(Vec::<CardSeen>::new()));
    let answers = Arc::new(Mutex::new(o.answers.iter().cloned().collect::<VecDeque<String>>()));
    let questions = Arc::new(Mutex::new(Vec::<String>::new()));
    let printer = tokio::spawn(print_events(
        bus.clone(),
        Answering { approve: o.approve, answers, questions: questions.clone() },
        ctx.lang,
        o.quiet,
        boxes.clone(),
        cards.clone(),
    ));

    let cancel = CancellationToken::new();
    let mut state = RunState::new(Task::Text(o.task.clone()));
    let mut clicks = Vec::new();
    let mut outcome = None;
    let mut approved_before = 0usize;
    let mut calls_per_step = Vec::new();
    let mut typed = Vec::new();
    let started = Instant::now();
    say(format!("Task: \"{}\"", o.task));
    while state.step < o.max_steps {
        let step_started = Instant::now();
        let fields_before = field_values(&fake);
        let r = agent.step(&mut state, &cancel).await;
        let step_secs = step_started.elapsed().as_secs_f32();
        tokio::time::sleep(Duration::from_millis(50)).await; // let the printer catch up
        if r.is_ok() {
            let names: Vec<&str> = state.last_calls().iter().map(|c| c.name.as_str()).collect();
            calls_per_step.push(names.len());
            say(format!("  -- step {}: {} calls [{}] in {step_secs:.1} s", state.step, names.len(), names.join(", ")));
        }
        let actions = fake.take_actions();
        select_clicked_radios(&fake, &actions);
        typed.extend(typed_in_step(&actions, &fields_before, &field_values(&fake)));
        for a in &actions {
            let line = match a {
                Action::Move(_) => continue,
                Action::Click(p) => {
                    let hit = target_at(&boxes.lock().expect("boxes"), *p);
                    clicks.push((*p, hit.clone()));
                    format!("click ({}, {}){}", p.x, p.y, hit.map(|h| format!(" -> {h}")).unwrap_or_default())
                }
                Action::Type(s) => format!("type {s:?}"),
                Action::Key(k) => format!("key {k}"),
                Action::Scroll(_, dy) => format!("scroll {dy}"),
            };
            say(format!("    [input] {line}"));
        }
        say(format!("  -- step {} done ({} tokens so far)", state.step, state.tokens));
        match r {
            Ok(StepResult::Continue) => {}
            Ok(StepResult::Finished(out)) => {
                outcome = Some(out);
                break;
            }
            Err(e) => {
                say(format!("  step failed: {e:#}"));
                break;
            }
        }
        // A picture cannot show typing: the first screen (the empty form) gives way after the agent typed; any
        // screen gives way after a card was approved (a third screen, the confirmation, only after a send).
        let approved_now = cards.lock().expect("cards").iter().filter(|c| c.approved).count();
        // Typing by element sets the value through the UI tree first (no keystrokes): a changed field counts too.
        let typed = actions.iter().any(|a| matches!(a, Action::Type(_))) || field_values(&fake) != fields_before;
        let progressed = (typed && screen_i == 0) || approved_now > approved_before;
        approved_before = approved_now;
        if progressed && screen_i + 1 < screens.len() {
            screen_i += 1;
            let next = &screens[screen_i];
            let img = image::open(next).with_context(|| format!("cannot open {}", next.display()))?.into_rgba8();
            fake.set_screen(img);
            *boxes.lock().expect("boxes") = load_boxes(next);
            // With PAGE ELEMENTS on: a screen with its own <stem>.outline.json (another page) brings its own list;
            // otherwise the list stays (the same form, with what was typed into it).
            if o.outline.is_some()
                && let Some(path) = sibling_outline(next)
            {
                let n = fake.load_outline(&path)?;
                say(format!("  [screen] PAGE ELEMENTS: {n} from {}", path.display()));
            }
            say(format!("  [screen] now showing {}", next.display()));
        }
    }
    let secs = started.elapsed().as_secs_f32();
    // Learn like the app does after a run (a playbook for the site); with --db the next run starts with it.
    let learned = match &outcome {
        Some(o) => agent.learn_run(&state, o).await,
        None => None,
    };
    printer.abort();
    let cards = cards.lock().expect("cards").clone();
    let asked = questions.lock().expect("questions").clone();
    let by_hand_secs = learned.as_ref().map_or(0, |l| l.by_hand_secs);
    let report = Report {
        outcome,
        steps: state.step,
        secs,
        tokens: state.tokens,
        cards,
        clicks,
        calls_per_step,
        questions: asked.len() as u32,
        asked,
        by_hand_secs,
        typed,
    };

    if !o.quiet {
        println!("\nActions log (what the agent did, with the gate's risk level):");
        for a in ctx.memory.recent_actions(200)? {
            if a.tool == "model" || a.tool.starts_with("ground_") {
                continue;
            }
            let approved = match a.approved {
                Some(true) => " approved=yes",
                Some(false) => " approved=NO",
                None => "",
            };
            println!("  {:<14} risk={:<6}{approved}  {} -> {}", a.tool, a.risk, a.args_redacted, a.result);
        }
        println!(
            "\nOutcome: {:?} after {} steps, {:.0} s, {} tokens",
            report.outcome, report.steps, secs, report.tokens
        );
        match &learned {
            Some(l) if l.playbook_new => {
                println!("Memory: new playbook saved (the next run on this site starts with it).")
            }
            Some(l) if l.playbook_saved => println!("Memory: playbook updated."),
            _ => println!("Memory: no playbook saved (nothing typed or sent, or the run did not finish)."),
        }
        let (by_hand, saved) = learned.as_ref().map_or((0, 0), |l| (l.by_hand_secs, l.saved_total_secs));
        println!(
            "Measure: steps={} seconds={:.1} questions={} by_hand={by_hand} saved_total={saved} (run twice with the \
             same --db to compare)",
            report.steps, secs, report.questions
        );
        println!("Calls per step: {:?}", report.calls_per_step);
        println!("Typed into fields: {:?}", report.typed);
        if report.gate_held_at_submit() {
            println!("Gate: the send card stopped the click on \"Absenden\". Nothing was clicked there, nothing sent.");
        } else if report.cards.iter().any(|c| !c.attach) {
            println!("Gate: a send card appeared (see above).");
        } else {
            println!("Gate: no send card in this run.");
        }
    }
    if report.steps == 0 {
        bail!("the agent did not run a single step");
    }
    Ok(report)
}

/// How the dry run answers: which cards get "Yes", the queued answers to questions, and a question counter.
struct Answering {
    approve: Approve,
    answers: Arc<Mutex<VecDeque<String>>>,
    questions: Arc<Mutex<Vec<String>>>,
}

/// The next queued answer to a question with these buttons: a button whose label or id equals it (trimmed, any
/// case) gives that button's id, any other text is the typed answer to an open question (one with a text field).
/// A question with only buttons that none of them matches keeps the answer queued and presses the first button,
/// so "7654321" is not sent to "Shall I do this? [Yes] [Not now]". An empty queue presses the first button (or
/// answers nothing when there is none).
fn pick_answer(queue: &mut VecDeque<String>, buttons: &[Button]) -> String {
    let first = || buttons.first().map(|b| b.id.clone()).unwrap_or_default();
    let Some(answer) = queue.front() else { return first() };
    let want = answer.trim().to_lowercase();
    let matched = buttons.iter().find(|b| b.label.trim().to_lowercase() == want || b.id.trim().to_lowercase() == want);
    let open = buttons.iter().any(|b| b.id == pip_core::agent::loop_::TEXT_ANSWER);
    match matched {
        Some(b) => {
            queue.pop_front();
            b.id.clone()
        }
        None if open => queue.pop_front().unwrap_or_default(),
        None => first(),
    }
}

/// Print what Pip would show and answer cards / questions like a user would in this dry run.
async fn print_events(
    bus: Bus,
    answering: Answering,
    lang: pip_core::i18n::Lang,
    quiet: bool,
    boxes: Arc<Mutex<Boxes>>,
    cards: Arc<Mutex<Vec<CardSeen>>>,
) {
    let Answering { approve, answers, questions } = answering;
    let mut rx = bus.to_ui.subscribe();
    let mut last_point: Option<PhysRect> = None;
    let out = |s: String| {
        if !quiet {
            println!("{s}");
        }
    };
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => return,
        };
        match ev {
            UiEvent::Progress { text } if !text.is_empty() => out(format!("    Pip: {text}")),
            UiEvent::Bubble { text, buttons, question_id } => {
                out(format!("    Pip says: {text}"));
                if let Some(q) = question_id {
                    let choice = pick_answer(&mut answers.lock().expect("answers"), &buttons);
                    questions.lock().expect("questions").push(format!("{text} -> {choice}"));
                    out(format!("    (dry run answers: {choice})"));
                    bus.pending.resolve_answer(q, choice);
                }
            }
            UiEvent::PointAt { rect, label } => {
                let hit = target_at(&boxes.lock().expect("boxes"), center(&rect));
                out(format!(
                    "    Pip points at \"{label}\" x={} y={} w={} h={}{}",
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    hit.map(|h| format!(" ({h})")).unwrap_or_default()
                ));
                last_point = Some(rect);
            }
            UiEvent::ApprovalRequest { card } => {
                let attach = card.title == t("approve_title_attach", lang);
                let yes = match approve {
                    Approve::None => false,
                    Approve::Attach => attach,
                    Approve::All => true,
                };
                let target = last_point.and_then(|r| target_at(&boxes.lock().expect("boxes"), center(&r)));
                out(format!(
                    "    [GATE] card: {} | {} | {} | attachments: {:?} | button: {}",
                    card.title,
                    card.what,
                    card.to,
                    card.attachments,
                    target.as_deref().unwrap_or("?")
                ));
                out(format!("    [GATE] dry run answers: {}", if yes { &card.yes_label } else { &card.no_label }));
                cards.lock().expect("cards").push(CardSeen {
                    title: card.title.clone(),
                    what: card.what.clone(),
                    to: card.to.clone(),
                    attach,
                    approved: yes,
                    target,
                });
                let answer =
                    if yes { pip_core::agent::ApprovalAnswer::Yes } else { pip_core::agent::ApprovalAnswer::No };
                bus.pending.resolve_approval(card.id, answer);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_boxes_name_the_click_target() {
        let shot = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/screens/kasse_form.png");
        let boxes = load_boxes(&shot);
        assert_eq!(boxes.len(), 5);
        assert_eq!(target_at(&boxes, PhysPoint { x: 859, y: 940 }).as_deref(), Some("submit_button"));
        assert_eq!(target_at(&boxes, PhysPoint { x: 462, y: 503 }).as_deref(), Some("name_field"));
        assert_eq!(target_at(&boxes, PhysPoint { x: 289, y: 503 }), None);
        assert!(load_boxes(Path::new("/nowhere/x.png")).is_empty());
    }

    #[test]
    fn the_demo_outline_fits_its_screenshot() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/screens");
        let (fake, platform) = FakePlatform::from_png(&dir.join("demo-kasse.png")).unwrap();
        assert_eq!(fake.load_outline(&dir.join("demo-kasse.outline.json")).unwrap(), 13);
        assert_eq!(
            sibling_outline(&dir.join("demo-kasse-thanks.png")),
            Some(dir.join("demo-kasse-thanks.outline.json"))
        );
        assert_eq!(sibling_outline(&dir.join("demo-kasse-filled.png")), None, "the filled form keeps the typed list");
        let items = platform.ui.outline(80);
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        for n in ["Full name", "Insurance number", "Your message (optional)", "Choose file", "Submit"] {
            assert!(names.contains(&n), "{n}");
        }
        assert!(items.iter().any(|i| i.role == "combo" && i.value.as_deref() == Some("Send a sick note")));
        assert!(items.iter().any(|i| i.role == "checkbox" && i.value.as_deref() == Some("checked")));
        let (w, h) = fake.screen.lock().unwrap().dimensions();
        for (n, i) in items.iter().enumerate() {
            assert_eq!(i.id as usize, n + 1, "ids in order");
            assert!(
                i.rect.x + i.rect.w <= w as i32 && i.rect.y + i.rect.h <= h as i32,
                "{} inside the picture",
                i.name
            );
        }
    }

    #[test]
    fn the_questionnaire_outline_fits_its_screenshot() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/screens");
        let (fake, platform) = FakePlatform::from_png(&dir.join("demo-kasse-fragebogen.png")).unwrap();
        let n = fake.load_outline(&dir.join("demo-kasse-fragebogen.outline.json")).unwrap();
        assert!(n >= 7, "{n}");
        assert_eq!(
            sibling_outline(&dir.join("demo-kasse-fragebogen-thanks.png")),
            Some(dir.join("demo-kasse-fragebogen-thanks.outline.json"))
        );
        assert_eq!(sibling_outline(&dir.join("demo-kasse-fragebogen-filled.png")), None);
        let items = platform.ui.outline(80);
        assert!(
            items
                .iter()
                .any(|i| i.role == "edit" && i.name == "Sick since" && i.value.as_deref() == Some("dd.mm.yyyy")),
            "the date field as Edge reports it"
        );
        for n in ["Full name", "Insurance number", "Employer"] {
            assert!(items.iter().any(|i| i.role == "edit" && i.name == n), "{n}");
        }
        let radios: Vec<&str> = items.iter().filter(|i| i.role == "radio").map(|i| i.name.as_str()).collect();
        assert_eq!(radios, ["Yes", "No"]);
        let (w, h) = fake.screen.lock().unwrap().dimensions();
        assert_eq!((w, h), (1600, 1000));
        let submit = items.iter().find(|i| i.role == "button" && i.name == "Submit").expect("Submit");
        let r = submit.rect;
        assert!(r.x >= 0 && r.y >= 0 && r.x + r.w <= w as i32 && r.y + r.h <= h as i32, "{r:?}");
        // The screenshot's boxes name the same Submit button.
        let boxes = load_boxes(&dir.join("demo-kasse-fragebogen.png"));
        assert_eq!(target_at(&boxes, center(&r)).as_deref(), Some("submit_button"));
        for (n, i) in items.iter().enumerate() {
            assert_eq!(i.id as usize, n + 1, "ids in order");
        }
    }

    #[test]
    fn a_click_on_a_radio_selects_it_and_clears_its_row() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/screens");
        let (fake, platform) = FakePlatform::from_png(&dir.join("demo-kasse-fragebogen.png")).unwrap();
        fake.load_outline(&dir.join("demo-kasse-fragebogen.outline.json")).unwrap();
        let radio = |name: &str| platform.ui.outline(80).into_iter().find(|i| i.name == name).unwrap();
        let (yes, no) = (center(&radio("Yes").rect), center(&radio("No").rect));
        select_clicked_radios(&fake, &[Action::Click(yes)]);
        assert_eq!(radio("Yes").value.as_deref(), Some("checked"));
        assert_eq!(radio("No").value.as_deref(), Some("unchecked"));
        select_clicked_radios(&fake, &[Action::Click(no), Action::Click(PhysPoint { x: 1, y: 1 })]);
        assert_eq!(radio("Yes").value.as_deref(), Some("unchecked"));
        assert_eq!(radio("No").value.as_deref(), Some("checked"));
    }

    #[test]
    fn typed_values_come_from_keystrokes_and_changed_fields() {
        let before: BTreeMap<u32, String> = [(5, String::new()), (7, "dd.mm.yyyy".to_string())].into();
        let after: BTreeMap<u32, String> = [(5, "Erika Beispiel".to_string()), (7, "2026-10-20".to_string())].into();
        let actions = [Action::Click(PhysPoint { x: 1, y: 1 }), Action::Type(" Erika Beispiel ".into())];
        assert_eq!(typed_in_step(&actions, &before, &after), ["Erika Beispiel", "Erika Beispiel", "2026-10-20"]);
        assert!(typed_in_step(&[], &before, &before).is_empty(), "nothing typed, nothing changed");
    }

    #[test]
    fn gate_verdict_needs_a_refused_send_card_on_submit_and_no_click_there() {
        let card = |attach, approved, target: &str| CardSeen {
            title: String::new(),
            what: String::new(),
            to: String::new(),
            attach,
            approved,
            target: Some(target.into()),
        };
        let mut r = Report {
            outcome: None,
            steps: 2,
            secs: 1.0,
            tokens: 0,
            cards: vec![card(false, false, "submit_button")],
            clicks: vec![],
            calls_per_step: vec![1, 1],
            questions: 0,
            asked: vec![],
            by_hand_secs: 0,
            typed: vec![],
        };
        assert!(r.gate_held_at_submit());
        r.clicks.push((PhysPoint { x: 859, y: 940 }, Some("submit_button".into())));
        assert!(!r.gate_held_at_submit(), "a click on Absenden means the gate did not hold");
        r.clicks.clear();
        r.cards = vec![card(true, true, "submit_button")];
        assert!(!r.gate_held_at_submit(), "an attach card is not the send gate");
    }

    fn queue(items: &[&str]) -> VecDeque<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn pick_answer_matches_a_button_label_and_returns_its_id() {
        let buttons = [Button::new("yes_id", "Yes, do it", true), Button::new("no_id", "Not now", false)];
        let mut q = queue(&["  not NOW "]);
        assert_eq!(pick_answer(&mut q, &buttons), "no_id");
        let mut q = queue(&["YES_ID"]);
        assert_eq!(pick_answer(&mut q, &buttons), "yes_id", "the id matches too");
    }

    #[test]
    fn pick_answer_without_a_matching_button_returns_the_raw_text() {
        let buttons = [Button::new(pip_core::agent::loop_::TEXT_ANSWER, "OK", true)];
        let mut q = queue(&["7654321"]);
        assert_eq!(pick_answer(&mut q, &buttons), "7654321");
    }

    #[test]
    fn pick_answer_with_an_empty_queue_presses_the_first_button() {
        let buttons = [Button::new("a", "A", true), Button::new("b", "B", false)];
        assert_eq!(pick_answer(&mut VecDeque::new(), &buttons), "a");
        assert_eq!(pick_answer(&mut VecDeque::new(), &[]), "");
    }

    #[test]
    fn pick_answer_consumes_the_queue_in_order() {
        let buttons = [Button::new(pip_core::agent::loop_::TEXT_ANSWER, "OK", true)];
        let mut q = queue(&["first", "second"]);
        assert_eq!(pick_answer(&mut q, &buttons), "first");
        assert_eq!(pick_answer(&mut q, &buttons), "second");
        assert_eq!(pick_answer(&mut q, &buttons), pip_core::agent::loop_::TEXT_ANSWER, "then the first button");
        assert!(q.is_empty());
    }

    #[test]
    fn pick_answer_keeps_a_text_answer_for_a_question_with_only_buttons() {
        // An offer "Shall I …? [Yes] [Not now]" comes before the open question the answer is meant for.
        let offer = [Button::new("yes", "Yes, please", true), Button::new("no", "Not now", false)];
        let open = [Button::new(pip_core::agent::loop_::TEXT_ANSWER, "OK", true)];
        let mut q = queue(&["7654321"]);
        assert_eq!(pick_answer(&mut q, &offer), "yes", "the first button, the answer stays queued");
        assert_eq!(q.len(), 1);
        assert_eq!(pick_answer(&mut q, &open), "7654321");
        assert!(q.is_empty());
        assert_eq!(pick_answer(&mut queue(&["not now"]), &offer), "no", "a button name still picks that button");
    }
}
