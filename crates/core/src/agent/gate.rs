//! Approval gate. Commit is detected at three places, one is enough: the model's is_commit,
//! the word list on description/sam_query, the UI-tree name at the click point.
//! OWNER: agent "agent".

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::tools::Tool;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    /// read, point, remember
    Free,
    /// click, type (visible, stoppable)
    Act,
    /// send, submit, attach a private file, pay, delete -> approval card
    Commit,
}

pub const COMMIT_WORDS: &[&str] = &[
    "senden",
    "absenden",
    "abschicken",
    "abschließen",
    "hochladen",
    "bezahlen",
    "kaufen",
    "löschen",
    "kündigen",
    "bestellen",
    "submit",
    "send",
    "pay",
    "buy",
    "upload",
    "delete",
    "order",
    "confirm",
];

/// Shown before every Commit. Title e.g. "Darf ich das abschicken?"; buttons "Ja, abschicken" / "Noch nicht".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalCard {
    pub id: Uuid,
    pub title: String,
    /// "Was: deine Krankschreibung 20.10.–30.10."
    pub what: String,
    /// "An: Evergreen Health (Kontaktformular)"
    pub to: String,
    pub attachments: Vec<String>,
    /// What Pip typed into the form in this run ("Name: Erika Beispiel"), so the user can check it.
    #[serde(default)]
    pub filled: Vec<String>,
    /// Thumbnail of the target button (PNG, base64, no data: prefix).
    pub crop_png_b64: Option<String>,
    /// Primary button label ("Ja, abschicken" / "Ja, anhängen").
    pub yes_label: String,
    /// Secondary button label ("Noch nicht").
    pub no_label: String,
    /// Send cards on a known site: the checkbox text "Don't ask again for www.example.org". None = no checkbox.
    #[serde(default)]
    pub remember_label: Option<String>,
    /// Facts Pip learned in this run ("I'll remember: insurer e-mail: …"); a Send confirms the held-back ones.
    #[serde(default)]
    pub remembered: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalAnswer {
    Yes,
    No,
    Timeout,
}

#[derive(Debug, Clone, Default)]
pub struct Gate {
    pub demo_mode: bool,
    /// DEMO_MODE: folders whose local pages may be opened and sent (file-URL form, lowercase, no trailing '/').
    fixture_roots: Vec<String>,
}

/// Extra phrases (not in the public list): attaching a private file, payment wording.
const EXTRA_COMMIT: &[&str] = &[
    "anhängen",
    "anhang hinzufügen",
    "versenden",
    "verschicken",
    "übermitteln",
    "bestätigen",
    "zahlen",
    "datei auswählen",
    "durchsuchen",
    "zahlungspflichtig",
    "kostenpflichtig",
    "überweisen",
    "attach",
    "choose file",
    "browse",
    "purchase",
    "checkout",
];

/// Final or irreversible actions that are too common as plain words to match in free text
/// ("Formular Unterlagen einreichen, Feld Name"). They count on the UI-tree name of the target and on
/// the quoted label in the description („Antrag einreichen“). A card too many costs little.
const LABEL_COMMIT: &[&str] = &[
    "einreich",
    "beantrag",
    "antrag stellen",
    "buchen",
    "abonnier",
    "weiterleit",
    "antwort",
    "veröffentlich",
    "entfern",
    "überweis",
    "ausführen",
    "freigeb",
    "unterschreib",
    "stornier",
    "widerruf",
    "teilen",
    "book",
    "subscribe",
    "forward",
    "reply",
    "publish",
    "post",
    "remove",
    "transfer",
    "sign",
    "share",
];

/// Whole UI names that confirm whatever a dialog asks ("Wirklich löschen?" [Ja] [Nein]).
const CONFIRM_NAMES: &[&str] = &["ja", "ok", "okay", "yes"];

/// German / long words: substring match ("Formular absenden", "Antrag übermitteln").
/// Short English words: whole word or word + s/ed/ing/en ("Send", "sending"), so "Sender"
/// or "Ordner" do not trigger.
pub(crate) fn word_hit(text: &str) -> bool {
    hit_in(text, COMMIT_WORDS.iter().chain(EXTRA_COMMIT))
}

/// word_hit plus the label-only stems and the bare "Ja"/"OK" of a confirm dialog.
/// Use it on UI-tree names and quoted labels, not on free text.
pub(crate) fn label_hit(label: &str) -> bool {
    let low = label.trim().to_lowercase();
    if low.is_empty() {
        return false;
    }
    if CONFIRM_NAMES.contains(&low.trim_end_matches(['!', '.'])) {
        return true;
    }
    // "Sign in" / "Sign up" are logins, not signatures.
    let signin = ["sign in", "sign-in", "sign up", "sign-up", "signin", "signup"].iter().any(|w| low.contains(w));
    word_hit(label) || hit_in(label, LABEL_COMMIT.iter().filter(|w| !(signin && **w == "sign")))
}

fn hit_in<'a>(text: &str, words: impl Iterator<Item = &'a &'a str>) -> bool {
    let low = text.to_lowercase();
    let tokens: Vec<&str> = low.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).collect();
    words.into_iter().any(|w| {
        if w.contains(' ') || !w.is_ascii() || w.len() >= 7 {
            low.contains(w)
        } else {
            tokens.iter().any(|t| {
                t.strip_prefix(w).is_some_and(|rest| matches!(rest, "" | "s" | "ed" | "ing" | "en" | "button"))
            })
        }
    })
}

/// The label inside quotes in a description: „Absenden“, "Absenden", “Absenden”, 'Absenden'. None if unquoted.
pub fn quoted_label(description: &str) -> Option<String> {
    for (open, close) in [('„', '“'), ('"', '"'), ('“', '”'), ('»', '«'), ('«', '»'), ('\'', '\'')] {
        if let Some(a) = description.find(open) {
            let rest = &description[a + open.len_utf8()..];
            if let Some(b) = rest.find(close) {
                let label = rest[..b].trim();
                if !label.is_empty() && label.chars().count() <= 80 {
                    return Some(label.to_string());
                }
            }
        }
    }
    None
}

/// True if the text looks like attaching / uploading a file (card title "anhängen" instead of "abschicken").
pub(crate) fn is_attach_like(text: &str) -> bool {
    let low = text.to_lowercase();
    ["anhäng", "anhang", "hochlad", "upload", "attach", "datei", "file", "durchsuchen", "browse"]
        .iter()
        .any(|w| low.contains(w))
}

/// True if the target looks like a send / submit button (not a file picker).
pub(crate) fn is_send_target(target: &str) -> bool {
    let low = target.to_lowercase();
    [
        "absend",
        "abschick",
        "senden",
        "submit",
        "send",
        "verschick",
        "versend",
        "übermitt",
        "einreich",
        "bestell",
        "order",
    ]
    .iter()
    .any(|w| low.contains(w))
}

/// True if the click target (description, SAM query, UI name) is an attach / file button and not a send button.
/// Only the target counts: "Was: deine Krankschreibung (Datei au.pdf)" on an "Absenden" card is still a send.
pub(crate) fn is_attach_target(target: &str) -> bool {
    is_attach_like(target) && !is_send_target(target)
}

/// True if the target deletes something (card title "löschen").
pub(crate) fn is_delete_target(target: &str) -> bool {
    let low = target.to_lowercase();
    ["lösch", "entfern", "delete", "remove", "papierkorb", "trash", "kündig", "stornier"]
        .iter()
        .any(|w| low.contains(w))
}

/// True if the target pays, buys, orders, books or subscribes (never sent without a card, even on a trusted site).
pub(crate) fn is_money_target(target: &str) -> bool {
    let low = target.to_lowercase();
    let stems = [
        "bezahl",
        "zahlung",
        "zahlen",
        "kauf",
        "bestell",
        "überweis",
        "buch",
        "abonnier",
        "kostenpflichtig",
        "pay",
        "buy",
        "purchase",
        "checkout",
        "order",
        "transfer",
        "book",
        "subscri",
    ];
    stems.iter().any(|w| low.contains(w))
}

/// True if the target cancels a contract or booking (never sent without a card, even on a trusted site).
pub(crate) fn is_cancel_target(target: &str) -> bool {
    let low = target.to_lowercase();
    ["kündig", "stornier", "widerruf", "cancel", "terminate", "unsubscri", "abmeld"].iter().any(|w| low.contains(w))
}

/// Key combos that send, confirm or delete in common software: Enter, Ctrl+Enter; Alt (+Shift) + a letter or
/// digit (HTML accesskey, Outlook Alt+S, dialog mnemonics &Ja); Delete, Shift+Delete, Ctrl+D.
fn key_commits(combo: &str) -> bool {
    let low = combo.to_lowercase();
    let parts: Vec<&str> = low.split('+').map(str::trim).collect();
    let last = parts.last().copied().unwrap_or("");
    let alt_mnemonic = parts.contains(&"alt") && last.chars().count() == 1 && last.chars().all(|c| c.is_alphanumeric());
    matches!(last, "enter" | "return" | "delete" | "del") || alt_mnemonic || (parts.contains(&"ctrl") && last == "d")
}

impl Gate {
    pub fn new(demo_mode: bool) -> Self {
        Self { demo_mode, fixture_roots: Vec::new() }
    }

    /// DEMO_MODE: local pages under these folders may be opened and sent (e.g. Config::fixtures_dir).
    /// Folders are canonicalized when they exist; without any folder no file:// page is allowed.
    pub fn with_fixtures<P: AsRef<std::path::Path>>(mut self, dirs: &[P]) -> Self {
        for d in dirs {
            let d = d.as_ref();
            let d = d.canonicalize().unwrap_or_else(|_| d.to_path_buf());
            let url = crate::config::file_url(&d).to_lowercase();
            let root = url.trim_end_matches('/').to_string();
            if !self.fixture_roots.contains(&root) {
                self.fixture_roots.push(root);
            }
        }
        self
    }

    /// The key of the site a page belongs to, for "Don't ask again for this site" and the per-site playbooks:
    /// the host of an http(s) page, or (demo mode) the demo site folder of a local page: the first folder
    /// below the fixtures root ("…/fixtures/demo-uni"), or the root itself for a file directly in it.
    /// So the invented insurer and the invented university are two sites. None = no key (other file:// pages, garbage).
    pub fn site_key(&self, url: &str) -> Option<String> {
        let low = url.trim().to_lowercase();
        if low.starts_with("file:") {
            if !self.url_allowed(url) {
                return None;
            }
            let path = low.strip_prefix("file://")?;
            let path = path.strip_prefix("localhost").unwrap_or(path).replace('\\', "/");
            let full = format!("file://{path}");
            let root = self.fixture_roots.iter().find(|root| full.starts_with(&format!("{root}/")))?;
            let rest = full[root.len() + 1..].split(['?', '#']).next().unwrap_or("");
            return Some(match rest.split_once('/') {
                Some((folder, _)) if !folder.is_empty() => format!("{root}/{folder}"),
                _ => root.clone(),
            });
        }
        url_host(url)
    }

    /// What the user would call the site of a page, for sentences: the host of an http(s) page
    /// ("www.evergreen-health.example"), or the demo folder of a local page ("demo-insurer"). None like `site_key`.
    pub fn site_label(&self, url: &str) -> Option<String> {
        let key = self.site_key(url)?;
        if url.trim().to_lowercase().starts_with("file:") {
            return key.rsplit('/').next().filter(|s| !s.is_empty()).map(str::to_string);
        }
        Some(key)
    }

    /// Commit if is_commit || COMMIT_WORDS hit (case-insensitive, word-ish) in description/sam_query,
    /// or the UI name / quoted label looks final (`label_hit`). Keys: see `key_commits`; Space when the
    /// focused element looks like a send button or is unknown (ui_name None).
    /// Click/TypeText/Key/Scroll/OpenUrl = Act; the rest Free.
    /// Pip never closes, minimizes or quits the user's windows (unsaved work, the user's own programs).
    /// A popup's own close button ("X vom Hinweis") stays allowed.
    pub fn closes_window(description: &str, sam_query: &str, ui_name: &str) -> bool {
        let text = format!("{description} {sam_query}").to_lowercase();
        let ui = ui_name.trim().to_lowercase();
        let popup = ["hinweis", "popup", "pop-up", "meldung", "banner", "cookie", "dialog", "notice", "tooltip"]
            .iter()
            .any(|w| text.contains(w));
        let window = ["fenster", "window", "programm", "app ", "anwendung", "powershell", "terminal"]
            .iter()
            .any(|w| text.contains(w));
        let closing =
            ["schließ", "schliess", "close", "minimi", "beend", "quit", "exit"].iter().any(|w| text.contains(w));
        let ui_close = matches!(ui.as_str(), "schließen" | "close" | "minimieren" | "minimize" | "fenster schließen");
        !popup && ((window && closing) || (ui_close && window))
    }

    pub fn classify(&self, tool: &Tool, ui_name: Option<&str>) -> Risk {
        match tool {
            Tool::Click { sam_query, description, is_commit, .. } => {
                let ui_hit = ui_name.is_some_and(label_hit);
                let quoted = quoted_label(description).is_some_and(|l| label_hit(&l));
                if *is_commit || ui_hit || quoted || word_hit(description) || word_hit(sam_query) {
                    Risk::Commit
                } else {
                    Risk::Act
                }
            }
            // Enter in a form submits it, so Enter is always a commit (the loop lets one Enter through after
            // an approved attach while the file dialog is open). Space activates a focused button: commit when
            // that button looks final, or when nobody can tell what has the focus.
            Tool::Key { combo, is_commit, .. } => {
                let low = combo.to_lowercase();
                let press = low.split('+').next_back().map(str::trim) == Some("space");
                let space_hits = press && ui_name.is_none_or(label_hit);
                if *is_commit || key_commits(combo) || space_hits { Risk::Commit } else { Risk::Act }
            }
            Tool::TypeText { .. } | Tool::Scroll { .. } | Tool::OpenUrl { .. } => Risk::Act,
            Tool::Say { .. }
            | Tool::Point { .. }
            | Tool::Wait { .. }
            | Tool::RememberFact { .. }
            | Tool::SearchMemory { .. }
            | Tool::AddDeadline { .. }
            | Tool::AskUser { .. }
            | Tool::Done { .. }
            | Tool::Suggest { .. } => Risk::Free,
        }
    }

    /// http(s) only. DEMO_MODE: only localhost / 127.0.0.1 / [::1], or file:///… inside a fixtures folder
    /// given to `with_fixtures`.
    pub fn url_allowed(&self, url: &str) -> bool {
        let url = url.trim();
        if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return false;
        }
        let low = url.to_lowercase();
        if let Some(rest) = low.strip_prefix("file://") {
            // Local demo pages only, and only in demo mode.
            // Empty host only: "file://server/share" or "file:////server/share" opens a network path on Windows.
            let path = rest.strip_prefix("localhost").unwrap_or(rest).replace('\\', "/");
            if !self.demo_mode || !path.starts_with('/') || path.starts_with("//") {
                return false;
            }
            if path.contains("..") || path.contains("%2e") || path.contains("%2f") || path.contains("%5c") {
                return false;
            }
            let full = format!("file://{path}");
            return self.fixture_roots.iter().any(|root| full.starts_with(&format!("{root}/")));
        }
        let Some(rest) = low.strip_prefix("https://").or_else(|| low.strip_prefix("http://")) else {
            return false;
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        if authority.is_empty() || authority.contains('@') {
            return false; // "http://localhost@evil.example" tricks
        }
        if !self.demo_mode {
            return true;
        }
        matches!(host_of_authority(authority).as_str(), "localhost" | "127.0.0.1" | "[::1]")
    }
}

/// "localhost:8000" -> "localhost", "[::1]:9000" -> "[::1]".
fn host_of_authority(authority: &str) -> String {
    if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().map(|h| format!("[{h}]")).unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or("").to_string()
    }
}

/// Host of an http(s) URL ("upload.evil.example"); "Lokale Seite" style callers handle file:// themselves.
pub fn url_host(url: &str) -> Option<String> {
    let low = url.trim().to_lowercase();
    let rest = low.strip_prefix("https://").or_else(|| low.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = host_of_authority(authority);
    (!host.is_empty()).then_some(host)
}

/// What a browser address bar shows -> a full URL: "localhost:8000/x" -> "http://localhost:8000/x",
/// "www.aok.de/kontakt" -> "https://www.aok.de/kontakt", "C:/x/y.html" or "C:\x\y.html" -> "file:///C:/x/y.html".
pub fn normalize_address(shown: &str) -> Option<String> {
    let s = shown.trim();
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        return None;
    }
    let low = s.to_lowercase();
    if low.starts_with("http://") || low.starts_with("https://") || low.starts_with("file://") {
        return Some(s.to_string());
    }
    let b = s.as_bytes();
    if b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\') {
        return Some(format!("file:///{}", s.replace('\\', "/")));
    }
    if low.contains("://") || !s.contains('.') && !low.starts_with("localhost") {
        return None; // other schemes (edge://, about:) or no host
    }
    let local = low.starts_with("localhost") || low.starts_with("127.0.0.1") || low.starts_with("[::1]");
    Some(format!("{}{s}", if local { "http://" } else { "https://" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(desc: &str, query: &str, is_commit: bool) -> Tool {
        Tool::Click {
            sam_query: query.into(),
            description: desc.into(),
            is_commit,
            commit_what: None,
            commit_to: None,
            element: None,
        }
    }

    #[test]
    fn three_sources_each_enough() {
        let g = Gate::new(true);
        // 1. model flag
        assert_eq!(g.classify(&click("den blauen Knopf", "blue button", true), None), Risk::Commit);
        // 2. word list on description / sam_query
        assert_eq!(g.classify(&click("den Knopf „Absenden“", "blue button", false), None), Risk::Commit);
        assert_eq!(g.classify(&click("the button", "submit button", false), None), Risk::Commit);
        // 3. UI-tree name at the click point
        assert_eq!(g.classify(&click("den blauen Knopf", "blue button", false), Some("Jetzt kaufen")), Risk::Commit);
        // none
        assert_eq!(g.classify(&click("das Feld Name", "text input field", false), Some("Name")), Risk::Act);
    }

    #[test]
    fn word_list_de_en() {
        for s in [
            "Senden",
            "Formular absenden",
            "Abschicken",
            "Datei hochladen",
            "Jetzt bezahlen",
            "Zahlungspflichtig bestellen",
            "Konto löschen",
            "Vertrag kündigen",
            "Krankschreibung anhängen",
            "Datei auswählen",
            "Send",
            "SUBMIT",
            "Pay now",
            "Buy",
            "Upload file",
            "Delete account",
            "Place order",
            "Confirm",
            "submitButton",
            "Jetzt versenden",
            "Antrag übermitteln",
            "Eingaben bestätigen",
            "Jetzt zahlen",
        ] {
            assert!(word_hit(s), "{s} should be commit");
        }
        for s in [
            "Weiter",
            "Name",
            "Absender",
            "Sender name",
            "Ordner",
            "Zurück",
            "Versichertennummer",
            "Next",
            "Payload",
            "Formular Unterlagen einreichen, Feld Name",
            "Eingangsbestätigung",
        ] {
            assert!(!word_hit(s), "{s} should not be commit");
        }
    }

    #[test]
    fn enter_on_send_button_is_commit() {
        let g = Gate::new(false);
        let key = |c: &str| Tool::Key { combo: c.into(), is_commit: false, commit_what: None, commit_to: None };
        // Enter submits a form even from a text field: always a commit.
        assert_eq!(g.classify(&key("enter"), None), Risk::Commit);
        assert_eq!(g.classify(&key("enter"), Some("Name")), Risk::Commit);
        assert_eq!(g.classify(&key("ctrl+enter"), None), Risk::Commit);
        assert_eq!(g.classify(&key("space"), Some("Absenden")), Risk::Commit);
        assert_eq!(g.classify(&key("space"), Some("Name")), Risk::Act);
        // Nobody knows what has the focus: Space might press a send button.
        assert_eq!(g.classify(&key("space"), None), Risk::Commit);
        assert_eq!(g.classify(&key("shift+delete"), None), Risk::Commit);
        // Delete removes mail or files; Alt+letter is an access key (submit, Outlook "Senden", dialog "&Ja").
        for c in ["delete", "ctrl+d", "alt+s", "alt+shift+s", "alt+j", "alt+1"] {
            assert_eq!(g.classify(&key(c), Some("Nachricht")), Risk::Commit, "{c}");
        }
        // Moving focus away from a send button is not a commit.
        assert_eq!(g.classify(&key("tab"), Some("Absenden")), Risk::Act);
        assert_eq!(g.classify(&key("alt+left"), None), Risk::Act);
        assert_eq!(g.classify(&key("backspace"), None), Risk::Act);
    }

    #[test]
    fn final_labels_count_on_ui_names_and_quotes_only() {
        let g = Gate::new(false);
        for label in
            ["Antrag einreichen", "Jetzt beantragen", "Book now", "Weiterleiten", "Entfernen", "Sign", "Ja", "OK"]
        {
            assert_eq!(
                g.classify(&click("den blauen Knopf", "blue button", false), Some(label)),
                Risk::Commit,
                "{label}"
            );
            assert_eq!(
                g.classify(&click(&format!("den Knopf „{label}“"), "button", false), None),
                Risk::Commit,
                "{label}"
            );
        }
        // Free text that only mentions such a word, logins and plain fields stay Act.
        assert_eq!(
            g.classify(&click("Formular Unterlagen einreichen, Feld Name", "text input field", false), None),
            Risk::Act
        );
        for name in ["Sign in", "Postleitzahl", "Weiter", "Vor- und Nachname", "Mitteilung"] {
            assert_eq!(g.classify(&click("das Feld", "text input field", false), Some(name)), Risk::Act, "{name}");
        }
        assert_eq!(quoted_label("den Knopf „Datei auswählen“ unten"), Some("Datei auswählen".into()));
        assert_eq!(quoted_label("the \"Send\" button"), Some("Send".into()));
        assert_eq!(quoted_label("das Feld Name"), None);
    }

    #[test]
    fn free_and_act() {
        let g = Gate::new(false);
        assert_eq!(g.classify(&Tool::Say { text: "senden".into() }, None), Risk::Free);
        assert_eq!(
            g.classify(&Tool::Point { sam_query: "submit button".into(), description: "Absenden".into() }, None),
            Risk::Free
        );
        let t = Tool::TypeText {
            text: "x".into(),
            what: "w".into(),
            into_sam_query: None,
            field_label: None,
            element: None,
        };
        assert_eq!(g.classify(&t, None), Risk::Act);
        assert_eq!(g.classify(&Tool::OpenUrl { url: "https://x".into() }, None), Risk::Act);
    }

    #[test]
    fn urls() {
        let live = Gate::new(false);
        assert!(live.url_allowed("https://www.aok.de/kontakt"));
        assert!(live.url_allowed("http://localhost:8000/"));
        assert!(!live.url_allowed("file:///C:/demo/fixtures/demo-insurer/index.html"));
        assert!(!live.url_allowed("javascript:alert(1)"));
        assert!(!live.url_allowed("ftp://x"));
        assert!(!live.url_allowed("https://localhost@evil.example/"));

        let demo = Gate::new(true).with_fixtures(&["/home/x/papierkram-copilot/fixtures", "C:/Pip/fixtures"]);
        assert!(demo.url_allowed("http://localhost:8000/form"));
        assert!(demo.url_allowed("http://127.0.0.1/"));
        assert!(demo.url_allowed("http://[::1]:9000/"));
        assert!(demo.url_allowed("file:///home/x/papierkram-copilot/fixtures/demo-insurer/index.html"));
        assert!(demo.url_allowed("file:///C:/Pip/fixtures/demo-insurer/index.html"));
        assert!(demo.url_allowed("file://localhost/C:/Pip/fixtures/demo-insurer/index.html"));
        assert!(!demo.url_allowed("https://www.aok.de/"));
        assert!(!demo.url_allowed("http://localhost.evil.example/"));
        assert!(!demo.url_allowed("http://localhost@evil.example/"));
        assert!(!demo.url_allowed("file:///C:/Users/x/Documents/secret.pdf"));
        assert!(!demo.url_allowed("file:///home/x/papierkram-copilot/fixtures/../../etc/passwd"));
        assert!(!demo.url_allowed("file://evil.example/share/fixtures/x.html"), "UNC path");
        assert!(!demo.url_allowed("file:////evil.example/share/fixtures/x.html"), "UNC via four slashes");
        assert!(!demo.url_allowed("file:///C:/Users/erika/Downloads/fixtures/evil.html"), "any folder named fixtures");
        assert!(!demo.url_allowed("file:///C:/Pip/fixtures-evil/x.html"), "prefix of another folder");
        assert!(!Gate::new(true).url_allowed("file:///C:/Pip/fixtures/demo-insurer/index.html"), "no folder given");
    }

    #[test]
    fn hosts_and_address_bar() {
        assert_eq!(url_host("https://Upload.Evil.example:8443/x?y").as_deref(), Some("upload.evil.example"));
        assert_eq!(url_host("http://localhost:8000/"), Some("localhost".into()));
        assert_eq!(url_host("file:///C:/x"), None);
        assert_eq!(normalize_address("localhost:8000/form").as_deref(), Some("http://localhost:8000/form"));
        assert_eq!(normalize_address("www.aok.de/kontakt").as_deref(), Some("https://www.aok.de/kontakt"));
        assert_eq!(normalize_address("C:\\Pip\\fixtures\\a.html").as_deref(), Some("file:///C:/Pip/fixtures/a.html"));
        assert_eq!(normalize_address("file:///C:/a.html").as_deref(), Some("file:///C:/a.html"));
        assert_eq!(normalize_address("edge://settings"), None);
        assert_eq!(normalize_address("Suchen oder Adresse eingeben"), None);
        assert_eq!(normalize_address(""), None);
    }

    #[test]
    fn attach_like() {
        assert!(is_attach_like("Datei auswählen"));
        assert!(is_attach_like("Krankschreibung anhängen"));
        assert!(!is_attach_like("Absenden"));
        assert!(is_attach_target("Knopf „Datei auswählen“ button"));
        assert!(!is_attach_target("den grünen Knopf „Absenden“ green button"));
        assert!(!is_attach_target("Datei hochladen und absenden"));
        assert!(is_send_target("Absenden"));
        assert!(!is_send_target("Datei auswählen"));
        assert!(is_delete_target("Konto löschen"));
        assert!(!is_delete_target("Absenden"));
        for m in ["Jetzt bezahlen", "Zahlungspflichtig bestellen", "Buy now", "Place order", "Book now", "Checkout"] {
            assert!(is_money_target(m), "{m}");
        }
        assert!(!is_money_target("Absenden") && !is_money_target("Send message"));
        assert!(is_cancel_target("Vertrag kündigen") && is_cancel_target("Cancel subscription"));
        assert!(!is_cancel_target("Absenden"));
    }

    #[test]
    fn site_keys() {
        let live = Gate::new(false);
        assert_eq!(live.site_key("https://WWW.Insurer.example:8443/form?x").as_deref(), Some("www.insurer.example"));
        assert_eq!(
            live.site_key("file:///C:/Pip/fixtures/demo-insurer/index.html"),
            None,
            "no local pages outside demo"
        );
        let demo = Gate::new(true).with_fixtures(&["C:/Pip/fixtures"]);
        // One key per demo site folder: the insurer's three pages share one, the university has its own.
        let insurer = Some("file:///c:/pip/fixtures/demo-insurer");
        for page in ["index.html", "services.html", "questionnaire.html?x=1#top"] {
            assert_eq!(
                demo.site_key(&format!("file:///C:/Pip/fixtures/demo-insurer/{page}")).as_deref(),
                insurer,
                "{page}"
            );
        }
        assert_eq!(demo.site_key("file://localhost/C:/Pip/fixtures/demo-insurer/index.html").as_deref(), insurer);
        assert_eq!(demo.site_key("file:///C:\\Pip\\fixtures\\demo-insurer\\services.html").as_deref(), insurer);
        let uni = demo.site_key("file:///C:/Pip/fixtures/demo-uni/index.html");
        assert_eq!(uni.as_deref(), Some("file:///c:/pip/fixtures/demo-uni"));
        assert_ne!(uni.as_deref(), insurer);
        // A page directly in the root keeps the root as its key.
        assert_eq!(demo.site_key("file:///C:/Pip/fixtures/page.html").as_deref(), Some("file:///c:/pip/fixtures"));
        assert_eq!(demo.site_key("file:///C:/Pip/fixtures/page.html?a/b").as_deref(), Some("file:///c:/pip/fixtures"));
        assert_eq!(demo.site_key("file:///C:/Users/x/evil.html"), None);
        assert_eq!(demo.site_key("file:///C:/Pip/fixtures-evil/demo-uni/index.html"), None);
        assert_eq!(demo.site_key("file:///C:/Pip/fixtures/../demo-uni/index.html"), None);
        assert_eq!(demo.site_key("http://localhost:8000/"), Some("localhost".into()));
    }

    #[test]
    fn site_labels_for_sentences() {
        let live = Gate::new(false);
        assert_eq!(live.site_label("https://WWW.Insurer.example/form?x").as_deref(), Some("www.insurer.example"));
        let demo = Gate::new(true).with_fixtures(&["C:/Pip/fixtures"]);
        assert_eq!(demo.site_label("file:///C:/Pip/fixtures/demo-insurer/index.html").as_deref(), Some("demo-insurer"));
        assert_eq!(demo.site_label("file:///C:/Pip/fixtures/page.html").as_deref(), Some("fixtures"));
        assert_eq!(demo.site_label("file:///C:/Users/x/evil.html"), None);
    }
}

#[cfg(test)]
mod close_tests {
    use super::Gate;

    #[test]
    fn window_close_is_refused_popup_close_is_not() {
        assert!(Gate::closes_window(
            "das Schließen-Kreuz oben rechts im PowerShell-Fenster",
            "close button X",
            "Schließen"
        ));
        assert!(Gate::closes_window("das X zum Schließen oben rechts am schwarzen Fenster", "close button X", ""));
        assert!(Gate::closes_window("Fenster minimieren", "minimize button", ""));
        assert!(!Gate::closes_window("das X zum Schließen vom Hinweis oben rechts", "close button X", "Schließen"));
        assert!(!Gate::closes_window("das Feld Vor- und Nachname", "name text field", "Vor- und Nachname"));
        assert!(!Gate::closes_window("den Knopf Absenden", "green submit button", "Absenden"));
    }
}
