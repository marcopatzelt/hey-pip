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
    Click {
        sam_query: String,
        description: String,
        is_commit: bool,
        commit_what: Option<String>,
        commit_to: Option<String>,
    },
    /// into_sam_query null = type where the focus is. `what` feeds "Ich trage {what} ein."
    TypeText {
        text: String,
        what: String,
        into_sam_query: Option<String>,
    },
    Key {
        combo: String,
        is_commit: bool,
        commit_what: Option<String>,
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
        options: Vec<String>,
    },
    Done {
        summary: String,
    },
}

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
        let args = lenient(args);
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
            Tool::TypeText { text, what, into_sam_query } => {
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

/// Live Muse Spark sends nullable fields as the string "null" and sometimes numbers/bools as strings.
fn lenient(mut args: Value) -> Value {
    if let Some(o) = args.as_object_mut() {
        for (k, v) in o.iter_mut() {
            let Value::String(s) = v else { continue };
            let t = s.trim();
            match k.as_str() {
                "into_sam_query" | "commit_what" | "commit_to" if t.is_empty() || t.eq_ignore_ascii_case("null") => {
                    *v = Value::Null
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
    }
    args
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
            ]),
        ),
        def(
            "type_text",
            "Type text. If into_sam_query is set, first click that field; if null, type where the cursor is. \
             Never type passwords.",
            obj(&[
                ("text", str_("The exact text to type.")),
                ("what", str_("What you fill in, user's language, as object of a sentence, e.g. 'deinen Namen'.")),
                (
                    "into_sam_query",
                    opt_str("Shape phrase of the field to click first (e.g. 'text input field'), or null."),
                ),
            ]),
        ),
        def(
            "key",
            "Press a key or key combination: 'enter', 'tab', 'backspace', 'up', 'ctrl+a', 'alt+left'. \
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
                    json!({"type": "array", "items": {"type": "string"}, "description": "0 to 4 short answer buttons."}),
                ),
            ]),
        ),
        def(
            "done",
            "Finish the task. summary = the one fact you will remember, user's language \
             (e.g. 'Krankschreibung ist bei der Kasse.').",
            obj(&[("summary", str_("One short sentence."))]),
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
        ];
        for (name, args) in cases {
            let t = Tool::from_call(&call(name, args)).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(t.name(), name);
        }
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
    }

    #[test]
    fn key_combos() {
        assert_eq!(normalize_combo("Ctrl + A").unwrap(), "ctrl+a");
        assert_eq!(normalize_combo("enter").unwrap(), "enter");
        assert_eq!(normalize_combo("alt+F4").unwrap(), "alt+f4");
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
        assert_eq!(defs.len(), 13);
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
        let t = Tool::TypeText { text: "x".repeat(200), what: "w".into(), into_sam_query: None };
        let r = t.redacted_args();
        let v: Value = serde_json::from_str(&r).unwrap();
        assert_eq!(v["text"].as_str().unwrap().chars().count(), 81, "{r}"); // 80 + ellipsis
    }
}
