//! Event bus between core and the app (UI). Serialized to JS as
//! `{"type":"pip","state":"thinking"}` etc. (internally tagged, snake_case).
//!
//! OWNER: orchestrator (frozen). Ask the orchestrator for changes.
//! Deviation from the design note: payload variants are struct variants
//! (serde cannot internally tag a newtype holding a string or a list).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

use crate::agent::{ApprovalAnswer, ApprovalCard};
use crate::platform::PhysRect;

/// Maps to ui/pip/<png>: idle, listen, think, point, point, ask, happy, sleep, ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipState {
    Idle,
    Listening,
    Thinking,
    Pointing,
    Acting,
    Asking,
    Happy,
    Sleeping,
    Error,
}

impl PipState {
    pub fn png(self) -> &'static str {
        match self {
            PipState::Idle => "idle",
            PipState::Listening => "listen",
            PipState::Thinking => "think",
            PipState::Pointing | PipState::Acting => "point",
            PipState::Asking | PipState::Error => "ask",
            PipState::Happy => "happy",
            PipState::Sleeping => "sleep",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Button {
    pub id: String,
    pub label: String,
    pub primary: bool,
}

impl Button {
    pub fn new(id: impl Into<String>, label: impl Into<String>, primary: bool) -> Self {
        Self { id: id.into(), label: label.into(), primary }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingStep {
    /// Pip flies in, waves, asks for the name.
    AskName,
    /// "Zieh einen Brief auf mich. Oder drück hier: [Brief auswählen]"
    FirstLetter,
    /// Asked only when a deadline needs it: "Wohnst du in Nordrhein-Westfalen?"
    AskState,
    Done,
}

/// Core -> UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    Pip {
        state: PipState,
    },
    /// Max 2 sentences. Buttons optional. question_id set when the core waits for an answer.
    Bubble {
        text: String,
        buttons: Vec<Button>,
        question_id: Option<Uuid>,
    },
    /// One line under Pip while acting ("Ich trage deinen Namen ein.").
    Progress {
        text: String,
    },
    PointAt {
        rect: PhysRect,
        label: String,
    },
    PointClear,
    FlyTo {
        x: i32,
        y: i32,
    },
    FlyHome,
    ApprovalRequest {
        card: ApprovalCard,
    },
    ApprovalClosed {
        id: Uuid,
    },
    /// `text` = the two short sentences; `why` = shown behind "Warum?".
    Reminder {
        reminder_id: i64,
        text: String,
        why: String,
        buttons: Vec<Button>,
    },
    Speak {
        text: String,
    },
    Notify {
        title: String,
        body: String,
    },
    Onboarding {
        step: OnboardingStep,
    },
    Error {
        text: String,
        retry: bool,
    },
}

/// UI -> core.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserCommand {
    Approve {
        id: Uuid,
        answer: ApprovalAnswer,
    },
    Answer {
        question_id: Uuid,
        choice: String,
    },
    ReminderAnswer {
        reminder_id: i64,
        choice: String,
    },
    Stop,
    Resume,
    Quit,
    DropFiles {
        paths: Vec<PathBuf>,
    },
    PickFile,
    VoiceStart,
    VoiceStop,
    Text {
        text: String,
    },
    /// Where Pip, bubble and card are (physical px): click-through + screenshot masking.
    HitRects {
        rects: Vec<PhysRect>,
    },
    OnboardingAnswer {
        step: OnboardingStep,
        value: String,
    },
}

/// Open approvals and questions the agent is waiting for. The app's command
/// dispatcher resolves them; the agent awaits the receivers.
#[derive(Debug, Clone, Default)]
pub struct Pending {
    approvals: Arc<Mutex<HashMap<Uuid, oneshot::Sender<ApprovalAnswer>>>>,
    answers: Arc<Mutex<HashMap<Uuid, oneshot::Sender<String>>>>,
}

impl Pending {
    pub fn wait_approval(&self, id: Uuid) -> oneshot::Receiver<ApprovalAnswer> {
        let (tx, rx) = oneshot::channel();
        self.approvals.lock().expect("pending lock").insert(id, tx);
        rx
    }
    /// Returns false if nobody waits for this id (already answered / timed out).
    pub fn resolve_approval(&self, id: Uuid, answer: ApprovalAnswer) -> bool {
        let tx = self.approvals.lock().expect("pending lock").remove(&id);
        tx.is_some_and(|tx| tx.send(answer).is_ok())
    }
    pub fn wait_answer(&self, question_id: Uuid) -> oneshot::Receiver<String> {
        let (tx, rx) = oneshot::channel();
        self.answers.lock().expect("pending lock").insert(question_id, tx);
        rx
    }
    pub fn resolve_answer(&self, question_id: Uuid, choice: String) -> bool {
        let tx = self.answers.lock().expect("pending lock").remove(&question_id);
        tx.is_some_and(|tx| tx.send(choice).is_ok())
    }
}

/// Shared handle. Clone freely.
#[derive(Debug, Clone)]
pub struct Bus {
    pub to_ui: broadcast::Sender<UiEvent>,
    pub from_ui: mpsc::Sender<UserCommand>,
    pub pending: Pending,
    /// Latest HitRects from the UI (physical px).
    pub hit_rects: Arc<RwLock<Vec<PhysRect>>>,
}

impl Bus {
    /// Returns the bus and the single receiver of user commands (owned by the app's dispatcher).
    pub fn new() -> (Self, mpsc::Receiver<UserCommand>) {
        let (to_ui, _) = broadcast::channel(256);
        let (from_ui, rx) = mpsc::channel(64);
        let bus = Self { to_ui, from_ui, pending: Pending::default(), hit_rects: Arc::default() };
        (bus, rx)
    }
    /// Send to the UI; ignores "no receiver".
    pub fn emit(&self, ev: UiEvent) {
        let _ = self.to_ui.send(ev);
    }
    pub fn hit_rects(&self) -> Vec<PhysRect> {
        self.hit_rects.read().expect("hit_rects lock").clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_serialize_internally_tagged() {
        let v = serde_json::to_value(UiEvent::Pip { state: PipState::Thinking }).unwrap();
        assert_eq!(v, serde_json::json!({"type":"pip","state":"thinking"}));
        let v = serde_json::to_value(UiEvent::PointClear).unwrap();
        assert_eq!(v, serde_json::json!({"type":"point_clear"}));
        let c: UserCommand =
            serde_json::from_value(serde_json::json!({"type":"hit_rects","rects":[{"x":1,"y":2,"w":3,"h":4}]}))
                .unwrap();
        assert!(matches!(c, UserCommand::HitRects { rects } if rects.len() == 1));
        let c: UserCommand = serde_json::from_value(serde_json::json!({"type":"stop"})).unwrap();
        assert!(matches!(c, UserCommand::Stop));
    }

    #[tokio::test]
    async fn pending_roundtrip() {
        let p = Pending::default();
        let id = Uuid::new_v4();
        let rx = p.wait_approval(id);
        assert!(p.resolve_approval(id, ApprovalAnswer::Yes));
        assert_eq!(rx.await.unwrap(), ApprovalAnswer::Yes);
        assert!(!p.resolve_approval(id, ApprovalAnswer::No));
    }
}
