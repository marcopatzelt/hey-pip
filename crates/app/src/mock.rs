//! `PIP_MOCK=1`: a scripted demo of every overlay element without the core services
//! (no API, no database, no mouse control). Used to design and test the UI on its own.
//! All data here is invented.

use std::time::Duration;

use pip_core::agent::{APPROVAL_TIMEOUT, AgentOutcome, ApprovalAnswer, ApprovalCard, CancellationToken};
use pip_core::events::{Bus, Button, PipState, UiEvent};
use pip_core::i18n::{Lang, t, tf};
use pip_core::platform::PhysRect;
use uuid::Uuid;

use crate::overlay::OverlayInfo;

/// Sleep unless cancelled. Returns false when cancelled.
async fn pause(token: &CancellationToken, ms: u64, pace: f32) -> bool {
    tokio::select! {
        biased;
        _ = token.cancelled() => false,
        _ = tokio::time::sleep(Duration::from_millis((ms as f32 * pace) as u64)) => true,
    }
}

/// A rect at fractions of the screen (the fake form sits in the left half).
fn frac(s: &OverlayInfo, x: f32, y: f32, w: f32, h: f32) -> PhysRect {
    PhysRect {
        x: s.x + (s.w as f32 * x) as i32,
        y: s.y + (s.h as f32 * y) as i32,
        w: (s.w as f32 * w) as i32,
        h: (s.h as f32 * h) as i32,
    }
}

fn pick<'a>(lang: Lang, de: &'a str, en: &'a str) -> &'a str {
    if lang == Lang::En { en } else { de }
}

pub struct MockLetter {
    pub bubble: String,
}

pub async fn ingest(lang: Lang) -> MockLetter {
    tokio::time::sleep(Duration::from_millis(1800)).await;
    MockLetter { bubble: tf("doc_au_summary", lang, &[("from", "20.10."), ("until", "30.10.")]) }
}

pub fn reminder(lang: Lang) -> UiEvent {
    let due = pick(lang, "Montag, 2. November", "Monday, November 2");
    UiEvent::Reminder {
        reminder_id: 1,
        text: format!("{} {}", t("rem_au_ends_tomorrow", lang), tf("rem_au_need_new", lang, &[("due", due)])),
        why: format!("{} {}", t("rem_au_why", lang), t("hint_not_legal_advice", lang)),
        buttons: vec![
            Button::new("remind:2026-11-02T08:00:00+01:00", t("btn_remind_monday", lang), true),
            Button::new("done", t("btn_healthy", lang), false),
            Button::new("later", t("btn_later", lang), false),
        ],
    }
}

pub fn reminder_answer(choice: &str, lang: Lang) -> String {
    if choice.starts_with("remind:") {
        pick(lang, "Gut. Ich erinnere dich Montag um 8 Uhr.", "OK. I'll remind you on Monday at 8.").into()
    } else if choice == "done" {
        pick(
            lang,
            "Schön, dass es dir besser geht! Ich passe nicht mehr auf diese Frist auf.",
            "Glad you're better! I'll stop watching this deadline.",
        )
        .into()
    } else {
        pick(lang, "Gut, ich melde mich in einer Stunde nochmal.", "OK, I'll check back in an hour.").into()
    }
}

/// The demo story: fill the invented health-insurance form, two approval cards, done.
/// `PIP_MOCK_SCENE=guard`: the lookalike-page scene instead (the amber guard bubble, for filming).
pub async fn run_task(bus: &Bus, screen: OverlayInfo, lang: Lang, token: CancellationToken) -> AgentOutcome {
    if std::env::var("PIP_MOCK_SCENE").is_ok_and(|v| v.trim() == "guard") {
        return run_guard_paced(bus, screen, lang, token, 1.0).await;
    }
    run_task_paced(bus, screen, lang, token, 1.0).await
}

/// The guard bubble as the core sends it: an ordinary bubble with "guard:" buttons; line 1 = title, one line per
/// warning sign, last line = the honest closing sentence. All texts invented.
pub fn guard_bubble(lang: Lang, question_id: Option<Uuid>) -> UiEvent {
    let lines = [
        pick(
            lang,
            "Vorsicht: Diese Seite sieht nur aus wie die Gesundkasse.",
            "Careful: this page only looks like Gesundkasse.",
        ),
        pick(lang, "Die Adresse ist nicht die aus deinen Briefen", "The web address is not the one from your letters"),
        pick(lang, "Sie will deinen SMS-Code", "It asks for your SMS code"),
        pick(lang, "Sie sagt, du musst dich beeilen", "It says you must hurry"),
        pick(
            lang,
            "Ich kann mich irren. Du entscheidest, und ich habe hier nichts eingetippt.",
            "I can be wrong. You decide, and I typed nothing here.",
        ),
    ];
    UiEvent::Bubble {
        text: lines.join("\n"),
        buttons: vec![
            Button::new(
                "guard:real",
                pick(lang, "Bring mich zur echten Gesundkasse", "Take me to the real Gesundkasse"),
                true,
            ),
            Button::new("guard:continue", pick(lang, "Ich bin sicher, weiter", "I'm sure, continue"), false),
        ],
        question_id,
    }
}

/// The lookalike-page scene: Pip looks at the page, stops before typing anything, asks with the amber bubble.
async fn run_guard_paced(
    bus: &Bus,
    screen: OverlayInfo,
    lang: Lang,
    token: CancellationToken,
    pace: f32,
) -> AgentOutcome {
    bus.emit(UiEvent::Pip { state: PipState::Thinking });
    bus.emit(UiEvent::Progress { text: t("thinking", lang).into() });
    if !pause(&token, 2500, pace).await {
        return AgentOutcome::Stopped;
    }
    bus.emit(UiEvent::PointAt {
        rect: frac(&screen, 0.18, 0.50, 0.30, 0.05),
        label: pick(lang, "SMS-Code", "SMS code").into(),
    });
    bus.emit(UiEvent::Progress { text: String::new() });
    bus.emit(UiEvent::Pip { state: PipState::Asking });
    let qid = Uuid::new_v4();
    let rx = bus.pending.wait_answer(qid);
    let ev = guard_bubble(lang, Some(qid));
    if let UiEvent::Bubble { text, .. } = &ev {
        bus.emit(UiEvent::Speak { text: text.lines().next().unwrap_or_default().to_string() });
    }
    bus.emit(ev);
    let answer = tokio::select! {
        biased;
        _ = token.cancelled() => None,
        a = tokio::time::timeout(APPROVAL_TIMEOUT, rx) => a.ok().and_then(Result::ok),
    };
    bus.emit(UiEvent::PointClear);
    bus.emit(UiEvent::FlyHome);
    let Some(choice) = answer else {
        bus.emit(UiEvent::Pip { state: PipState::Idle });
        return AgentOutcome::Stopped;
    };
    let text = if choice == "guard:continue" {
        pick(
            lang,
            "Gut, du entscheidest. Ich tippe trotzdem nichts Geheimes ein.",
            "OK, you decide. I still won't type anything secret.",
        )
    } else {
        pick(
            lang,
            "Gut. Wir nehmen die echte Seite. Diese hier fasse ich nicht an.",
            "Good. We'll use the real page. I won't touch this one.",
        )
    };
    bus.emit(UiEvent::Pip { state: PipState::Happy });
    bus.emit(UiEvent::Bubble { text: text.into(), buttons: vec![], question_id: None });
    bus.emit(UiEvent::Speak { text: text.into() });
    AgentOutcome::Done(text.into())
}

/// `pace` scales every wait (tests use 0).
async fn run_task_paced(
    bus: &Bus,
    screen: OverlayInfo,
    lang: Lang,
    token: CancellationToken,
    pace: f32,
) -> AgentOutcome {
    let step = |state: PipState, progress: String| {
        bus.emit(UiEvent::Pip { state });
        bus.emit(UiEvent::Progress { text: progress });
    };
    step(PipState::Thinking, t("thinking", lang).into());
    if !pause(&token, 3500, pace).await {
        return AgentOutcome::Stopped;
    }
    step(PipState::Acting, t("progress_open_page", lang).into());
    if !pause(&token, 1200, pace).await {
        return AgentOutcome::Stopped;
    }
    let fields = [
        (frac(&screen, 0.18, 0.30, 0.30, 0.05), pick(lang, "Name", "Name"), t("progress_type_name", lang).to_string()),
        (
            frac(&screen, 0.18, 0.40, 0.30, 0.05),
            pick(lang, "Versichertennummer", "Insurance number"),
            tf("progress_type", lang, &[("what", pick(lang, "deine Versichertennummer", "your insurance number"))]),
        ),
    ];
    for (rect, label, progress) in fields {
        bus.emit(UiEvent::PointAt { rect, label: label.into() });
        step(PipState::Acting, progress);
        if !pause(&token, 2200, pace).await {
            return AgentOutcome::Stopped;
        }
    }

    let what = pick(lang, "deine Krankschreibung 20.10.–30.10.", "your sick note 20.10.–30.10.");
    let to = pick(lang, "Gesundkasse (Kontaktformular)", "Gesundkasse (contact form)");
    let cards = [
        (
            frac(&screen, 0.18, 0.50, 0.14, 0.05),
            pick(lang, "Datei anhängen", "Attach file"),
            "approve_title_attach",
            "btn_yes_attach",
        ),
        (frac(&screen, 0.36, 0.62, 0.12, 0.06), pick(lang, "Absenden", "Send"), "approve_title_send", "btn_yes_send"),
    ];
    for (rect, label, title, yes) in cards {
        bus.emit(UiEvent::PointAt { rect, label: label.into() });
        step(PipState::Asking, String::new());
        let send = title == "approve_title_send";
        let card = ApprovalCard {
            id: Uuid::new_v4(),
            title: tf(title, lang, &[("to", "Gesundkasse")]),
            what: tf("approve_what", lang, &[("what", what)]),
            to: tf("approve_to", lang, &[("to", to)]),
            attachments: vec!["krankschreibung_beispiel.pdf".into()],
            filled: vec!["Name: Erika Beispiel".into(), "Versichertennummer: A123456780".into()],
            crop_png_b64: None,
            yes_label: t(yes, lang).into(),
            no_label: t("btn_not_yet", lang).into(),
            remember_label: send.then(|| tf("approve_remember_site", lang, &[("to", "Gesundkasse")])),
            remembered: Vec::new(),
        };
        let id = card.id;
        let rx = bus.pending.wait_approval(id);
        bus.emit(UiEvent::ApprovalRequest { card });
        let answer = tokio::select! {
            biased;
            _ = token.cancelled() => None,
            a = tokio::time::timeout(APPROVAL_TIMEOUT, rx) => Some(match a {
                Ok(Ok(a)) => a,
                _ => ApprovalAnswer::Timeout,
            }),
        };
        bus.emit(UiEvent::ApprovalClosed { id });
        match answer {
            None => return AgentOutcome::Stopped,
            Some(ApprovalAnswer::Yes) => {
                step(PipState::Acting, tf("progress_click", lang, &[("target", label)]));
                if !pause(&token, 1400, pace).await {
                    return AgentOutcome::Stopped;
                }
            }
            Some(a) => {
                let key = if a == ApprovalAnswer::Timeout { "approve_timeout" } else { "approve_not_sent" };
                bus.emit(UiEvent::PointClear);
                bus.emit(UiEvent::FlyHome);
                bus.emit(UiEvent::Progress { text: String::new() });
                bus.emit(UiEvent::Pip { state: PipState::Idle });
                bus.emit(UiEvent::Bubble { text: t(key, lang).into(), buttons: vec![], question_id: None });
                return AgentOutcome::NeedsUser(t(key, lang).into());
            }
        }
    }
    bus.emit(UiEvent::PointClear);
    bus.emit(UiEvent::FlyHome);
    bus.emit(UiEvent::Progress { text: String::new() });
    bus.emit(UiEvent::Pip { state: PipState::Happy });
    let fact = pick(lang, "Krankschreibung ist bei der Kasse.", "the sick note is with the insurer.");
    let done = tf("done_remember", lang, &[("fact", fact)]);
    bus.emit(UiEvent::Bubble { text: done.clone(), buttons: vec![], question_id: None });
    bus.emit(UiEvent::Speak { text: done.clone() });
    tokio::time::sleep(Duration::from_millis((2000.0 * pace) as u64)).await;
    bus.emit(UiEvent::Pip { state: PipState::Idle });
    AgentOutcome::Done(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> OverlayInfo {
        OverlayInfo { x: 0, y: 0, w: 1920, h: 1080, scale: 1.0, windowed: false }
    }

    #[tokio::test]
    async fn story_runs_to_done_when_approved() {
        let (bus, _rx) = pip_core::events::Bus::new();
        let mut ui = bus.to_ui.subscribe();
        let b2 = bus.clone();
        let approver = tokio::spawn(async move {
            let mut cards = 0;
            while let Ok(ev) = ui.recv().await {
                if let UiEvent::ApprovalRequest { card } = &ev {
                    cards += 1;
                    assert!(!card.yes_label.is_empty() && !card.no_label.is_empty());
                    // the waiter is registered before the event is sent
                    assert!(b2.pending.resolve_approval(card.id, ApprovalAnswer::Yes));
                }
                if let UiEvent::Pip { state: PipState::Happy } = ev {
                    return cards;
                }
            }
            cards
        });
        let out = run_task_paced(&bus, screen(), Lang::De, CancellationToken::new(), 0.0).await;
        assert!(matches!(out, AgentOutcome::Done(ref s) if s.starts_with("Erledigt.")), "{out:?}");
        assert_eq!(approver.await.unwrap(), 2);
    }

    #[tokio::test]
    async fn no_means_nothing_sent() {
        let (bus, _rx) = pip_core::events::Bus::new();
        let mut ui = bus.to_ui.subscribe();
        let b2 = bus.clone();
        tokio::spawn(async move {
            while let Ok(ev) = ui.recv().await {
                if let UiEvent::ApprovalRequest { card } = ev {
                    b2.pending.resolve_approval(card.id, ApprovalAnswer::No);
                }
            }
        });
        let out = run_task_paced(&bus, screen(), Lang::De, CancellationToken::new(), 0.0).await;
        assert_eq!(out, AgentOutcome::NeedsUser(t("approve_not_sent", Lang::De).into()));
    }

    #[tokio::test]
    async fn stop_ends_the_story() {
        let (bus, _rx) = pip_core::events::Bus::new();
        let token = CancellationToken::new();
        token.cancel();
        assert_eq!(run_task_paced(&bus, screen(), Lang::En, token, 0.0).await, AgentOutcome::Stopped);
    }

    #[tokio::test]
    async fn guard_scene_asks_with_guard_buttons_and_never_types() {
        let (bus, _rx) = pip_core::events::Bus::new();
        let mut ui = bus.to_ui.subscribe();
        let b2 = bus.clone();
        let seen = tokio::spawn(async move {
            while let Ok(ev) = ui.recv().await {
                // Pip only looks and asks here: never the acting state, never a typing step
                assert!(!matches!(ev, UiEvent::Pip { state: PipState::Acting }), "the guard scene never acts");
                if let UiEvent::Progress { text } = &ev {
                    assert!(!text.contains(t("progress_type_name", Lang::En)), "typed: {text}");
                }
                if let UiEvent::Bubble { text, buttons, question_id: Some(q) } = ev {
                    assert!(buttons.iter().all(|b| b.id.starts_with("guard:")));
                    assert_eq!(buttons[0].id, "guard:real");
                    assert!(buttons[0].primary);
                    assert_eq!(text.lines().count(), 5, "title, three signs, closing");
                    assert!(b2.pending.resolve_answer(q, "guard:real".into()));
                    return true;
                }
            }
            false
        });
        let out = run_guard_paced(&bus, screen(), Lang::En, CancellationToken::new(), 0.0).await;
        assert!(seen.await.unwrap());
        assert!(matches!(out, AgentOutcome::Done(ref s) if s.starts_with("Good.")), "{out:?}");
    }

    #[test]
    fn reminder_has_three_buttons_and_hint() {
        let UiEvent::Reminder { buttons, why, .. } = reminder(Lang::De) else { panic!() };
        assert_eq!(buttons.len(), 3);
        assert!(why.ends_with(t("hint_not_legal_advice", Lang::De)));
    }
}
