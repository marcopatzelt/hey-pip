//! Tools Muse Spark may call. OWNER: agent "agent".

use anyhow::{Context, anyhow, ensure};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::meta::{FunctionCall, ToolDef};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollDir {
    Up,
    Down,
}

/// Parsed from FunctionCall {name, arguments}. Bounds are checked in `from_call`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "name", content = "arguments", rename_all = "snake_case")]
pub enum Tool {
    Say {
        text: String,
    },
    Point {
        sam_query: String,
        description: String,
    },
    /// `commit_what` / `commit_to` fill the approval card ("deine Krankschreibung", "Gesundkasse").
    /// Missing is_commit/commit_* parse as false/null: the live model leaves them out (26.10 Edge run).
    Click {
        sam_query: String,
        description: String,
        #[serde(default)]
        is_commit: bool,
        #[serde(default)]
        commit_what: Option<String>,
        #[serde(default)]
        commit_to: Option<String>,
        /// Number (#n) from PAGE ELEMENTS: act on that element without finding it in the picture.
        #[serde(default)]
        element: Option<u32>,
    },
    /// into_sam_query null = type where the focus is. `what` feeds "Ich trage {what} ein."
    TypeText {
        text: String,
        /// Missing = the field_label or "the text" (see `lenient`).
        #[serde(default)]
        what: String,
        #[serde(default)]
        into_sam_query: Option<String>,
        /// The field's visible label ("Versichertennummer"): found in the UI tree at once, no model call.
        #[serde(default)]
        field_label: Option<String>,
        /// Number (#n) from PAGE ELEMENTS: click that field, select its text and type.
        #[serde(default)]
        element: Option<u32>,
    },
    Key {
        combo: String,
        #[serde(default)]
        is_commit: bool,
        #[serde(default)]
        commit_what: Option<String>,
        #[serde(default)]
        commit_to: Option<String>,
    },
    Scroll {
        direction: ScrollDir,
        amount: u8,
    },
    OpenUrl {
        url: String,
    },
    Wait {
        seconds: u8,
    },
    RememberFact {
        key: String,
        value: String,
    },
    SearchMemory {
        query: String,
    },
    AddDeadline {
        title: String,
        due: NaiveDate,
        why: String,
    },
    AskUser {
        question: String,
        /// Missing, null or "" = open question (the model often sends `"options": ""`; see `lenient`).
        #[serde(default)]
        options: Vec<String>,
    },
    Done {
        summary: String,
    },
    /// After-task discovery: one offer from the page that is open, tied to a letter or an open deadline.
    /// No URL and no free text: the app writes the sentence for the user itself.
    Suggest {
        page_quote: String,
        #[serde(default)]
        reason_doc_id: Option<i64>,
        #[serde(default)]
        reason_deadline_id: Option<i64>,
    },
}

/// Tools the model gets while Pip only looks around after a task (no click, no typing, no sending).
pub const DISCOVER_TOOLS: &[&str] = &["open_url", "wait", "scroll", "say", "suggest", "done"];

/// Key names `key` accepts (plus single letters/digits and f1..f12). No Esc: while Pip acts, Esc is the
/// user's global stop key, and an injected Esc would stop Pip's own run (click "Abbrechen" instead).
const KEY_NAMES: &[&str] = &[
    "enter",
    "return",
    "tab",
    "backspace",
    "delete",
    "space",
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "pageup",
    "pagedown",
];
const MODIFIERS: &[&str] = &["ctrl", "alt", "shift"];

fn check_len(field: &str, s: &str, min: usize, max: usize) -> anyhow::Result<()> {
    let n = s.trim().chars().count();
    ensure!(n >= min, "{field} must not be empty");
    ensure!(n <= max, "{field} is too long ({n} chars, max {max})");
    Ok(())
}

fn check_opt(field: &str, s: &Option<String>, max: usize) -> anyhow::Result<()> {
    match s {
        Some(s) => check_len(field, s, 0, max),
        None => Ok(()),
    }
}

/// "Ctrl + A" -> "ctrl+a". Errors on unknown keys and on the Windows key (could start anything).
pub(crate) fn normalize_combo(combo: &str) -> anyhow::Result<String> {
    let parts: Vec<String> = combo.split('+').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()).collect();
    ensure!(!parts.is_empty(), "combo must not be empty");
    let (last, mods) = parts.split_last().expect("non-empty");
    for m in mods {
        ensure!(MODIFIERS.contains(&m.as_str()), "unknown modifier '{m}' (use ctrl, alt, shift)");
    }
    let is_fkey = last.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=12).contains(&n));
    let is_char = last.chars().count() == 1 && last.chars().all(|c| c.is_ascii_alphanumeric());
    ensure!(
        !matches!(last.as_str(), "esc" | "escape"),
        "esc is not available (it is the user's stop key); click the dialog's cancel button instead"
    );
    ensure!(
        KEY_NAMES.contains(&last.as_str()) || is_fkey || is_char,
        "unknown key '{last}' (use enter, tab, backspace, arrows, f1-f12 or one letter)"
    );
    // Closing keys end the user's windows or tabs (unsaved work).
    let joined = parts.join("+");
    ensure!(
        !matches!(joined.as_str(), "alt+f4" | "ctrl+w" | "ctrl+f4" | "ctrl+q" | "ctrl+shift+w" | "ctrl+shift+q"),
        "keys that close windows or tabs are not available; Pip never closes the user's windows"
    );
    // Alt + letter/digit triggers access keys (a form's submit button, Outlook "Senden", a dialog's "&Ja").
    ensure!(
        !(mods.iter().any(|m| m == "alt") && is_char),
        "alt with a letter or digit can send or delete things; click the button instead"
    );
    Ok(parts.join("+"))
}

impl Tool {
    /// FunctionCall -> Tool (+ bounds check). Errors become a function_call_output telling the model what was wrong.
    pub fn from_call(call: &FunctionCall) -> anyhow::Result<Self> {
        let args = match &call.arguments {
            Value::String(s) => serde_json::from_str(s).context("arguments are not valid JSON")?,
            v => v.clone(),
        };
        let args = lenient(&call.name, args);
        let tool: Tool = serde_json::from_value(json!({"name": call.name, "arguments": args}))
            .map_err(|e| anyhow!("invalid call of '{}': {e}", call.name))?;
        tool.validate()?;
        Ok(tool)
    }

    fn validate(&self) -> anyhow::Result<()> {
        match self {
            Tool::Say { text } => check_len("text", text, 1, 400),
            Tool::Point { sam_query, description } => {
                check_len("sam_query", sam_query, 1, 80)?;
                check_len("description", description, 1, 200)
            }
            Tool::Click { sam_query, description, commit_what, commit_to, .. } => {
                check_len("sam_query", sam_query, 1, 80)?;
                check_len("description", description, 1, 200)?;
                check_opt("commit_what", commit_what, 200)?;
                check_opt("commit_to", commit_to, 200)
            }
            Tool::TypeText { text, what, into_sam_query, field_label, .. } => {
                check_opt("field_label", field_label, 80)?;
                ensure!(!text.is_empty(), "text must not be empty");
                ensure!(text.chars().count() <= 2000, "text is too long (max 2000 chars)");
                // A typed line break or tab would act like Enter/Tab and could send a form past the gate.
                ensure!(
                    !text.chars().any(char::is_control),
                    "text must be one line without tabs: type one line at a time and press enter or tab with the \
                     key tool"
                );
                check_len("what", what, 1, 100)?;
                check_opt("into_sam_query", into_sam_query, 80)
            }
            Tool::Key { combo, commit_what, commit_to, .. } => {
                normalize_combo(combo)?;
                check_opt("commit_what", commit_what, 200)?;
                check_opt("commit_to", commit_to, 200)
            }
            Tool::Scroll { amount, .. } => {
                ensure!((1..=10).contains(amount), "amount must be 1..10");
                Ok(())
            }
            Tool::OpenUrl { url } => check_len("url", url, 1, 2000),
            Tool::Wait { seconds } => {
                ensure!(*seconds <= 10, "seconds must be 0..10");
                Ok(())
            }
            Tool::RememberFact { key, value } => {
                check_len("key", key, 1, 40)?;
                ensure!(
                    key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                    "key must be lowercase snake_case (a-z, 0-9, _)"
                );
                check_len("value", value, 1, 300)
            }
            Tool::SearchMemory { query } => check_len("query", query, 1, 200),
            Tool::AddDeadline { title, why, .. } => {
                check_len("title", title, 1, 120)?;
                check_len("why", why, 0, 400)
            }
            Tool::AskUser { question, options } => {
                check_len("question", question, 1, 300)?;
                ensure!(options.len() <= 4, "at most 4 options");
                for o in options {
                    check_len("option", o, 1, 40)?;
                }
                Ok(())
            }
            Tool::Done { summary } => check_len("summary", summary, 1, 300),
            Tool::Suggest { page_quote, reason_doc_id, reason_deadline_id } => {
                check_len("page_quote", page_quote, 3, 120)?;
                let low = page_quote.to_lowercase();
                ensure!(
                    !low.contains('@') && !low.contains("://") && !low.contains("www."),
                    "page_quote must be a heading from the page, never an address"
                );
                ensure!(
                    reason_doc_id.is_some() || reason_deadline_id.is_some(),
                    "give reason_doc_id or reason_deadline_id (an id from WHAT YOU KNOW)"
                );
                Ok(())
            }
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Tool::Say { .. } => "say",
            Tool::Point { .. } => "point",
            Tool::Click { .. } => "click",
            Tool::TypeText { .. } => "type_text",
            Tool::Key { .. } => "key",
            Tool::Scroll { .. } => "scroll",
            Tool::OpenUrl { .. } => "open_url",
            Tool::Wait { .. } => "wait",
            Tool::RememberFact { .. } => "remember_fact",
            Tool::SearchMemory { .. } => "search_memory",
            Tool::AddDeadline { .. } => "add_deadline",
            Tool::AskUser { .. } => "ask_user",
            Tool::Done { .. } => "done",
            Tool::Suggest { .. } => "suggest",
        }
    }

    /// Arguments for actions_log: every string truncated to 80 chars.
    pub(crate) fn redacted_args(&self) -> String {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        let args = v.get_mut("arguments").map(Value::take).unwrap_or(Value::Null);
        let args = truncate_strings(args, 80);
        serde_json::to_string(&args).unwrap_or_default()
    }
}

/// Live Muse Spark sends nullable fields as the string "null", sometimes numbers/bools as strings, and
/// sometimes leaves fields out. Value checks stay in `validate`; this only fills harmless gaps.
fn lenient(name: &str, mut args: Value) -> Value {
    if let Some(o) = args.as_object_mut() {
        for (k, v) in o.iter_mut() {
            if k == "options" && v.is_null() {
                *v = json!([]);
                continue;
            }
            let Value::String(s) = v else { continue };
            let t = s.trim();
            match k.as_str() {
                // An open question: the model sends "" or "null" instead of [] (seen in most dry runs).
                "options" if t.is_empty() || t.eq_ignore_ascii_case("null") => *v = json!([]),
                "into_sam_query" | "commit_what" | "commit_to" | "reason_doc_id" | "reason_deadline_id" | "element"
                | "field_label"
                    if t.is_empty() || t.eq_ignore_ascii_case("null") =>
                {
                    *v = Value::Null
                }
                // is_commit "" or "null": the model meant "not set", the gate still reads the button's label.
                "is_commit" if t.is_empty() || t.eq_ignore_ascii_case("null") => *v = Value::Bool(false),
                "reason_doc_id" | "reason_deadline_id" | "element" => {
                    if let Ok(n) = t.trim_start_matches('#').parse::<i64>() {
                        *v = Value::from(n)
                    }
                }
                "is_commit" if t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("false") => {
                    *v = Value::Bool(t.eq_ignore_ascii_case("true"))
                }
                "amount" | "seconds" => {
                    if let Ok(n) = t.parse::<u64>() {
                        *v = Value::from(n)
                    }
                }
                _ => {}
            }
        }
        fill_gaps(name, o);
    }
    args
}

/// Missing or empty string (also "null").
fn blank(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.trim().is_empty() || s.trim().eq_ignore_ascii_case("null"),
        Some(_) => false,
    }
}

/// Defaults for fields the model left out. Only where the meaning is clear: an element number names the
/// target, so its description and shape can be filled; `what` is only the wording of "I fill in {what}".
fn fill_gaps(name: &str, o: &mut serde_json::Map<String, Value>) {
    let element = o.get("element").and_then(Value::as_u64);
    if name == "type_text" {
        if blank(o.get("what")) {
            let what = match o.get("field_label") {
                Some(Value::String(l)) if !blank(o.get("field_label")) => l.trim().to_string(),
                _ => "the text".to_string(),
            };
            o.insert("what".into(), Value::String(what));
        }
        // The model sometimes writes sam_query (click's name) instead of into_sam_query.
        if blank(o.get("into_sam_query")) && !blank(o.get("sam_query")) {
            let q = o.remove("sam_query").unwrap_or(Value::Null);
            o.insert("into_sam_query".into(), q);
        }
        // An element without into_sam_query needs nothing: the loop falls back to 'text input field' itself.
    }
    if name == "click" {
        if let Some(n) = element
            && blank(o.get("description"))
        {
            o.insert("description".into(), Value::String(format!("element #{n}")));
        }
        // A shape is only a hint next to a named target; with neither, the call stays an error.
        if blank(o.get("sam_query")) && !blank(o.get("description")) {
            o.insert("sam_query".into(), Value::String("button".into()));
        }
    }
}

fn truncate_strings(v: Value, max: usize) -> Value {
    match v {
        Value::String(s) => Value::String(truncate(&s, max)),
        Value::Array(a) => Value::Array(a.into_iter().map(|x| truncate_strings(x, max)).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, x)| (k, truncate_strings(x, max))).collect()),
        other => other,
    }
}

/// Char-safe truncation with an ellipsis.
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

fn str_(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}
fn opt_str(desc: &str) -> Value {
    json!({"type": ["string", "null"], "description": desc})
}
fn bool_(desc: &str) -> Value {
    json!({"type": "boolean", "description": desc})
}
fn int_(desc: &str) -> Value {
    json!({"type": "integer", "description": desc})
}
fn opt_int(desc: &str) -> Value {
    json!({"type": ["integer", "null"], "description": desc})
}

/// Object schema: strict, additionalProperties false, every property required.
fn obj(props: &[(&str, Value)]) -> Value {
    let mut properties = serde_json::Map::new();
    for (k, v) in props {
        properties.insert((*k).to_string(), v.clone());
    }
    let required: Vec<&str> = props.iter().map(|(k, _)| *k).collect();
    json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
}

fn def(name: &str, description: &str, parameters: Value) -> ToolDef {
    ToolDef { name: name.into(), description: description.into(), parameters }
}

const SAM_QUERY: &str = "Short English noun phrase describing the SHAPE of the target, for an image segmenter that \
cannot read text: e.g. 'blue button', 'text input field', 'checkbox', 'link'. Never put the label text here.";
const DESCRIPTION: &str = "The target in the user's language, with its visible label and position, e.g. \
'den Knopf „Absenden“ unten rechts'. Shown to the user.";
const IS_COMMIT: &str = "true if this sends, submits, uploads or attaches a private file, pays, buys, orders, \
deletes or cancels something. The user must approve it first.";
const COMMIT_WHAT: &str = "Only when is_commit: what is sent, in the user's language (e.g. 'deine Krankschreibung \
20.10.–30.10.'). Otherwise null.";
const COMMIT_TO: &str = "Only when is_commit: to whom (e.g. 'Gesundkasse (Kontaktformular)'). Otherwise null.";
const ELEMENT: &str = "The # number of the target in PAGE ELEMENTS (e.g. 4 for '#4'), when the list has it; then Pip \
acts on that element directly. null when there is no PAGE ELEMENTS list or the target is not in it.";

/// JSON schemas: type function, strict, additionalProperties:false, all fields required
/// (optional ones as ["string","null"]).
pub fn tool_defs() -> Vec<ToolDef> {
    vec![
        def(
            "say",
            "Tell the user something short (max 2 plain sentences, user's language). Use it to explain what you do and why.",
            obj(&[("text", str_("What to say."))]),
        ),
        def(
            "point",
            "Show the user where something is on the screen, without clicking.",
            obj(&[("sam_query", str_(SAM_QUERY)), ("description", str_(DESCRIPTION))]),
        ),
        def(
            "click",
            "Click one element on the current screen.",
            obj(&[
                ("sam_query", str_(SAM_QUERY)),
                ("description", str_(DESCRIPTION)),
                ("is_commit", bool_(IS_COMMIT)),
                ("commit_what", opt_str(COMMIT_WHAT)),
                ("commit_to", opt_str(COMMIT_TO)),
                ("element", opt_int(ELEMENT)),
            ]),
        ),
        def(
            "type_text",
            "Type text. If element is set, Pip clicks that field, replaces its text and reports what it shows. \
             Else if into_sam_query is set, first click that field; if both are null, type where the cursor is. \
             Never type passwords.",
            obj(&[
                ("text", str_("The exact text to type.")),
                ("what", str_("What you fill in, user's language, as object of a sentence, e.g. 'deinen Namen'.")),
                (
                    "into_sam_query",
                    opt_str("Shape phrase of the field to click first (e.g. 'text input field'), or null."),
                ),
                (
                    "field_label",
                    opt_str(
                        "The label printed next to or inside the field, exactly as on screen (e.g. 'Versichertennummer'), or null.",
                    ),
                ),
                ("element", opt_int(ELEMENT)),
            ]),
        ),
        def(
            "key",
            "Press a key or key combination: 'enter', 'tab', 'shift+tab', 'up', 'alt+left'. \
             There is no esc: to close a dialog, click its cancel button.",
            obj(&[
                ("combo", str_("Key combination, keys joined with '+'.")),
                ("is_commit", bool_(IS_COMMIT)),
                ("commit_what", opt_str(COMMIT_WHAT)),
                ("commit_to", opt_str(COMMIT_TO)),
            ]),
        ),
        def(
            "scroll",
            "Scroll the window under the middle of the screen.",
            obj(&[
                ("direction", json!({"type": "string", "enum": ["up", "down"], "description": "Direction."})),
                ("amount", int_("How far, 1 (a little) to 10 (a lot).")),
            ]),
        ),
        def(
            "open_url",
            "Open a web page or local page in the browser.",
            obj(&[("url", str_("Full URL (https://… or file:///…)."))]),
        ),
        def("wait", "Wait for a page or dialog to load.", obj(&[("seconds", int_("0 to 10."))])),
        def(
            "remember_fact",
            "Remember a fact about the user for later (e.g. key 'krankenkasse', value 'Gesundkasse').",
            obj(&[("key", str_("lowercase snake_case key")), ("value", str_("The value."))]),
        ),
        def(
            "search_memory",
            "Search the user's saved letters. Returns id, type, date, sender, file path and a summary.",
            obj(&[("query", str_("Search words, e.g. 'Krankschreibung'."))]),
        ),
        def(
            "add_deadline",
            "Save a deadline the user must keep. Pip will remind the user.",
            obj(&[
                ("title", str_("Short title in the user's language.")),
                ("due", str_("Due date, YYYY-MM-DD.")),
                ("why", str_("One plain sentence why it matters.")),
            ]),
        ),
        def(
            "ask_user",
            "Ask the user and wait for the answer. Use it for choices, logins, captchas, codes and passwords.",
            obj(&[
                ("question", str_("Short question in the user's language.")),
                (
                    "options",
                    json!({"type": "array", "items": {"type": "string"}, "description": "0 to 4 short answer buttons. Empty = open question: the user types the answer, which you get back as text."}),
                ),
            ]),
        ),
        def(
            "done",
            "Finish the task. summary = the one fact you will remember, user's language \
             (e.g. 'Krankschreibung ist bei der Kasse.').",
            obj(&[("summary", str_("One short sentence."))]),
        ),
        def(
            "suggest",
            "Offer the user ONE thing from the page that is open now. The app writes the question and asks the \
             user. Only for a heading you can see on the page right now that fits a letter or an open deadline.",
            obj(&[
                ("page_quote", str_("The heading exactly as it is written on the page (3 to 120 characters).")),
                ("reason_doc_id", opt_int("Id (#n) of the letter in WHAT YOU KNOW that it fits, or null.")),
                ("reason_deadline_id", opt_int("Id (#n) of the open deadline in WHAT YOU KNOW that it fits, or null.")),
            ]),
        ),
    ]
}

/// Error text for the model, in one place so it stays consistent.
pub(crate) fn bad_call(e: &anyhow::Error) -> String {
    format!("error: {e:#}. Fix the arguments and call again.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: Value) -> FunctionCall {
        FunctionCall { call_id: "c1".into(), name: name.into(), arguments: args }
    }

    #[test]
    fn parses_every_tool() {
        let cases = [
            ("say", json!({"text": "Hallo"})),
            ("point", json!({"sam_query": "blue button", "description": "Absenden"})),
            (
                "click",
                json!({"sam_query": "blue button", "description": "Absenden", "is_commit": true,
                       "commit_what": "Krankschreibung", "commit_to": null}),
            ),
            ("type_text", json!({"text": "Erika", "what": "deinen Namen", "into_sam_query": null})),
            ("key", json!({"combo": "Ctrl + A", "is_commit": false, "commit_what": null, "commit_to": null})),
            ("scroll", json!({"direction": "down", "amount": 3})),
            ("open_url", json!({"url": "http://localhost:8000/"})),
            ("wait", json!({"seconds": 2})),
            ("remember_fact", json!({"key": "krankenkasse", "value": "Gesundkasse"})),
            ("search_memory", json!({"query": "Krankschreibung"})),
            ("add_deadline", json!({"title": "Widerspruch", "due": "2026-11-30", "why": "Sonst gilt der Bescheid."})),
            ("ask_user", json!({"question": "Welche?", "options": ["A", "B"]})),
            ("done", json!({"summary": "Erledigt."})),
            (
                "suggest",
                json!({"page_quote": "Return your questionnaire online", "reason_doc_id": 3, "reason_deadline_id": null}),
            ),
        ];
        for (name, args) in cases {
            let t = Tool::from_call(&call(name, args)).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(t.name(), name);
        }
    }

    #[test]
    fn element_numbers_parse_leniently() {
        let t = Tool::from_call(&call(
            "type_text",
            json!({"text": "Erika", "what": "your name", "into_sam_query": null, "field_label": null, "element": "#3"}),
        ))
        .unwrap();
        assert!(matches!(t, Tool::TypeText { element: Some(3), .. }));
        let t = Tool::from_call(&call(
            "click",
            json!({"sam_query": "button", "description": "Submit", "is_commit": true, "commit_what": null,
                   "commit_to": null, "element": "null"}),
        ))
        .unwrap();
        assert!(matches!(t, Tool::Click { element: None, .. }));
        let t = Tool::from_call(&call(
            "click",
            json!({"sam_query": "button", "description": "Submit", "is_commit": true, "commit_what": null,
                   "commit_to": null, "element": 7}),
        ))
        .unwrap();
        assert!(matches!(t, Tool::Click { element: Some(7), .. }));
        let defs = tool_defs();
        for name in ["click", "type_text"] {
            let d = defs.iter().find(|d| d.name == name).unwrap();
            assert_eq!(d.parameters["properties"]["element"]["type"], json!(["integer", "null"]), "{name}");
        }
    }

    #[test]
    fn missing_is_commit_defaults_to_false() {
        let t = Tool::from_call(&call("click", json!({"element": 7, "sam_query": "button", "description": "Submit"})))
            .unwrap();
        assert!(matches!(
            t,
            Tool::Click { is_commit: false, commit_what: None, commit_to: None, element: Some(7), .. }
        ));
        let t = Tool::from_call(&call("key", json!({"combo": "tab"}))).unwrap();
        assert_eq!(t, Tool::Key { combo: "tab".into(), is_commit: false, commit_what: None, commit_to: None });
        // Also as a JSON string, the way the API sends it.
        let t = Tool::from_call(&call("key", Value::String(r#"{"combo":"enter"}"#.into()))).unwrap();
        assert!(matches!(t, Tool::Key { is_commit: false, .. }));
        // "null" / "" for is_commit and field_label: not set, like the other nullable fields.
        let t = Tool::from_call(&call("key", json!({"combo": "tab", "is_commit": "null"}))).unwrap();
        assert!(matches!(t, Tool::Key { is_commit: false, .. }));
        let t = Tool::from_call(&call("click", json!({"element": 7, "description": "Next", "is_commit": ""}))).unwrap();
        assert!(matches!(t, Tool::Click { is_commit: false, .. }));
        let t = Tool::from_call(&call("type_text", json!({"text": "x", "element": 3, "field_label": "null"}))).unwrap();
        assert!(matches!(&t, Tool::TypeText { field_label: None, what, .. } if what == "the text"), "{t:?}");
    }

    #[test]
    fn type_text_without_what_or_field_label_parses() {
        let t = Tool::from_call(&call("type_text", json!({"text": "Erika Beispiel", "element": 3}))).unwrap();
        assert!(
            matches!(&t, Tool::TypeText { what, element: Some(3), into_sam_query: None, field_label: None, .. }
                if what == "the text"),
            "{t:?}"
        );
        let t = Tool::from_call(&call(
            "type_text",
            json!({"text": "G123", "what": "null", "field_label": "Insurance number"}),
        ))
        .unwrap();
        assert!(matches!(&t, Tool::TypeText { what, .. } if what == "Insurance number"), "{t:?}");
        let t = Tool::from_call(&call("type_text", json!({"text": "x", "sam_query": "text input field"}))).unwrap();
        assert!(matches!(&t, Tool::TypeText { into_sam_query: Some(q), .. } if q == "text input field"), "{t:?}");
    }

    #[test]
    fn element_call_without_description_is_accepted() {
        let t = Tool::from_call(&call("click", json!({"element": "#3"}))).unwrap();
        assert!(
            matches!(&t, Tool::Click { description, sam_query, element: Some(3), is_commit: false, .. }
                if description == "element #3" && sam_query == "button"),
            "{t:?}"
        );
        let t = Tool::from_call(&call("click", json!({"element": 3, "description": ""}))).unwrap();
        assert!(matches!(&t, Tool::Click { description, .. } if description == "element #3"));
        // Without an element number nothing names the target: still an error.
        assert!(Tool::from_call(&call("click", json!({"is_commit": false}))).is_err());
        assert!(Tool::from_call(&call("click", json!({"sam_query": "button"}))).is_err());
    }

    #[test]
    fn values_are_still_validated() {
        assert!(Tool::from_call(&call("key", json!({"combo": "alt+s"}))).is_err(), "alt+letter");
        assert!(Tool::from_call(&call("key", json!({"combo": "ctrl+hyper"}))).is_err(), "bad combo");
        assert!(Tool::from_call(&call("key", json!({"combo": "alt+f4"}))).is_err(), "closing key");
        assert!(Tool::from_call(&call("key", json!({}))).is_err(), "no combo");
        assert!(Tool::from_call(&call("type_text", json!({"text": "", "element": 3}))).is_err(), "empty text");
        assert!(Tool::from_call(&call("type_text", json!({"element": 3}))).is_err(), "no text");
        assert!(Tool::from_call(&call("type_text", json!({"text": "a\nb", "element": 3}))).is_err(), "control char");
        assert!(Tool::from_call(&call("remember_fact", json!({"key": "Bad Key", "value": "v"}))).is_err());
        assert!(Tool::from_call(&call("suggest", json!({"page_quote": "Return your questionnaire"}))).is_err());
        assert!(
            Tool::from_call(&call("suggest", json!({"page_quote": "Return your questionnaire", "reason_doc_id": 2})))
                .is_ok()
        );
        let long = "x".repeat(201);
        assert!(Tool::from_call(&call("click", json!({"element": 3, "description": long}))).is_err(), "too long");
    }

    #[test]
    fn arguments_as_json_string_are_accepted() {
        let t = Tool::from_call(&call("say", Value::String(r#"{"text":"Hi"}"#.into()))).unwrap();
        assert_eq!(t, Tool::Say { text: "Hi".into() });
    }

    #[test]
    fn live_quirks_null_strings() {
        let t = Tool::from_call(&call(
            "click",
            json!({"sam_query": "button", "description": "Weiter", "is_commit": "false", "commit_what": "null", "commit_to": ""}),
        ))
        .unwrap();
        assert!(matches!(t, Tool::Click { is_commit: false, commit_what: None, commit_to: None, .. }));
        let t =
            Tool::from_call(&call("type_text", json!({"text": "Erika", "what": "Namen", "into_sam_query": "null"})))
                .unwrap();
        assert!(matches!(t, Tool::TypeText { into_sam_query: None, .. }));
        assert_eq!(Tool::from_call(&call("wait", json!({"seconds": "3"}))).unwrap(), Tool::Wait { seconds: 3 });
    }

    #[test]
    fn bounds_and_unknown_tools_rejected() {
        assert!(Tool::from_call(&call("wait", json!({"seconds": 11}))).is_err());
        assert!(Tool::from_call(&call("scroll", json!({"direction": "down", "amount": 0}))).is_err());
        assert!(Tool::from_call(&call("scroll", json!({"direction": "sideways", "amount": 1}))).is_err());
        assert!(Tool::from_call(&call("say", json!({"text": ""}))).is_err());
        assert!(Tool::from_call(&call("rm_rf", json!({}))).is_err());
        for bad in ["a\nb", "a\rb", "a\tb"] {
            let c = call("type_text", json!({"text": bad, "what": "x", "into_sam_query": null}));
            assert!(Tool::from_call(&c).is_err(), "{bad:?}");
        }
        let ok = call("type_text", json!({"text": "Erika Beispiel", "what": "x", "into_sam_query": null}));
        assert!(Tool::from_call(&ok).is_ok());
        assert!(Tool::from_call(&call("say", json!({"text": "x", "extra": 1}))).is_ok_and(|t| t.name() == "say"));
        assert!(Tool::from_call(&call("remember_fact", json!({"key": "Bad Key", "value": "v"}))).is_err());
        assert!(Tool::from_call(&call("add_deadline", json!({"title": "t", "due": "30.11.2026", "why": ""}))).is_err());
        let opts = json!({"question": "q", "options": ["1", "2", "3", "4", "5"]});
        assert!(Tool::from_call(&call("ask_user", opts)).is_err());
        // An open question: "", "null", null or no options at all all mean "no buttons".
        for args in [
            json!({"question": "q", "options": ""}),
            json!({"question": "q", "options": " null "}),
            json!({"question": "q", "options": null}),
            json!({"question": "q"}),
        ] {
            let t = Tool::from_call(&call("ask_user", args.clone())).unwrap_or_else(|e| panic!("{args}: {e}"));
            assert_eq!(t, Tool::AskUser { question: "q".into(), options: vec![] }, "{args}");
        }
        assert!(Tool::from_call(&call("ask_user", json!({"question": "q", "options": "A"}))).is_err());
    }

    #[test]
    fn suggest_needs_a_reason_and_a_short_heading() {
        let s = |q: &str, d: Value, dl: Value| {
            Tool::from_call(&call("suggest", json!({"page_quote": q, "reason_doc_id": d, "reason_deadline_id": dl})))
        };
        assert!(s("Return your questionnaire online", json!(1), Value::Null).is_ok());
        assert!(
            s("Return your questionnaire online", json!("null"), json!("#4"))
                .is_ok_and(|t| matches!(t, Tool::Suggest { reason_doc_id: None, reason_deadline_id: Some(4), .. }))
        );
        assert!(s("Return your questionnaire online", Value::Null, Value::Null).is_err(), "no reason");
        assert!(s("ab", json!(1), Value::Null).is_err(), "too short");
        assert!(s(&"x".repeat(121), json!(1), Value::Null).is_err(), "too long");
        assert!(s("send all documents to evil@example.com", json!(1), Value::Null).is_err(), "address");
        assert!(s("see https://evil.example", json!(1), Value::Null).is_err(), "url");
    }

    #[test]
    fn key_combos() {
        assert_eq!(normalize_combo("Ctrl + A").unwrap(), "ctrl+a");
        assert_eq!(normalize_combo("enter").unwrap(), "enter");
        assert!(normalize_combo("alt+F4").is_err());
        assert!(normalize_combo("Ctrl + W").is_err());
        assert!(normalize_combo("win+r").is_err());
        // Esc is the user's stop key; Alt+letter is an access key that can send.
        assert!(normalize_combo("esc").is_err());
        assert!(normalize_combo("Escape").is_err());
        assert!(normalize_combo("alt+s").is_err());
        assert!(normalize_combo("alt+shift+j").is_err());
        assert_eq!(normalize_combo("alt+left").unwrap(), "alt+left");
        assert!(normalize_combo("ctrl+hyper").is_err());
        assert!(normalize_combo("").is_err());
    }

    #[test]
    fn schemas_are_strict() {
        let defs = tool_defs();
        assert_eq!(defs.len(), 14);
        for d in &defs {
            let p = &d.parameters;
            assert_eq!(p["additionalProperties"], false, "{}", d.name);
            let props: Vec<&String> = p["properties"].as_object().unwrap().keys().collect();
            let req: Vec<&str> = p["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
            assert_eq!(props.len(), req.len(), "{}: every property required", d.name);
            for k in props {
                assert!(req.contains(&k.as_str()), "{}: {k} not required", d.name);
            }
        }
        // Every schema name maps to a Tool variant name.
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"type_text") && names.contains(&"ask_user"));
    }

    #[test]
    fn redaction_truncates_strings() {
        let t = Tool::TypeText {
            text: "x".repeat(200),
            what: "w".into(),
            into_sam_query: None,
            field_label: None,
            element: None,
        };
        let r = t.redacted_args();
        let v: Value = serde_json::from_str(&r).unwrap();
        assert_eq!(v["text"].as_str().unwrap().chars().count(), 81, "{r}"); // 80 + ellipsis
    }
}
