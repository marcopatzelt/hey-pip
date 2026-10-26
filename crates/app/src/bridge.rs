//! Wiring: core (Bus, Agent, RuleEngine, DocPipeline) <-> Tauri (`ui` events, `ui_command`).
//! The page gets core events as "ui" and app-only events (acting, paused, drag, deadlines) as "app".

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use pip_core::agent::{AgentOutcome, CancellationToken, StopReason, Task, task_from_words};
use pip_core::docs::DocError;
use pip_core::docs::DocType;
use pip_core::events::{Bus, Button, ForgetKind, OnboardingStep, PipState, UiEvent, UserCommand};
use pip_core::i18n::{Lang, t, tf};
use pip_core::memory::{DeadlineStatus, Knowledge, untrust_site};
use pip_core::meta::MetaError;
use pip_core::rules::daily::{EV_ACCEPTED, EV_DECLINED, EV_SUGGESTED};
use pip_core::rules::text::fmt_date;
use pip_core::rules::{AfterTask, AfterTaskOffer};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::mpsc;

use crate::backend::{Backend, DEMO_SERVICES_KEY, env_flag, guard, guard_async};
use crate::overlay::{self, FocusKeeper, OverlayInfo};
use crate::voice::{Recorder, Silent, Speaker, TooShort, TtsSpeaker};
use crate::{mock, tray};

/// File types Pip can read.
const LETTER_EXT: &[&str] = &["png", "jpg", "jpeg", "webp", "pdf"];
/// After "Ja" on a card, the presenter's hand moves on: no mouse-move stop for this long.
const QUIET_AFTER_YES: Duration = Duration::from_secs(7);
/// After any click on Pip's own bubbles, cards or buttons the hand is still moving: no mouse-move stop.
const QUIET_AFTER_CLICK: Duration = Duration::from_secs(3);
/// How long "Rückgängig" stays after "Ich bin gesund" / "Erledigt".
const UNDO_SECS: u32 = 30;
/// Strings the page asks for once at start (everything it shows itself).
const UI_KEYS: &[&str] = &[
    "onb_hello",
    "onb_ask_name",
    "onb_next",
    "onb_privacy",
    "onb_drop_letter",
    "btn_look_screen",
    "btn_read_letter",
    "btn_type_or_say",
    "task_look_screen",
    "onb_name_placeholder",
    "onb_ask_state",
    "onb_state_pick",
    "states_list",
    "btn_pick_letter",
    "btn_yes",
    "btn_elsewhere",
    "ask_what",
    "ask_placeholder",
    "btn_go",
    "btn_mic",
    "btn_close",
    "btn_ok",
    "thinking",
    "listening",
    "btn_stop",
    "btn_resume",
    "btn_why",
    "btn_retry",
    "btn_later",
    "btn_onb_later",
    "btn_done",
    "btn_other_photo",
    "approve_filled",
    "chip_send_au",
    "chip_last_letter",
    "chip_deadlines",
    "btn_pause_link",
    "queue_of",
    "setup_key_label",
    "setup_key_save",
    "approve_attachments",
    "approve_button_preview",
    "drop_hint",
    "deadlines_title",
    "deadlines_none",
    "hint_not_legal_advice",
    "tray_deadlines",
    "tray_knowledge",
    "btn_forget",
];

/// Profile keys that are Pip's own settings, not something about the person ("What Pip knows" hides them).
const SETTING_KEYS: &[&str] = &[
    "lang",
    "onboarded",
    "screen_notice_ok",
    "trusted_send_hosts",
    "krankenkasse_kontaktformular",
    "krankenkasse_services",
    "brief_shown_on",
    "seconds_saved_total",
];

/// After a finished task Pip waits this long before his offer, so the "Done." line can be read.
const OFFER_AFTER_SECS: u64 = 4;

/// An offer waiting for its button: the agent task behind "Yes", the heading (for the yes/no events) and the
/// reason (for "Why?").
#[derive(Debug, Clone, PartialEq)]
struct PendingOffer {
    task: String,
    heading: String,
    reason: String,
}

/// Buttons of the after-task offer; after "Why?" without a second "Why?".
fn offer_buttons(lang: Lang, with_why: bool) -> Vec<Button> {
    let mut b = vec![
        Button::new("app:offer_go", t("btn_sugg_yes", lang), true),
        Button::new("app:offer_no", t("btn_not_now", lang), false),
    ];
    if with_why {
        b.push(Button::new("app:offer_why", t("btn_sugg_why", lang), false));
    }
    b
}

/// App-only events for the page (not part of the core bus).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEvent {
    Acting {
        on: bool,
    },
    Paused {
        on: bool,
    },
    Drag {
        state: &'static str,
    },
    Deadlines {
        items: Vec<DeadlineItem>,
    },
    /// "What Pip knows": groups of rows, each with a Forget button.
    Knowledge {
        title: String,
        groups: Vec<KnowGroup>,
    },
    /// An error bubble that can always be closed; `retry` = what "Nochmal" sends (exactly the thing that failed),
    /// `pick` = offer "Anderes Foto wählen" instead.
    Error {
        text: String,
        retry: Option<UserCommand>,
        pick: bool,
    },
    /// Pip has no key yet: show the "Schlüssel einfügen" field.
    Setup {
        text: String,
    },
    /// A bubble whose buttons go away after `ttl_secs` (e.g. "Rückgängig").
    Timed {
        text: String,
        buttons: Vec<Button>,
        ttl_secs: u32,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct DeadlineItem {
    pub id: i64,
    pub title: String,
    pub due: String,
    /// "in 3 Tagen", "morgen", "heute", "vorbei"
    pub when: String,
    pub overdue: bool,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct KnowGroup {
    pub title: String,
    /// Shown when `items` is empty.
    pub empty: String,
    pub items: Vec<KnowItem>,
}

/// One row; Forget sends `{type:"forget", kind, id, task}`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct KnowItem {
    pub kind: ForgetKind,
    pub id: String,
    pub task: String,
    pub title: String,
    pub detail: String,
}

pub struct Ctx {
    app: AppHandle,
    bus: Bus,
    backend: Backend,
    speaker: Box<dyn Speaker>,
    windowed: bool,
    run: Mutex<Option<CancellationToken>>,
    last_task: Mutex<Option<Task>>,
    recorder: Mutex<Option<Recorder>>,
    listening: AtomicBool,
    paused: AtomicBool,
    /// Pip acts on the screen (agent or mock story running).
    acting: AtomicBool,
    /// An approval card or a question is open: the user needs the mouse, no mouse-move stop.
    waiting: AtomicBool,
    /// Read by the overlay poll: watch for a person moving the mouse. `PIP_MOUSE_STOP=0` turns it off.
    mouse_armed: Arc<AtomicBool>,
    mouse_quiet: Arc<Mutex<Option<Instant>>>,
    mouse_stop: bool,
    /// The task's words, for "Nochmal" after an error.
    last_text: Mutex<Option<String>>,
    focus: FocusKeeper,
    ready: AtomicBool,
    onboarding: Mutex<OnboardingStep>,
    tray: Mutex<Option<tray::Tray>>,
    ingest_lock: tokio::sync::Mutex<()>,
    started: Instant,
    /// The agent task behind "Yes, do it now" on the daily brief.
    brief_task: Mutex<Option<PendingOffer>>,
    /// The after-task offer behind [Yes, let's do it] [Not now] [Why?].
    offer_task: Mutex<Option<PendingOffer>>,
}

type Shared = Arc<Ctx>;

// ---------- Tauri commands ----------

/// JS: `invoke("ui_command", { cmd: { type: "stop" } })`.
#[tauri::command]
async fn ui_command(ctx: tauri::State<'_, Shared>, cmd: UserCommand) -> Result<(), String> {
    ctx.bus.from_ui.send(cmd).await.map_err(|e| e.to_string())
}

/// Page errors and warnings go to the log file (the Windows build has no dev tools).
#[tauri::command]
fn js_log(level: String, msg: String) {
    let msg: String = msg.chars().take(500).collect();
    if level == "error" {
        tracing::error!("page: {msg}");
    } else {
        tracing::warn!("page: {msg}");
    }
}

#[tauri::command]
fn overlay_info(ctx: tauri::State<'_, Shared>) -> OverlayInfo {
    ctx.screen()
}

#[tauri::command]
fn ui_strings(ctx: tauri::State<'_, Shared>) -> HashMap<&'static str, &'static str> {
    let lang = ctx.lang();
    UI_KEYS.iter().map(|k| (*k, t(k, lang))).collect()
}

/// The page is listening: start the first-run flow or greet.
#[tauri::command]
fn ui_ready(ctx: tauri::State<'_, Shared>) {
    if !ctx.ready.swap(true, Ordering::SeqCst) {
        ctx.inner().clone().start();
    }
}

/// Click on the sleeping Pip.
#[tauri::command]
fn wake(ctx: tauri::State<'_, Shared>) {
    if ctx.paused.load(Ordering::SeqCst) {
        ctx.set_paused(false);
    }
}

/// Text fields need keyboard focus; the overlay starts without it (and never takes it by a click).
#[tauri::command]
fn focus_window(ctx: tauri::State<'_, Shared>) {
    if let Some(w) = ctx.window() {
        ctx.focus.take(&w);
    }
}

/// The text field closed: the keyboard goes back to the window that had it.
#[tauri::command]
fn release_focus(ctx: tauri::State<'_, Shared>) {
    if let Some(w) = ctx.window() {
        ctx.focus.give_back(&w);
    }
}

/// Buttons with ids "app:…": pause, undo:<reminder>, last_letter, deadline_done:<id>.
#[tauri::command]
fn app_action(ctx: tauri::State<'_, Shared>, action: String) {
    ctx.quiet_mouse(QUIET_AFTER_CLICK);
    ctx.inner().clone().app_action(&action);
}

/// "Schlüssel einfügen": store the key next to Pip's data and restart. The key is never logged.
#[tauri::command]
fn save_key(ctx: tauri::State<'_, Shared>, key: String) -> Result<(), String> {
    let lang = ctx.lang();
    match pip_core::config::save_api_key(&key) {
        Ok(_) => {
            ctx.pip(PipState::Happy);
            ctx.say(t("setup_key_saved", lang), vec![]);
            let app = ctx.app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                app.restart();
            });
            Ok(())
        }
        Err(e) => {
            tracing::warn!("key not saved: {e}");
            Err(t("setup_key_bad", lang).to_string())
        }
    }
}

#[tauri::command]
fn open_deadlines(ctx: tauri::State<'_, Shared>) {
    ctx.show_deadlines();
}

#[tauri::command]
fn open_knowledge(ctx: tauri::State<'_, Shared>) {
    ctx.show_knowledge();
}

// ---------- entry ----------

pub fn run() -> Result<()> {
    let windowed = env_flag("PIP_WINDOWED");
    let (bus, commands) = Bus::new();
    let backend = Backend::init(&bus);
    let speaker: Box<dyn Speaker> = if env_flag("PIP_MUTE") { Box::new(Silent) } else { Box::new(TtsSpeaker::new()) };
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            ui_command,
            overlay_info,
            ui_strings,
            open_knowledge,
            ui_ready,
            wake,
            focus_window,
            release_focus,
            app_action,
            save_key,
            open_deadlines,
            js_log
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::DragDrop(dd) = event {
                on_drag_drop(window.app_handle(), dd);
            }
        })
        .setup(move |app| {
            let ctx: Shared = Arc::new(Ctx {
                app: app.handle().clone(),
                bus,
                backend,
                speaker,
                windowed,
                run: Mutex::new(None),
                last_task: Mutex::new(None),
                recorder: Mutex::new(None),
                listening: AtomicBool::new(false),
                paused: AtomicBool::new(false),
                acting: AtomicBool::new(false),
                waiting: AtomicBool::new(false),
                mouse_armed: Arc::new(AtomicBool::new(false)),
                mouse_quiet: Arc::new(Mutex::new(None)),
                last_text: Mutex::new(None),
                focus: FocusKeeper::default(),
                mouse_stop: std::env::var("PIP_MOUSE_STOP").map_or(true, |v| v.trim() != "0"),
                ready: AtomicBool::new(false),
                onboarding: Mutex::new(OnboardingStep::Done),
                tray: Mutex::new(None),
                ingest_lock: tokio::sync::Mutex::new(()),
                started: Instant::now(),
                brief_task: Mutex::default(),
                offer_task: Mutex::default(),
            });
            app.manage(ctx.clone());
            if let Some(win) = app.get_webview_window("pip") {
                let handle = app.handle().clone();
                let mouse_stop = overlay::MouseStop {
                    armed: ctx.mouse_armed.clone(),
                    quiet_until: ctx.mouse_quiet.clone(),
                    on_user_move: Box::new(move || {
                        tracing::info!("the user moved the mouse: stopping");
                        if let Some(ctx) = ctx_of(&handle) {
                            ctx.stop_by(StopReason::Mouse);
                        }
                    }),
                };
                overlay::setup(&win, ctx.bus.clone(), windowed, mouse_stop)?;
            }
            match tray::setup(app.handle(), ctx.lang()) {
                Ok(t) => *ctx.tray.lock().expect("tray lock") = Some(t),
                Err(e) => tracing::warn!("tray not available: {e:#}"),
            }
            let push_to_talk = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);
            if let Err(e) = app.global_shortcut().on_shortcut(push_to_talk, |app, _, ev| {
                let cmd =
                    if ev.state() == ShortcutState::Pressed { UserCommand::VoiceStart } else { UserCommand::VoiceStop };
                send(app, cmd);
            }) {
                tracing::warn!("push-to-talk shortcut not available: {e}");
            }
            tauri::async_runtime::spawn(forward_events(ctx.clone()));
            tauri::async_runtime::spawn(dispatch(ctx.clone(), commands));
            tauri::async_runtime::spawn(reminder_loop(ctx));
            // Smoke tests: leave on our own after N seconds (window closed first, never a hard kill).
            if let Some(secs) = std::env::var("PIP_QUIT_AFTER_SECS").ok().and_then(|s| s.parse::<u64>().ok()) {
                let app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                    quit(&app);
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}

fn ctx_of(app: &AppHandle) -> Option<Shared> {
    app.try_state::<Shared>().map(|s| s.inner().clone())
}

fn send(app: &AppHandle, cmd: UserCommand) {
    if let Some(ctx) = ctx_of(app)
        && let Err(e) = ctx.bus.from_ui.try_send(cmd)
    {
        tracing::warn!("command dropped: {e}");
    }
}

fn on_drag_drop(app: &AppHandle, ev: &tauri::DragDropEvent) {
    let Some(ctx) = ctx_of(app) else { return };
    match ev {
        tauri::DragDropEvent::Enter { .. } => ctx.app_event(AppEvent::Drag { state: "enter" }),
        tauri::DragDropEvent::Leave => ctx.app_event(AppEvent::Drag { state: "leave" }),
        tauri::DragDropEvent::Drop { paths, .. } => {
            ctx.app_event(AppEvent::Drag { state: "drop" });
            if !paths.is_empty() {
                send(app, UserCommand::DropFiles { paths: paths.clone() });
            }
        }
        _ => {}
    }
}

/// Core events -> page ("ui"), plus the side effects that live in Rust (voice, Windows toast).
async fn forward_events(ctx: Shared) {
    let mut rx = ctx.bus.to_ui.subscribe();
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("ui lagged by {n} events");
                continue;
            }
            Err(_) => break,
        };
        match &ev {
            UiEvent::ApprovalRequest { .. } | UiEvent::Bubble { question_id: Some(_), .. } => ctx.set_waiting(true),
            UiEvent::ApprovalClosed { .. } | UiEvent::Pip { state: PipState::Thinking | PipState::Acting } => {
                ctx.set_waiting(false)
            }
            _ => {}
        }
        match &ev {
            UiEvent::Speak { text } if !ctx.paused.load(Ordering::SeqCst) => {
                let _ = ctx.speaker.speak(text, ctx.lang());
            }
            UiEvent::Notify { title, body } => {
                if let Err(e) = ctx.app.notification().builder().title(title).body(body).show() {
                    tracing::debug!("notification: {e}");
                }
            }
            _ => {}
        }
        let _ = ctx.app.emit("ui", &ev);
    }
}

async fn dispatch(ctx: Shared, mut rx: mpsc::Receiver<UserCommand>) {
    while let Some(cmd) = rx.recv().await {
        let c = ctx.clone();
        if !matches!(cmd, UserCommand::HitRects { .. }) {
            // Only the command name: texts and paths can be personal.
            let dbg = format!("{cmd:?}");
            tracing::info!("ui command: {}", dbg.split([' ', '{', '(']).next().unwrap_or(""));
            // A click on Pip's bubble or card: the hand moving away from it is not "stop".
            ctx.quiet_mouse(QUIET_AFTER_CLICK);
        }
        match cmd {
            UserCommand::HitRects { rects } => {
                *ctx.bus.hit_rects.write().expect("hit_rects lock") = overlay::sanitize(rects);
            }
            UserCommand::Stop => ctx.stop_by(StopReason::User),
            UserCommand::Resume => ctx.resume(),
            UserCommand::Quit => quit(&ctx.app),
            UserCommand::Approve { id, answer, remember_site } => {
                if answer == pip_core::agent::ApprovalAnswer::Yes {
                    ctx.quiet_mouse(QUIET_AFTER_YES);
                    if remember_site {
                        ctx.bus.pending.mark_remember_site(id);
                    }
                }
                if !ctx.bus.pending.resolve_approval(id, answer) {
                    tracing::debug!("approval {id} had no waiter");
                }
                ctx.bus.emit(UiEvent::ApprovalClosed { id });
            }
            UserCommand::Answer { question_id, choice } => {
                ctx.bus.pending.resolve_answer(question_id, choice);
                ctx.set_waiting(false);
            }
            UserCommand::ReminderAnswer { reminder_id, choice } => c.reminder_answer(reminder_id, &choice),
            UserCommand::DropFiles { paths } => {
                tauri::async_runtime::spawn(c.ingest(paths));
            }
            UserCommand::PickFile => ctx.pick_file(),
            UserCommand::VoiceStart => ctx.voice_start(),
            UserCommand::VoiceStop => {
                tauri::async_runtime::spawn(c.voice_stop());
            }
            UserCommand::Text { text } => {
                let text = text.trim().to_string();
                if !text.is_empty() {
                    ctx.text(text);
                }
            }
            UserCommand::OnboardingAnswer { step, value } => c.onboarding_answer(step, value),
            UserCommand::Forget { kind, id, task } => ctx.forget(kind, &id, &task),
        }
    }
}

async fn reminder_loop(ctx: Shared) {
    let every = std::env::var("PIP_TICK_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(60u64).max(1);
    let mock_at = std::env::var("PIP_MOCK_REMINDER_SECS").ok().and_then(|s| s.parse::<u64>().ok());
    let mut mock_fired = false;
    tokio::time::sleep(Duration::from_secs(4)).await;
    let mut tick = tokio::time::interval(Duration::from_secs(every));
    loop {
        tick.tick().await;
        if !ctx.ready.load(Ordering::SeqCst) || *ctx.onboarding.lock().expect("onb lock") == OnboardingStep::AskName {
            continue;
        }
        // Never on top of Pip at work or an open card/question: the tick waits, nothing is marked fired.
        if ctx.run.lock().expect("run lock").is_some() || ctx.waiting.load(Ordering::SeqCst) {
            continue;
        }
        ctx.update_tray_tip();
        if ctx.backend.mock {
            if let Some(at) = mock_at
                && !mock_fired
                && ctx.started.elapsed() >= Duration::from_secs(at)
            {
                mock_fired = true;
                ctx.fire_reminder(mock::reminder(ctx.lang()));
            }
            continue;
        }
        let Some(rules) = ctx.backend.rules.clone() else { continue };
        let lang = ctx.lang();
        match guard("rules.tick", || rules.tick()) {
            Ok(list) => {
                for r in list {
                    match guard("reminder_event", || Ok(rules.reminder_event(&r, lang))) {
                        Ok(ev) => ctx.fire_reminder(ev),
                        Err(e) => tracing::warn!("{e:#}"),
                    }
                }
            }
            Err(e) => tracing::debug!("{e:#}"),
        }
    }
}

// ---------- tray hooks ----------

pub fn toggle_pause(app: &AppHandle) {
    if let Some(ctx) = ctx_of(app) {
        let now = ctx.paused.load(Ordering::SeqCst);
        ctx.set_paused(!now);
    }
}

pub fn show_deadlines(app: &AppHandle) {
    if let Some(ctx) = ctx_of(app) {
        ctx.show_deadlines();
    }
}

pub fn show_knowledge(app: &AppHandle) {
    if let Some(ctx) = ctx_of(app) {
        ctx.show_knowledge();
    }
}

/// Tray "Beenden": ask first, because a closed Pip cannot remind anybody.
pub fn ask_quit(app: &AppHandle) {
    let Some(ctx) = ctx_of(app) else {
        quit(app);
        return;
    };
    let lang = ctx.lang();
    ctx.pip(PipState::Asking);
    let text = t("quit_confirm", lang).to_string();
    ctx.bus.emit(UiEvent::Speak { text: text.clone() });
    ctx.say(
        text,
        vec![
            Button::new("cmd:quit", t("btn_quit_yes", lang), false),
            Button::new("app:pause", t("btn_rather_pause", lang), true),
        ],
    );
}

pub fn quit(app: &AppHandle) {
    if let Some(ctx) = ctx_of(app) {
        if let Some(t) = ctx.run.lock().expect("run lock").as_ref() {
            t.cancel();
        }
        let _ = ctx.speaker.stop();
    }
    // Close the window before the process ends (a window vanishing under the pointer can upset the compositor).
    if let Some(w) = app.get_webview_window("pip") {
        let _ = w.hide();
        let _ = w.destroy();
    }
    app.exit(0);
}

// ---------- behaviour ----------

impl Ctx {
    fn lang(&self) -> Lang {
        self.backend.lang()
    }

    fn window(&self) -> Option<WebviewWindow> {
        self.app.get_webview_window("pip")
    }

    fn screen(&self) -> OverlayInfo {
        self.window().and_then(|w| overlay::info(&w, self.windowed).ok()).unwrap_or(OverlayInfo {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
            scale: 1.0,
            windowed: self.windowed,
        })
    }

    fn app_event(&self, ev: AppEvent) {
        let _ = self.app.emit("app", &ev);
    }

    fn pip(&self, state: PipState) {
        self.bus.emit(UiEvent::Pip { state });
    }

    fn progress(&self, text: &str) {
        self.bus.emit(UiEvent::Progress { text: text.into() });
    }

    fn say(&self, text: impl Into<String>, buttons: Vec<Button>) {
        self.bus.emit(UiEvent::Bubble { text: text.into(), buttons, question_id: None });
    }

    /// Error bubble; `retry` = the exact command "Nochmal" repeats, `pick` = offer another photo.
    fn error(&self, text: impl Into<String>, retry: Option<UserCommand>, pick: bool) {
        self.app_event(AppEvent::Error { text: text.into(), retry, pick });
    }

    fn start(self: Arc<Self>) {
        let lang = self.lang();
        if self.backend.needs_setup() {
            // Without a key nothing works; say so plainly instead of a cheerful onboarding.
            self.pip(PipState::Asking);
            let text = t("err_setup", lang).to_string();
            self.bus.emit(UiEvent::Speak { text: text.clone() });
            self.app_event(AppEvent::Setup { text });
            return;
        }
        self.update_tray_tip();
        let name = self.backend.profile_get("name");
        let force = self.backend.mock && !env_flag("PIP_SKIP_ONBOARDING");
        if (name.is_none() || force) && !env_flag("PIP_SKIP_ONBOARDING") {
            *self.onboarding.lock().expect("onb lock") = OnboardingStep::AskName;
            self.bus.emit(UiEvent::Onboarding { step: OnboardingStep::AskName });
            self.bus.emit(UiEvent::Speak { text: format!("{} {}", t("onb_hello", lang), t("onb_ask_name", lang)) });
        } else if self.backend.profile_get("onboarded").is_none() && !env_flag("PIP_SKIP_ONBOARDING") {
            // The first start ended before the first letter: invite again instead of "welcome back".
            *self.onboarding.lock().expect("onb lock") = OnboardingStep::FirstLetter;
            self.pip(PipState::Idle);
            self.bus.emit(UiEvent::Onboarding { step: OnboardingStep::FirstLetter });
        } else {
            if self.clone().show_brief() {
                return;
            }
            self.pip(PipState::Idle);
            let text = match name {
                Some(n) => tf("welcome_back", lang, &[("name", &n)]),
                None => t("welcome_back_noname", lang).into(),
            };
            self.say(text, vec![]);
        }
    }

    /// Daily brief at start (once a day): the next deadlines and, if the insurer's services page fits one,
    /// "I can fill it in now" with [Yes, do it now] [Later]. False when there is nothing to say (then the
    /// usual "welcome back"). `PIP_BRIEF=0` turns it off, `PIP_BRIEF=1` shows it on every start (video retakes).
    fn show_brief(self: Arc<Self>) -> bool {
        let Some(mem) = self.backend.memory.clone() else { return false };
        let env = std::env::var("PIP_BRIEF").map(|v| v.trim().to_ascii_lowercase()).unwrap_or_default();
        if self.backend.mock || matches!(env.as_str(), "0" | "false" | "off" | "no") {
            return false;
        }
        let now = self.backend.clock.now();
        let today = now.format("%Y-%m-%d").to_string();
        let always = matches!(env.as_str(), "1" | "true" | "on" | "yes" | "always");
        if !always && self.backend.profile_get("brief_shown_on").as_deref() == Some(today.as_str()) {
            return false;
        }
        let services = self.backend.profile_get(DEMO_SERVICES_KEY).and_then(|url| {
            let html = std::fs::read_to_string(file_url_path(&url)?).ok()?;
            Some((url, html))
        });
        let lang = self.lang();
        let name = self.backend.profile_get("name");
        let brief = guard("brief", || {
            pip_core::rules::daily_brief(
                &mem,
                now,
                lang,
                name.as_deref(),
                services.as_ref().map(|(u, h)| (u.as_str(), h.as_str())),
            )
        });
        let brief = match brief {
            Ok(Some(b)) => b,
            Ok(None) => return false,
            Err(e) => {
                tracing::warn!("daily brief failed: {e:#}");
                return false;
            }
        };
        // A heading the user already said yes or "Not now" to comes back without an offer (no sentence, no
        // button): daily_brief skips it (rules::daily::already_suggested).
        let mut buttons = Vec::new();
        if brief.offer.is_some() {
            buttons.push(Button::new("app:brief_go", t("btn_brief_go", lang), true));
        }
        *self.brief_task.lock().expect("brief lock") =
            brief.offer.map(|o| PendingOffer { task: o.task, heading: o.heading, reason: String::new() });
        buttons.push(Button::new("close", t("btn_onb_later", lang), false));
        self.backend.profile_set("brief_shown_on", &today);
        self.pip(PipState::Happy);
        self.bus.emit(UiEvent::Speak { text: brief.text.clone() });
        self.say(brief.text, buttons);
        self.idle_later(2500);
        true
    }

    fn onboarding_answer(self: Arc<Self>, step: OnboardingStep, value: String) {
        let lang = self.lang();
        match step {
            OnboardingStep::AskName => {
                let name = clean_name(&value);
                if name.is_empty() {
                    self.bus.emit(UiEvent::Onboarding { step: OnboardingStep::AskName });
                    return;
                }
                self.backend.profile_set("name", &name);
                *self.onboarding.lock().expect("onb lock") = OnboardingStep::FirstLetter;
                self.release_focus();
                self.pip(PipState::Happy);
                let mut greet = tf("onb_greet", lang, &[("name", &name)]);
                if !self.backend.mock && enable_autostart() {
                    greet = format!("{greet} {}", t("onb_autostart", lang));
                }
                self.say(greet.clone(), vec![]);
                self.bus.emit(UiEvent::Speak { text: format!("{greet} {}", t("onb_drop_letter", lang)) });
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(2600)).await;
                    if *self.onboarding.lock().expect("onb lock") == OnboardingStep::FirstLetter {
                        self.pip(PipState::Idle);
                        self.bus.emit(UiEvent::Onboarding { step: OnboardingStep::FirstLetter });
                    }
                });
            }
            OnboardingStep::AskState => {
                self.backend.profile_set("bundesland", &canonical_state(&value));
                self.pip(PipState::Happy);
                self.say(t("onb_state_thanks", lang), vec![]);
                *self.onboarding.lock().expect("onb lock") = OnboardingStep::Done;
                self.bus.emit(UiEvent::Onboarding { step: OnboardingStep::Done });
                self.idle_later(1500);
            }
            OnboardingStep::FirstLetter | OnboardingStep::Done => {
                // "Später" on the first-letter step: Pip says how to call him back, then waits.
                self.backend.profile_set("onboarded", "1");
                *self.onboarding.lock().expect("onb lock") = OnboardingStep::Done;
                self.bus.emit(UiEvent::Onboarding { step: OnboardingStep::Done });
                self.release_focus();
                // "look" / "letter": the user picked a first job; that job speaks next.
                if !matches!(value.as_str(), "look" | "letter" | "later_quiet") {
                    self.say(t("later_hint", lang), vec![]);
                }
            }
        }
    }

    fn idle_later(self: &Arc<Self>, ms: u64) {
        let c = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            if c.run.lock().expect("run lock").is_none() && !c.paused.load(Ordering::SeqCst) {
                c.pip(PipState::Idle);
            }
        });
    }

    fn set_paused(&self, on: bool) {
        self.paused.store(on, Ordering::SeqCst);
        if let Some(t) = self.tray.lock().expect("tray lock").as_ref() {
            t.set_paused(on);
        }
        let lang = self.lang();
        if on {
            self.cancel_run();
            let _ = self.speaker.stop();
            self.pip(PipState::Sleeping);
            self.say(t("paused", lang), vec![]);
        } else {
            self.pip(PipState::Idle);
            self.say(t("woke", lang), vec![]);
        }
        self.app_event(AppEvent::Paused { on });
    }

    fn wake_if_paused(&self) {
        if self.paused.load(Ordering::SeqCst) {
            self.set_paused(false);
        }
    }

    fn set_acting(&self, on: bool) {
        let esc = Shortcut::new(None, Code::Escape);
        let gs = self.app.global_shortcut();
        if on {
            if !gs.is_registered(esc)
                && let Err(e) = gs.on_shortcut(esc, |app, _, ev| {
                    if ev.state() == ShortcutState::Pressed {
                        send(app, UserCommand::Stop);
                    }
                })
            {
                tracing::warn!("Esc shortcut not available: {e}");
            }
        } else if gs.is_registered(esc) {
            let _ = gs.unregister(esc);
        }
        self.acting.store(on, Ordering::SeqCst);
        if !on {
            self.waiting.store(false, Ordering::SeqCst);
        }
        self.update_mouse_armed();
        self.app_event(AppEvent::Acting { on });
    }

    /// No mouse-move stop until `d` from now (a later quiet time already set stays).
    fn quiet_mouse(&self, d: Duration) {
        let until = Instant::now() + d;
        let mut q = self.mouse_quiet.lock().expect("quiet lock");
        if q.is_none_or(|t| t < until) {
            *q = Some(until);
        }
    }

    fn set_waiting(&self, on: bool) {
        self.waiting.store(on, Ordering::SeqCst);
        self.update_mouse_armed();
    }

    fn update_mouse_armed(&self) {
        let armed = self.mouse_stop && self.acting.load(Ordering::SeqCst) && !self.waiting.load(Ordering::SeqCst);
        self.mouse_armed.store(armed, Ordering::SeqCst);
    }

    fn cancel_run(&self) -> bool {
        match self.run.lock().expect("run lock").as_ref() {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    /// Stop button, Esc, tray pause (User) or a hand on the mouse (Mouse). The core says the stop sentence.
    fn stop_by(&self, reason: StopReason) {
        if let Some(agent) = &self.backend.agent {
            agent.set_stop_reason(reason);
        }
        let was_running = self.cancel_run();
        let _ = self.speaker.stop();
        if !was_running {
            return;
        }
        self.bus.emit(UiEvent::PointClear);
        self.bus.emit(UiEvent::FlyHome);
        self.progress("");
    }

    /// The last word after a stop: what happened ("Nichts wurde abgeschickt" only when true) and [Weiter]
    /// when the run can go on; or the pause scene when paused.
    fn stopped_scene(&self, outcome: &AgentOutcome) {
        let lang = self.lang();
        self.progress("");
        if self.paused.load(Ordering::SeqCst) {
            self.pip(PipState::Sleeping);
            self.say(t("paused", lang), vec![]);
            return;
        }
        self.pip(PipState::Idle);
        let (text, can_resume) = match &self.backend.agent {
            Some(agent) => (agent.stopped_text(outcome, lang), agent.can_resume()),
            None => (None, self.backend.mock && matches!(outcome, AgentOutcome::Stopped)),
        };
        let text = text.unwrap_or_else(|| t("stopped", lang).to_string());
        let buttons = if can_resume { vec![Button::new("cmd:resume", t("btn_resume", lang), true)] } else { vec![] };
        self.say(text, buttons);
    }

    /// [Weiter] / [Jetzt weitermachen] / [Nochmal fragen]: continue the stopped run, never start it over.
    fn resume(self: &Arc<Self>) {
        if self.backend.mock {
            let task = self.last_task.lock().expect("task lock").clone();
            if let Some(task) = task {
                self.clone().start_task(task);
            }
            return;
        }
        if self.backend.agent.as_ref().is_some_and(|a| a.can_resume()) {
            self.clone().start_job(Job::Resume);
        }
    }

    fn text(self: &Arc<Self>, text: String) {
        if self.backend.mock {
            let l = text.to_lowercase();
            if l.contains("erinner") || l.contains("remind") {
                self.fire_reminder(mock::reminder(self.lang()));
                return;
            }
            if l.contains("frist") || l.contains("deadline") {
                self.show_deadlines();
                return;
            }
        }
        *self.last_text.lock().expect("text lock") = Some(text.clone());
        self.clone().start_task(task_from_words(&text, false));
    }

    fn start_task(self: Arc<Self>, task: Task) {
        self.start_job(Job::Task(task));
    }

    fn start_job(self: Arc<Self>, job: Job) {
        let lang = self.lang();
        self.wake_if_paused();
        if !self.backend.mock && self.backend.agent.is_none() {
            let key = if self.backend.needs_setup() { "err_setup" } else { "err_not_ready" };
            self.error(t(key, lang), None, false);
            return;
        }
        if self.run.lock().expect("run lock").is_some() {
            self.say(t("busy", lang), vec![]);
            return;
        }
        // The start bubble already says what goes to Meta's AI, and pressing a job is the user's yes:
        // no second question. PIP_SCREEN_NOTICE=1 brings the old one-time notice back.
        if matches!(job, Job::Task(_) | Job::Offered(_))
            && env_flag("PIP_SCREEN_NOTICE")
            && self.backend.profile_get("screen_notice_ok").is_none()
        {
            tauri::async_runtime::spawn(async move {
                if self.screen_notice(lang).await {
                    self.backend.profile_set("screen_notice_ok", "1");
                    self.run_job(job, lang);
                } else {
                    self.idle_later(500);
                }
            });
            return;
        }
        // No spoken "I'm working now, moving the mouse stops me": a deliberate move still stops Pip, and the
        // Stop button and Esc are on screen.
        self.run_job(job, lang);
    }

    /// "Damit ich klicken kann, schaue ich auf deinen Bildschirm …" [Verstanden] [Lieber nicht]. True = go.
    async fn screen_notice(&self, lang: Lang) -> bool {
        let qid = uuid::Uuid::new_v4();
        let rx = self.bus.pending.wait_answer(qid);
        let text = format!("{} {}", t("screen_notice", lang), t("screen_notice_mouse", lang));
        self.pip(PipState::Asking);
        self.bus.emit(UiEvent::Speak { text: text.clone() });
        self.bus.emit(UiEvent::Bubble {
            text,
            buttons: vec![
                Button::new("ok", t("btn_understood", lang), true),
                Button::new("no", t("btn_not_now", lang), false),
            ],
            question_id: Some(qid),
        });
        let ok = matches!(tokio::time::timeout(Duration::from_secs(600), rx).await, Ok(Ok(a)) if a == "ok");
        self.bus.pending.resolve_answer(qid, String::new());
        ok
    }

    fn run_job(self: Arc<Self>, job: Job, lang: Lang) {
        let token = {
            let mut run = self.run.lock().expect("run lock");
            if run.is_some() {
                drop(run);
                self.say(t("busy", lang), vec![]);
                return;
            }
            let token = CancellationToken::new();
            *run = Some(token.clone());
            token
        };
        if let Job::Task(task) | Job::Offered(task) = &job
            && !matches!(task, Task::Discover { .. })
        {
            *self.last_task.lock().expect("task lock") = Some(task.clone());
        }
        // A finished normal task may be followed by one look at the insurer's services page; a task the
        // user started from an offer is not (it was the offer).
        let discover_after = discover_after(&job);
        // The ask box had the keyboard: give it back to the page before Pip presses keys.
        self.release_focus();
        self.set_acting(true);
        tauri::async_runtime::spawn(async move {
            let out = if self.backend.mock {
                Ok(mock::run_task(&self.bus, self.screen(), lang, token.clone()).await)
            } else {
                let agent = self.backend.agent.clone().expect("checked above");
                let tk = token.clone();
                guard_async("agent", async move {
                    match job {
                        Job::Task(task) | Job::Offered(task) => agent.run(task, tk).await,
                        Job::Resume => Ok(agent.resume(tk).await?.unwrap_or(AgentOutcome::Stopped)),
                    }
                })
                .await
            };
            // Log only the kind of outcome: the texts may hold personal data (SPEC §4: no contents in logs).
            match &out {
                Ok(o) => tracing::info!("agent finished: {}", outcome_kind(o)),
                Err(e) => tracing::warn!("agent error: {e:#}"),
            }
            *self.run.lock().expect("run lock") = None;
            self.set_acting(false);
            let retry = self.last_text.lock().expect("text lock").clone().map(|text| UserCommand::Text { text });
            match out {
                // The core already said the friendly, specific reason ("zu lange", "kein Internet"): keep it.
                Ok(AgentOutcome::Failed(reason)) => {
                    self.reset_scene();
                    self.error(reason, retry, false);
                }
                Ok(AgentOutcome::NeedsUser(_)) => self.idle_later(5000),
                Ok(o @ (AgentOutcome::Stopped | AgentOutcome::StoppedAfterSend(_))) => self.stopped_scene(&o),
                Ok(AgentOutcome::Done(summary)) if discover_after && !self.backend.mock => self.discover_later(summary),
                Ok(AgentOutcome::Done(_)) => {}
                Err(_) if token.is_cancelled() => self.stopped_scene(&AgentOutcome::Stopped),
                Err(e) => {
                    self.reset_scene();
                    let f = friendly(&e, lang, "err_generic");
                    self.error(f.text, retry.filter(|_| f.retry), false);
                }
            }
        });
    }

    /// After-task offer: OFFER_AFTER_SECS after a finished task Pip offers one thing on the insurer's services
    /// page that fits an open deadline (`rules::after_task`, Rust only, no model call). Only when an open
    /// deadline exists and no heading fits does the model look around once (Task::Discover); in every other
    /// case (no open deadline, quiet days after "Not now", every fitting heading asked before) Pip stays quiet
    /// and opens nothing. Off with PIP_DISCOVER=0 or without a services page.
    fn discover_later(self: &Arc<Self>, after: String) {
        if std::env::var("PIP_DISCOVER").is_ok_and(|v| matches!(v.trim(), "0" | "false" | "off" | "no")) {
            return;
        }
        if self.backend.profile_get(DEMO_SERVICES_KEY).is_none_or(|v| v.trim().is_empty()) {
            return;
        }
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(OFFER_AFTER_SECS)).await;
            // The user started something else or paused Pip in the meantime: do not look around.
            if me.paused.load(Ordering::SeqCst) || me.run.lock().expect("run lock").is_some() {
                return;
            }
            match me.after_task() {
                AfterTask::Offer(o) => me.show_offer(o),
                AfterTask::LookAround => me.start_job(Job::Task(Task::Discover { after })),
                AfterTask::Silent => tracing::info!("after-task offer: nothing to offer"),
            }
        });
    }

    /// What follows a finished task (`rules::after_task`). The services page is read once; a page on the web
    /// (not file://) counts as one without headings, so only the model can look at it. A file:// page that
    /// cannot be read is gone for the model too: Silent. No memory or an error: Silent (never a page opened
    /// without words).
    fn after_task(&self) -> AfterTask {
        let Some(mem) = self.backend.memory.clone() else { return AfterTask::Silent };
        let Some(url) = self.backend.profile_get(DEMO_SERVICES_KEY) else { return AfterTask::Silent };
        let html = match file_url_path(&url).map(std::fs::read_to_string) {
            Some(Ok(html)) => html,
            Some(Err(e)) => {
                tracing::warn!("after-task offer: cannot read the services page {url}: {e}");
                return AfterTask::Silent;
            }
            None => String::new(),
        };
        let lang = self.lang();
        // Events carry the real time, so the 7 quiet days after "Not now" count in real time too.
        match guard("offer", || pip_core::rules::after_task(&mem, chrono::Local::now(), lang, (&url, &html))) {
            Ok(next) => next,
            Err(e) => {
                tracing::warn!("after-task offer failed: {e:#}");
                AfterTask::Silent
            }
        }
    }

    /// The deterministic after-task offer: "Gesundkasse also offers …" [Yes, let's do it] [Not now] [Why?].
    fn show_offer(self: Arc<Self>, o: AfterTaskOffer) {
        let lang = self.lang();
        self.offer_event(EV_SUGGESTED, &o.offer.heading);
        let text =
            tf("sugg_text", lang, &[("insurer", &o.insurer), ("quote", &o.offer.heading), ("reason", &o.reason)]);
        *self.offer_task.lock().expect("offer lock") =
            Some(PendingOffer { task: o.offer.task, heading: o.offer.heading, reason: o.reason });
        self.pip(PipState::Asking);
        self.bus.emit(UiEvent::Speak { text: text.clone() });
        self.say(text, offer_buttons(lang, true));
    }

    fn offer_event(&self, kind: &str, heading: &str) {
        if let Some(mem) = &self.backend.memory
            && let Err(e) = mem.add_event(kind, heading)
        {
            tracing::warn!("events write failed: {e:#}");
        }
    }

    fn release_focus(&self) {
        if let Some(w) = self.window() {
            self.focus.give_back(&w);
        }
    }

    fn app_action(self: Arc<Self>, action: &str) {
        let lang = self.lang();
        match action.split_once(':').unwrap_or((action, "")) {
            ("pause", _) => self.set_paused(true),
            ("last_letter", _) => self.last_letter(),
            ("brief_go", _) => {
                let offer = self.brief_task.lock().expect("brief lock").take();
                if let Some(o) = offer {
                    // Yes on the brief counts as an answer: the after-task offer will not ask again.
                    self.offer_event(EV_ACCEPTED, &o.heading);
                    // Straight to Task::Text: the task always says "Sending needs approval", so the word
                    // heuristics of task_from_words could reroute it (e.g. a heading with "sick note").
                    *self.last_text.lock().expect("text lock") = Some(o.task.clone());
                    // Job::Offered: when it is done Pip does not look for the next thing (it was the offer).
                    self.start_job(Job::Offered(Task::Text(o.task)));
                }
            }
            ("offer_go", _) => {
                let offer = self.offer_task.lock().expect("offer lock").take();
                if let Some(o) = offer {
                    self.offer_event(EV_ACCEPTED, &o.heading);
                    // Like brief_go: straight to Task::Text, and no look around after it.
                    *self.last_text.lock().expect("text lock") = Some(o.task.clone());
                    self.start_job(Job::Offered(Task::Text(o.task)));
                }
            }
            ("offer_no", _) => {
                let offer = self.offer_task.lock().expect("offer lock").take();
                if let Some(o) = offer {
                    self.offer_event(EV_DECLINED, &o.heading);
                    self.pip(PipState::Idle);
                    self.say(t("sugg_declined", lang), vec![]);
                }
            }
            ("offer_why", _) => {
                let reason = self.offer_task.lock().expect("offer lock").as_ref().map(|o| o.reason.clone());
                if let Some(reason) = reason {
                    // Only the bubble: the offer itself was already spoken.
                    self.say(tf("sugg_why", lang, &[("reason", &reason)]), offer_buttons(lang, false));
                }
            }
            ("undo", id) => {
                let Ok(id) = id.parse::<i64>() else { return };
                let r = self.backend.rules.clone().map(|rules| guard("undo", || rules.undo_done(id, lang)));
                match r {
                    Some(Ok(text)) => {
                        self.pip(PipState::Idle);
                        self.say(text, vec![]);
                        self.update_tray_tip();
                    }
                    Some(Err(e)) => tracing::warn!("undo: {e:#}"),
                    None if self.backend.mock => self.say(t("ans_undone", lang), vec![]),
                    None => {}
                }
            }
            ("deadline_done", id) => {
                let Ok(id) = id.parse::<i64>() else { return };
                if let Some(rules) = self.backend.rules.clone()
                    && let Err(e) = guard("deadline done", || rules.deadline_done(id, lang))
                {
                    tracing::warn!("deadline done: {e:#}");
                }
                self.update_tray_tip();
                self.show_deadlines();
            }
            (other, _) => tracing::debug!("unknown app action {other}"),
        }
    }

    /// "Was stand im letzten Brief?": the sentence Pip said after reading it.
    fn last_letter(&self) {
        let lang = self.lang();
        let text = self
            .backend
            .memory
            .clone()
            .and_then(|m| guard("events", || m.recent_events(200)).ok())
            .and_then(|ev| ev.into_iter().find(|e| e.kind == "letter").map(|e| e.text));
        match text {
            Some(text) => {
                self.bus.emit(UiEvent::Speak { text: text.clone() });
                self.say(text, vec![Button::new("close", t("btn_ok", lang), true)]);
            }
            None => self.say(t("no_letter_known", lang), vec![]),
        }
    }

    /// Tray tooltip: when Pip will speak up next.
    fn update_tray_tip(&self) {
        let lang = self.lang();
        let now = self.backend.clock.now();
        let next = self.backend.memory.clone().and_then(|m| m.next_reminder_at(now).ok().flatten());
        let when = next.map(|t| format!("{}, {}", format_due(t.date_naive(), lang), t.format("%H:%M")));
        if let Some(tray) = self.tray.lock().expect("tray lock").as_ref() {
            tray.set_next_reminder(when.as_deref());
        }
    }

    /// Pip back home, no highlight, no progress line.
    fn reset_scene(&self) {
        self.bus.emit(UiEvent::PointClear);
        self.bus.emit(UiEvent::FlyHome);
        self.progress("");
        self.pip(PipState::Error);
    }

    async fn ingest(self: Arc<Self>, paths: Vec<PathBuf>) {
        let lang = self.lang();
        let (letters, others): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| is_letter(p));
        if letters.is_empty() {
            if !others.is_empty() {
                self.pip(PipState::Asking);
                self.say(t("err_file_type", lang), vec![]);
                self.idle_later(4000);
            }
            return;
        }
        self.wake_if_paused();
        let _one_at_a_time = self.ingest_lock.lock().await;
        for path in letters.into_iter().take(3) {
            self.pip(PipState::Thinking);
            self.progress(t("doc_reading", lang));
            let p = path.clone();
            let result: Result<Read> = if self.backend.mock {
                let m = mock::ingest(lang).await;
                Ok(Read { bubble: m.bubble, duplicate: false, unsure: false, doc_id: None })
            } else if let Some(docs) = self.backend.docs.clone() {
                guard_async("ingest", async move { docs.ingest(&p, lang).await }).await.map(|r| Read {
                    bubble: r.bubble,
                    duplicate: r.duplicate,
                    unsure: r.unsure,
                    doc_id: Some(r.doc_id),
                })
            } else {
                Err(anyhow::anyhow!("document pipeline not available"))
            };
            self.progress("");
            match result {
                Ok(read) => {
                    let text =
                        if read.duplicate { format!("{} {}", t("doc_known", lang), read.bubble) } else { read.bubble };
                    self.pip(PipState::Happy);
                    self.bus.emit(UiEvent::Speak { text: text.clone() });
                    self.after_first_letter();
                    self.update_tray_tip();
                    if read.unsure
                        && let Some(doc_id) = read.doc_id
                    {
                        // "Ich habe als Frist Mittwoch, 18.11. gelesen …" [Stimmt] [Stimmt nicht]
                        self.check_date(text, doc_id, lang).await;
                    } else {
                        // The only place the user learns what the letter means: it stays until [Gut].
                        self.say(text, vec![Button::new("close", t("btn_ok", lang), true)]);
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                    self.pip(PipState::Idle);
                }
                Err(e) => {
                    tracing::warn!("ingest failed: {e:#}");
                    self.pip(PipState::Error);
                    let f = friendly(&e, lang, "err_read_letter");
                    let retry = f.retry.then(|| UserCommand::DropFiles { paths: vec![path.clone()] });
                    self.error(f.text, retry, f.pick);
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    self.pip(PipState::Idle);
                }
            }
        }
    }

    /// Unsure date: the user checks it on the paper. "Stimmt nicht" removes the letter's deadlines.
    async fn check_date(&self, text: String, doc_id: i64, lang: Lang) {
        let qid = uuid::Uuid::new_v4();
        let rx = self.bus.pending.wait_answer(qid);
        self.pip(PipState::Asking);
        self.bus.emit(UiEvent::Bubble {
            text,
            buttons: vec![
                Button::new("ok", t("btn_correct", lang), true),
                Button::new("wrong", t("btn_wrong", lang), false),
            ],
            question_id: Some(qid),
        });
        let answer = tokio::time::timeout(Duration::from_secs(3600), rx).await;
        self.bus.pending.resolve_answer(qid, String::new());
        match answer {
            Ok(Ok(a)) if a == "wrong" => {
                if let Some(mem) = self.backend.memory.clone() {
                    let r = guard("dismiss deadlines", || {
                        for d in mem.deadlines_for_document(doc_id)? {
                            mem.set_deadline_status(d.id, DeadlineStatus::Dismissed)?;
                        }
                        Ok(())
                    });
                    if let Err(e) = r {
                        tracing::warn!("{e:#}");
                    }
                }
                self.update_tray_tip();
                self.say(t("ans_date_wrong", lang), vec![]);
            }
            Ok(Ok(a)) if a == "ok" => self.say(t("ans_date_ok", lang), vec![]),
            _ => {}
        }
    }

    /// The first letter ends the onboarding. (The Bundesland question is not asked: no rule uses it yet.)
    fn after_first_letter(self: &Arc<Self>) {
        let mut onb = self.onboarding.lock().expect("onb lock");
        if *onb == OnboardingStep::FirstLetter {
            *onb = OnboardingStep::Done;
            self.backend.profile_set("onboarded", "1");
            self.bus.emit(UiEvent::Onboarding { step: OnboardingStep::Done });
        }
    }

    fn pick_file(&self) {
        let lang = self.lang();
        let bus = self.bus.clone();
        let mut dialog = self.app.dialog().file().add_filter(t("btn_pick_letter", lang), LETTER_EXT);
        // Start where letters land: Downloads (else Pictures).
        if let Ok(home) = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
            let home = PathBuf::from(home);
            if let Some(dir) = [home.join("Downloads"), home.join("Pictures")].into_iter().find(|d| d.is_dir()) {
                dialog = dialog.set_directory(dir);
            }
        }
        // No parent: the parent would be the full-screen overlay, and Windows then puts the dialog
        // in the top-left corner. Without one it opens in the middle of the screen.
        tracing::info!("file dialog: open");
        #[cfg(windows)]
        pip_platform::windows::center_own_dialog_soon();
        dialog.pick_file(move |file| {
            tracing::info!("file dialog: closed, picked={}", file.is_some());
            if let Some(path) = file.and_then(|f| f.into_path().ok()) {
                let _ = bus.from_ui.try_send(UserCommand::DropFiles { paths: vec![path] });
            }
        });
    }

    fn voice_start(&self) {
        if self.listening.swap(true, Ordering::SeqCst) {
            return;
        }
        self.wake_if_paused();
        let _ = self.speaker.stop();
        let lang = self.lang();
        if !self.backend.mock {
            match Recorder::start() {
                Ok(r) => *self.recorder.lock().expect("rec lock") = Some(r),
                Err(e) => {
                    tracing::warn!("microphone: {e:#}");
                    self.listening.store(false, Ordering::SeqCst);
                    self.error(t("err_mic", lang), None, false);
                    return;
                }
            }
        }
        self.pip(PipState::Listening);
        self.progress(t("listening", lang));
    }

    async fn voice_stop(self: Arc<Self>) {
        if !self.listening.swap(false, Ordering::SeqCst) {
            return;
        }
        let lang = self.lang();
        self.progress("");
        let text = if self.backend.mock {
            Ok(match lang {
                Lang::De => "Schick die neue Krankschreibung an meine Krankenkasse.".to_string(),
                Lang::En => "Send the new sick note to my health insurer.".to_string(),
            })
        } else {
            let Some(rec) = self.recorder.lock().expect("rec lock").take() else { return };
            let wav =
                tokio::task::spawn_blocking(move || rec.stop()).await.map_err(anyhow::Error::from).and_then(|r| r);
            match wav {
                Err(e) if e.downcast_ref::<TooShort>().is_some() => {
                    self.pip(PipState::Idle);
                    self.say(t("voice_too_short", lang), vec![]);
                    return;
                }
                Err(e) => Err(e),
                Ok(wav) => match self.backend.meta.clone() {
                    Some(meta) => {
                        self.pip(PipState::Thinking);
                        self.progress(t("thinking", lang));
                        let r = guard_async("transcribe", async move { meta.transcribe(wav, lang).await }).await;
                        self.progress("");
                        r
                    }
                    None => Err(anyhow::anyhow!("meta client not available")),
                },
            }
        };
        match text {
            Ok(text) if text.trim().is_empty() => {
                self.pip(PipState::Idle);
                self.say(t("voice_too_short", lang), vec![]);
            }
            Ok(text) => {
                let text = text.trim().to_string();
                *self.last_text.lock().expect("text lock") = Some(text.clone());
                self.say(tf("heard", lang, &[("text", &text)]), vec![]);
                self.clone().start_task(task_from_words(&text, true));
            }
            Err(e) => {
                tracing::warn!("voice: {e:#}");
                self.pip(PipState::Error);
                // Speaking again is the retry; "Nochmal" would replay something else.
                let f = friendly(&e, lang, "err_mic");
                self.error(f.text, None, false);
                self.idle_later(1500);
            }
        }
    }

    fn fire_reminder(&self, ev: UiEvent) {
        self.wake_if_paused();
        if let UiEvent::Reminder { text, .. } = &ev {
            self.bus.emit(UiEvent::Notify { title: "Pip".into(), body: text.clone() });
            self.bus.emit(UiEvent::Speak { text: text.clone() });
        }
        self.pip(PipState::Asking);
        self.bus.emit(ev);
    }

    fn reminder_answer(self: Arc<Self>, reminder_id: i64, choice: &str) {
        let lang = self.lang();
        let text = if self.backend.mock {
            Ok(mock::reminder_answer(choice, lang))
        } else if let Some(rules) = self.backend.rules.clone() {
            guard("reminder answer", || rules.answer(reminder_id, choice, lang))
        } else {
            Err(anyhow::anyhow!("rules not available"))
        };
        match text {
            Ok(text) if choice == "done" => {
                // "Ich bin gesund" ends all reminders: offer a way back for a slip of the hand.
                self.pip(PipState::Happy);
                let undo = Button::new(format!("app:undo:{reminder_id}"), t("btn_undo", lang), false);
                self.app_event(AppEvent::Timed { text, buttons: vec![undo], ttl_secs: UNDO_SECS });
                self.update_tray_tip();
                self.idle_later(2000);
            }
            Ok(text) => {
                self.pip(PipState::Idle);
                self.say(text, vec![]);
                self.update_tray_tip();
                self.idle_later(2000);
            }
            Err(e) => {
                tracing::warn!("reminder answer: {e:#}");
                self.error(t("err_generic", lang), None, false);
            }
        }
    }

    fn show_deadlines(&self) {
        let lang = self.lang();
        let today = self.backend.clock.now().date_naive();
        let item = |id: i64, title: String, due: NaiveDate, why: String| DeadlineItem {
            id,
            title,
            due: format_due(due, lang),
            when: relative_day(due, today, lang),
            overdue: due < today,
            why,
        };
        let items = if self.backend.mock {
            let title = match lang {
                Lang::De => "Neue Krankschreibung",
                Lang::En => "New sick note",
            };
            vec![item(
                0,
                title.into(),
                NaiveDate::from_ymd_opt(2026, 11, 2).expect("date"),
                t("rem_au_why", lang).into(),
            )]
        } else {
            self.backend
                .memory
                .clone()
                .and_then(|m| guard("open_deadlines", || m.open_deadlines()).ok())
                .unwrap_or_default()
                .into_iter()
                .map(|d| item(d.id, d.title, d.due, d.why))
                .collect()
        };
        self.app_event(AppEvent::Deadlines { items });
    }

    /// "What Pip knows": about you, your letters, places Pip knows, sites where Pip sends without asking.
    fn show_knowledge(&self) {
        let lang = self.lang();
        let k = self.backend.memory.clone().and_then(|m| guard("knowledge", || m.knowledge()).ok()).unwrap_or_default();
        self.app_event(AppEvent::Knowledge { title: t("know_title", lang).into(), groups: knowledge_groups(&k, lang) });
    }

    /// A Forget button: remove that one thing, then show the list again.
    fn forget(&self, kind: ForgetKind, id: &str, task: &str) {
        let Some(m) = self.backend.memory.clone() else { return };
        let r = guard("forget", || match kind {
            ForgetKind::Fact => m.profile_delete(id),
            ForgetKind::Letter => m.document_delete(id.parse()?),
            ForgetKind::Place => m.playbook_delete(id, task),
            ForgetKind::Trusted => untrust_site(&m, id),
        });
        match r {
            // Only the kind: the id can be personal.
            Ok(found) => tracing::info!(?kind, found, "forgotten"),
            Err(e) => tracing::warn!("forget: {e:#}"),
        }
        if kind == ForgetKind::Letter {
            self.update_tray_tip();
        }
        self.show_knowledge();
    }
}

/// The groups of "What Pip knows", in plain words.
fn knowledge_groups(k: &Knowledge, lang: Lang) -> Vec<KnowGroup> {
    let empty = t("know_empty", lang).to_string();
    let date_of =
        |id: i64| k.letters.iter().find(|d| d.id == id).map(|d| d.extract.date.unwrap_or(d.created_at.date_naive()));
    let facts = k
        .facts
        .iter()
        .filter(|f| !SETTING_KEYS.contains(&f.key.as_str()) && !f.value.contains("://"))
        .map(|f| {
            let (kind, doc) = f.source.split_once(':').unwrap_or((f.source.as_str(), ""));
            let detail = match kind {
                "user" => t("know_src_user", lang).to_string(),
                "typed" => t("know_src_typed", lang).to_string(),
                "letter" => match doc.parse().ok().and_then(date_of) {
                    Some(d) => tf("know_src_letter_date", lang, &[("date", &fmt_date(d, lang))]),
                    None => t("know_src_letter", lang).to_string(),
                },
                _ => t("know_src_model", lang).to_string(),
            };
            KnowItem {
                kind: ForgetKind::Fact,
                id: f.key.clone(),
                task: String::new(),
                title: format!("{}: {}", fact_label(&f.key, lang), f.value),
                detail,
            }
        })
        .collect();
    let letters = k
        .letters
        .iter()
        .map(|d| {
            let ty = match d.doc_type {
                DocType::AuBescheinigung => "know_type_au",
                DocType::Bescheid => "know_type_decision",
                DocType::Rechnung => "know_type_invoice",
                DocType::Mahnung => "know_type_reminder",
                DocType::Vertrag => "know_type_contract",
                DocType::Brief | DocType::Sonstiges => "know_type_letter",
            };
            let date = d.extract.date.unwrap_or(d.created_at.date_naive());
            KnowItem {
                kind: ForgetKind::Letter,
                id: d.id.to_string(),
                task: String::new(),
                title: tf("know_letter_from", lang, &[("type", t(ty, lang)), ("sender", d.extract.sender.trim())]),
                detail: fmt_date(date, lang),
            }
        })
        .collect();
    let site_name = |site: &str| {
        if site.starts_with("file:") { t("page_local", lang).to_string() } else { site.to_string() }
    };
    let places = k
        .playbooks
        .iter()
        .map(|p| {
            let task = match p.task.as_str() {
                "send_au" => "know_task_send_au",
                "send_letter" => "know_task_send_letter",
                "fill_form" => "know_task_fill_form",
                _ => "know_task_other",
            };
            let date = fmt_date(p.updated_at.date_naive(), lang);
            let n = p.runs.to_string();
            let key = if p.runs == 1 { "know_place_once" } else { "know_place_runs" };
            KnowItem {
                kind: ForgetKind::Place,
                id: p.site.clone(),
                task: p.task.clone(),
                title: site_name(&p.site),
                detail: tf(key, lang, &[("task", t(task, lang)), ("n", &n), ("date", &date)]),
            }
        })
        .collect();
    let trusted = k
        .trusted_sites
        .iter()
        .map(|s| KnowItem {
            kind: ForgetKind::Trusted,
            id: s.clone(),
            task: String::new(),
            title: site_name(s),
            detail: t("know_trusted_detail", lang).to_string(),
        })
        .collect();
    let group = |key: &str, items: Vec<KnowItem>| KnowGroup { title: t(key, lang).into(), empty: empty.clone(), items };
    vec![
        group("know_about_you", facts),
        group("know_letters", letters),
        group("know_places", places),
        group("know_trusted", trusted),
    ]
}

/// "krankenkasse" -> "Health insurer"; unknown keys: "favourite_shop" -> "Favourite shop".
fn fact_label(key: &str, lang: Lang) -> String {
    let k = format!("know_key_{key}");
    let known = t(&k, lang);
    if known != k {
        return known.to_string();
    }
    let words = key.replace(['_', '-'], " ");
    let mut c = words.trim().chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn outcome_kind(o: &AgentOutcome) -> &'static str {
    match o {
        AgentOutcome::Done(_) => "done",
        AgentOutcome::Stopped => "stopped",
        AgentOutcome::StoppedAfterSend(_) => "stopped after sending",
        AgentOutcome::NeedsUser(_) => "needs user",
        AgentOutcome::Failed(_) => "failed",
    }
}

/// The page sends the state name in the UI language; the profile keeps the German name.
fn canonical_state(value: &str) -> String {
    let value = value.trim();
    let de: Vec<&str> = t("states_list", Lang::De).split('|').collect();
    t("states_list", Lang::En)
        .split('|')
        .position(|en| en == value)
        .and_then(|i| de.get(i))
        .map_or_else(|| value.to_string(), |s| s.to_string())
}

/// Local path of a `file://` URL (the services page the demo profile points to); same rules as the agent's:
/// "file:///C:/x%20y/a.html" -> "C:/x y/a.html", "file:///home/a.html" -> "/home/a.html". Other URLs: None.
fn file_url_path(url: &str) -> Option<PathBuf> {
    let url = url.trim();
    if !url.get(..7)?.eq_ignore_ascii_case("file://") {
        return None;
    }
    let rest = &url[7..];
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let rest = rest.split(['?', '#']).next()?;
    let b = rest.as_bytes();
    let mut bytes = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = rest.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            bytes.push(v);
            i += 3;
            continue;
        }
        bytes.push(b[i]);
        i += 1;
    }
    let path = String::from_utf8_lossy(&bytes).into_owned();
    let b = path.as_bytes();
    let path = if b.len() >= 3 && b[0] == b'/' && b[2] == b':' { path[1..].to_string() } else { path };
    Some(PathBuf::from(path))
}

fn is_letter(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| LETTER_EXT.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// What Pip says about an error, and which button fits.
#[derive(Debug, PartialEq)]
struct Friendly {
    text: String,
    /// "Nochmal" can help (network, busy service, a model hiccup).
    retry: bool,
    /// "Anderes Foto wählen" instead (the photo itself was the problem).
    pick: bool,
}

/// What happened + next step, never a code.
fn friendly(e: &anyhow::Error, lang: Lang, fallback: &str) -> Friendly {
    let f = |key: &str, retry: bool, pick: bool| Friendly { text: t(key, lang).into(), retry, pick };
    for cause in e.chain() {
        if let Some(d) = cause.downcast_ref::<DocError>() {
            let unreadable = matches!(d, DocError::Unreadable);
            return Friendly { text: d.user_text(lang), retry: false, pick: unreadable };
        }
        if let Some(m) = cause.downcast_ref::<MetaError>() {
            return match m {
                MetaError::Network(_) => f("err_offline", true, false),
                MetaError::MissingKey | MetaError::Http { status: 401..=403, .. } => f("err_setup", false, false),
                m if m.is_retryable() => f("err_service_busy", true, false),
                _ => f(fallback, true, false),
            };
        }
    }
    if e.to_string().contains("panicked") || e.to_string().contains("not available") {
        return f("err_not_ready", false, false);
    }
    f(fallback, true, false)
}

/// "in 3 Tagen" / "morgen" / "heute" / "vorbei".
fn relative_day(due: NaiveDate, today: NaiveDate, lang: Lang) -> String {
    match (due - today).num_days() {
        n if n < 0 => t("deadline_overdue", lang).into(),
        0 => t("day_today", lang).into(),
        1 => t("day_tomorrow", lang).into(),
        n => tf("deadline_in_days", lang, &[("n", &n.to_string())]),
    }
}

/// Pip starts with Windows (current user's Run key), so reminders still come after a restart.
/// `PIP_AUTOSTART=0` turns it off. True if it is set up now.
fn enable_autostart() -> bool {
    if std::env::var("PIP_AUTOSTART").is_ok_and(|v| v.trim() == "0") {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let Ok(exe) = std::env::current_exe() else { return false };
        let status = std::process::Command::new("reg")
            .args([
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "Pip",
                "/t",
                "REG_SZ",
                "/f",
                "/d",
            ])
            .arg(format!("\"{}\"", exe.display()))
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        match status {
            Ok(s) if s.success() => true,
            other => {
                tracing::warn!("autostart not set: {other:?}");
                false
            }
        }
    }
    #[cfg(not(windows))]
    false
}

/// Letter reading result, the parts the bubble needs.
struct Read {
    bubble: String,
    duplicate: bool,
    unsure: bool,
    doc_id: Option<i64>,
}

/// What a run does: a new task, a task the user said yes to in an offer (brief or after-task), or "Weiter"
/// on the stopped one.
enum Job {
    Task(Task),
    /// From [Yes, do it now] / [Yes, let's do it]: runs like Task, but no after-task offer follows.
    Offered(Task),
    Resume,
}

/// Whether a finished job may be followed by the after-task offer: only a normal task the user asked for,
/// never an accepted offer (that would chain offers), a look around itself, or a resumed run.
fn discover_after(job: &Job) -> bool {
    matches!(job, Job::Task(task) if !matches!(task, Task::Discover { .. }))
}

/// "Montag, 2. November" / "Monday, November 2".
pub fn format_due(d: NaiveDate, lang: Lang) -> String {
    let weekdays: Vec<&str> = t("weekdays", lang).split('|').collect();
    let months: Vec<&str> = t("months", lang).split('|').collect();
    let wd = weekdays.get(d.weekday().num_days_from_monday() as usize).copied().unwrap_or("");
    let mo = months.get(d.month0() as usize).copied().unwrap_or("");
    tf("date_long", lang, &[("weekday", wd), ("day", &d.day().to_string()), ("month", mo)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offer_buttons_yes_first_why_only_once() {
        let b = offer_buttons(Lang::En, true);
        let ids: Vec<&str> = b.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["app:offer_go", "app:offer_no", "app:offer_why"]);
        assert!(b[0].primary && !b[1].primary && !b[2].primary);
        assert_eq!(b[0].label, "Yes, let's do it");
        assert_eq!(b[1].label, "Not now");
        let after_why: Vec<String> = offer_buttons(Lang::En, false).into_iter().map(|b| b.id).collect();
        assert_eq!(after_why, ["app:offer_go", "app:offer_no"]);
    }

    #[test]
    fn only_a_normal_task_is_followed_by_the_after_task_offer() {
        assert!(discover_after(&Job::Task(Task::Text("send my sick note".into()))));
        assert!(!discover_after(&Job::Offered(Task::Text("Open file:///x. Click \"A\"".into()))));
        assert!(!discover_after(&Job::Resume));
        assert!(!discover_after(&Job::Task(Task::Discover { after: "Sent.".into() })));
    }

    #[test]
    fn seconds_saved_is_a_setting() {
        assert!(SETTING_KEYS.contains(&"seconds_saved_total"));
    }

    #[test]
    fn every_ui_key_has_text() {
        for lang in [Lang::De, Lang::En] {
            for k in UI_KEYS {
                assert_ne!(t(k, lang), *k, "missing i18n key {k}");
            }
        }
        for k in ["welcome_back", "busy", "heard", "err_file_type", "err_read_letter", "paused", "woke", "weekdays"] {
            assert_ne!(t(k, Lang::De), k);
        }
    }

    #[test]
    fn due_dates_read_naturally() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        assert_eq!(format_due(d, Lang::De), "Montag, 2. November");
        assert_eq!(format_due(d, Lang::En), "Monday, November 2");
    }

    #[test]
    fn letters_by_extension() {
        assert!(is_letter(Path::new("C:/Users/x/Downloads/AU.JPG")));
        assert!(is_letter(Path::new("brief.pdf")));
        assert!(is_letter(Path::new("whatsapp_foto.webp")));
        assert!(!is_letter(Path::new("notes.docx")));
        assert!(!is_letter(Path::new("noext")));
    }

    #[test]
    fn errors_are_friendly() {
        // A busy service is not a WLAN problem.
        let e = anyhow::Error::new(MetaError::Http { status: 503, body: "x".into() });
        assert_eq!(friendly(&e, Lang::De, "err_generic").text, t("err_service_busy", Lang::De));
        let e = anyhow::Error::new(MetaError::Http { status: 429, body: "x".into() });
        assert!(friendly(&e, Lang::De, "err_generic").retry);
        // Missing or rejected key: no "Nochmal", it would fail forever.
        for e in [
            anyhow::Error::new(MetaError::MissingKey).context("reading letter"),
            anyhow::Error::new(MetaError::Http { status: 401, body: String::new() }),
            anyhow::Error::new(MetaError::Http { status: 403, body: String::new() }),
        ] {
            let f = friendly(&e, Lang::De, "err_generic");
            assert_eq!((f.text.as_str(), f.retry), (t("err_setup", Lang::De), false));
        }
        // A blurry photo: another photo, not the same one again.
        let f = friendly(&anyhow::Error::new(DocError::Unreadable), Lang::De, "err_read_letter");
        assert!(f.pick && !f.retry);
        let e = anyhow::anyhow!("ingest panicked: not implemented");
        assert_eq!(friendly(&e, Lang::En, "err_generic").text, t("err_not_ready", Lang::En));
        let e = anyhow::anyhow!("bad json");
        assert_eq!(friendly(&e, Lang::De, "err_read_letter").text, t("err_read_letter", Lang::De));
        // No error text promises a button.
        for k in ["err_offline", "err_service_busy", "err_generic", "err_setup"] {
            assert!(!t(k, Lang::De).contains("Nochmal") && !t(k, Lang::En).contains("Try again"), "{k}");
        }
    }

    #[test]
    fn days_until_a_deadline() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 29).unwrap();
        let d = |n: i64| today + chrono::Duration::days(n);
        assert_eq!(relative_day(d(3), today, Lang::De), "in 3 Tagen");
        assert_eq!(relative_day(d(1), today, Lang::De), "morgen");
        assert_eq!(relative_day(d(0), today, Lang::En), "today");
        assert_eq!(relative_day(d(-2), today, Lang::De), "vorbei");
    }

    #[test]
    fn state_is_stored_in_german() {
        assert_eq!(canonical_state("North Rhine-Westphalia"), "Nordrhein-Westfalen");
        assert_eq!(canonical_state(" Bayern "), "Bayern");
        assert_eq!(canonical_state("Bavaria"), "Bayern");
    }

    /// Every text the page asks for with s("…") is sent by ui_strings, and every key the
    /// Rust side uses exists in both languages (a missing key would show up as "btn_xyz").
    #[test]
    fn file_urls_to_paths() {
        assert_eq!(file_url_path("file:///C:/Pip/a%20b.html"), Some(PathBuf::from("C:/Pip/a b.html")));
        assert_eq!(file_url_path("file:///home/x/s.html#top"), Some(PathBuf::from("/home/x/s.html")));
        assert_eq!(file_url_path("https://kasse.example/services"), None);
    }

    #[test]
    fn every_used_key_exists() {
        let js = include_str!("../ui/main.js");
        for (i, _) in js.match_indices("s(\"") {
            let before = js[..i].chars().last().unwrap_or(' ');
            if before.is_ascii_alphanumeric() || before == '_' || before == '.' {
                continue; // contains("…"), classList.contains, …
            }
            let k = js[i + 3..].split('"').next().unwrap_or("");
            assert!(UI_KEYS.contains(&k), "main.js uses s(\"{k}\") but UI_KEYS lacks it");
        }
        let rust = [include_str!("bridge.rs"), include_str!("mock.rs"), include_str!("tray.rs")].concat();
        for pat in [" t(\"", "(t(\"", " tf(\"", "(tf(\""] {
            for k in rust.split(pat).skip(1).filter_map(|r| r.split('"').next()) {
                for lang in [Lang::De, Lang::En] {
                    assert_ne!(t(k, lang), k, "missing i18n key {k} ({lang:?})");
                }
            }
        }
    }

    #[test]
    fn knowledge_in_plain_words() {
        use pip_core::memory::{Memory, RunSummary, learn_from_run};
        let m = Memory::open_in_memory().unwrap();
        m.profile_set("krankenkasse", "Gesundkasse").unwrap();
        m.profile_set("lang", "en").unwrap();
        m.profile_set("trusted_send_hosts", "www.gesundkasse.example").unwrap();
        m.profile_set_with("favourite_shop", "Bäckerei", "model", 0.6).unwrap();
        let run = RunSummary {
            site: Some("www.gesundkasse.example".into()),
            task_kind: "send_au".into(),
            send_label: Some("Send".into()),
            ok: true,
            ..Default::default()
        };
        learn_from_run(&m, &run).unwrap();
        let g = knowledge_groups(&m.knowledge().unwrap(), Lang::En);
        let titles: Vec<&str> = g.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["About you", "Your letters", "Places I know", "Where I may send without asking"]);
        let about: Vec<(&str, &str)> = g[0].items.iter().map(|i| (i.title.as_str(), i.detail.as_str())).collect();
        assert!(about.contains(&("Health insurer: Gesundkasse", "you told me")), "{about:?}");
        assert!(about.contains(&("Favourite shop: Bäckerei", "I noticed it while helping")), "{about:?}");
        assert!(!about.iter().any(|(t, _)| t.starts_with("Lang")), "settings are not facts about you");
        assert!(g[1].items.is_empty() && g[1].empty == "Nothing yet.");
        assert_eq!(g[2].items[0].task, "send_au");
        assert!(g[2].items[0].detail.starts_with("Sending a sick note · once, on "), "{}", g[2].items[0].detail);
        assert_eq!(g[3].items[0].kind, ForgetKind::Trusted);
    }

    #[test]
    fn app_events_serialize_like_ui_events() {
        let v = serde_json::to_value(AppEvent::Acting { on: true }).unwrap();
        assert_eq!(v, serde_json::json!({"type":"acting","on":true}));
    }
}

/// What the user typed as their name, made into a name: "ich heiße erika." -> "Erika".
fn clean_name(raw: &str) -> String {
    let mut s = raw.trim();
    for p in [
        "ich heiße ",
        "ich heisse ",
        "ich bin ",
        "mein name ist ",
        "nenn mich ",
        "my name is ",
        "i am ",
        "i'm ",
        "call me ",
    ] {
        if s.len() > p.len() && s.is_char_boundary(p.len()) && s[..p.len()].eq_ignore_ascii_case(p) {
            s = s[p.len()..].trim_start();
            break;
        }
    }
    let s = s.trim_end_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace());
    let s: String = s.chars().take(40).collect();
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod name_tests {
    use super::clean_name;

    #[test]
    fn names() {
        assert_eq!(clean_name("  erika "), "Erika");
        assert_eq!(clean_name("Ich heiße Erika."), "Erika");
        assert_eq!(clean_name("ich bin Erika Muster!"), "Erika Muster");
        assert_eq!(clean_name("Bin"), "Bin");
        assert_eq!(clean_name("..."), "");
        assert_eq!(clean_name("Ölaf"), "Ölaf");
    }
}
