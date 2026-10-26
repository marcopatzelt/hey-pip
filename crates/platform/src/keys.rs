//! Key-combo parser shared by all OS backends: "ctrl+shift+a", "enter", "alt+f4".
//! Pure, so it is tested on Linux.

use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
    Win,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyName {
    Enter,
    Tab,
    Escape,
    Space,
    Backspace,
    Delete,
    Insert,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    /// F1..F24
    F(u8),
    /// A printable character (letters are lower-cased so ctrl+A == ctrl+a).
    Char(char),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combo {
    /// In press order; released in reverse.
    pub mods: Vec<Modifier>,
    /// None = modifiers only (e.g. "win").
    pub key: Option<KeyName>,
}

/// Parse "ctrl+a", "Shift + Tab", "enter", "alt+f4", "win". Separators: '+' (a lone "+" is the plus key).
pub fn parse_combo(combo: &str) -> Result<Combo> {
    let combo = combo.trim();
    if combo.is_empty() {
        bail!("empty key combo");
    }
    let parts: Vec<&str> = if combo == "+" {
        vec!["+"]
    } else if let Some(head) = combo.strip_suffix("++") {
        head.split('+').chain(std::iter::once("+")).collect()
    } else {
        combo.split('+').collect()
    };
    let mut mods = Vec::new();
    let mut key = None;
    for raw in parts {
        let p = raw.trim();
        let lower = p.to_lowercase();
        if let Some(m) = modifier(&lower) {
            if !mods.contains(&m) {
                mods.push(m);
            }
            continue;
        }
        if key.is_some() {
            bail!("key combo has more than one main key: {combo}");
        }
        key = Some(key_name(p, &lower).ok_or_else(|| anyhow::anyhow!("unknown key: {p}"))?);
    }
    if mods.is_empty() && key.is_none() {
        bail!("empty key combo");
    }
    Ok(Combo { mods, key })
}

/// Refuse control characters in text to type. Windows input turns "\n" into an Enter key press and
/// a typed "\r" into an Enter character; both can send a form without the approval gate seeing it.
/// Tabs would jump to the next field. The agent must press such keys with `key`, which the gate checks.
pub fn check_typeable(text: &str) -> Result<()> {
    if text.chars().any(char::is_control) {
        bail!(
            "the text contains a line break, tab or other control character; type one line at a time and \
             press enter or tab with the key tool (enter can send a form)"
        );
    }
    Ok(())
}

fn modifier(s: &str) -> Option<Modifier> {
    Some(match s {
        "ctrl" | "control" | "strg" => Modifier::Ctrl,
        "shift" | "umschalt" => Modifier::Shift,
        "alt" | "option" => Modifier::Alt,
        "win" | "windows" | "meta" | "super" | "cmd" => Modifier::Win,
        _ => return None,
    })
}

fn key_name(orig: &str, s: &str) -> Option<KeyName> {
    Some(match s {
        "enter" | "return" | "eingabe" => KeyName::Enter,
        "tab" | "tabulator" => KeyName::Tab,
        "esc" | "escape" => KeyName::Escape,
        "space" | "leertaste" => KeyName::Space,
        "backspace" | "back" => KeyName::Backspace,
        "delete" | "del" | "entf" => KeyName::Delete,
        "insert" | "ins" | "einfg" => KeyName::Insert,
        "up" | "arrowup" | "uparrow" => KeyName::Up,
        "down" | "arrowdown" | "downarrow" => KeyName::Down,
        "left" | "arrowleft" | "leftarrow" => KeyName::Left,
        "right" | "arrowright" | "rightarrow" => KeyName::Right,
        "home" | "pos1" => KeyName::Home,
        "end" | "ende" => KeyName::End,
        "pageup" | "pgup" | "bildauf" => KeyName::PageUp,
        "pagedown" | "pgdn" | "bildab" => KeyName::PageDown,
        "plus" => KeyName::Char('+'),
        _ => {
            if let Some(n) = s.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                return (1..=24).contains(&n).then_some(KeyName::F(n));
            }
            let mut chars = orig.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            KeyName::Char(c.to_lowercase().next().unwrap_or(c))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_combos() {
        assert_eq!(parse_combo("enter").unwrap(), Combo { mods: vec![], key: Some(KeyName::Enter) });
        assert_eq!(
            parse_combo("Ctrl+Shift+A").unwrap(),
            Combo { mods: vec![Modifier::Ctrl, Modifier::Shift], key: Some(KeyName::Char('a')) }
        );
        assert_eq!(parse_combo("alt + F4").unwrap(), Combo { mods: vec![Modifier::Alt], key: Some(KeyName::F(4)) });
        assert_eq!(parse_combo("win").unwrap(), Combo { mods: vec![Modifier::Win], key: None });
        assert_eq!(parse_combo("ctrl++").unwrap().key, Some(KeyName::Char('+')));
        assert_eq!(parse_combo("+").unwrap().key, Some(KeyName::Char('+')));
        assert_eq!(parse_combo("shift+tab").unwrap().key, Some(KeyName::Tab));
        assert_eq!(parse_combo("ü").unwrap().key, Some(KeyName::Char('ü')));
    }

    #[test]
    fn parses_every_key_the_agent_allows() {
        // Mirrors KEY_NAMES / MODIFIERS in core::agent::tools::normalize_combo.
        for k in [
            "enter",
            "return",
            "tab",
            "esc",
            "escape",
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
            "f1",
            "f12",
            "a",
            "z",
            "0",
            "9",
        ] {
            assert!(parse_combo(k).is_ok(), "{k}");
            assert!(parse_combo(&format!("ctrl+alt+shift+{k}")).is_ok(), "{k}");
        }
    }

    #[test]
    fn typeable_text() {
        assert!(check_typeable("Erika Musterfrau, Grüße 😀").is_ok());
        for bad in ["a\nb", "a\r", "a\tb", "\u{0}", "x\u{1b}"] {
            assert!(check_typeable(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_combo("").is_err());
        assert!(parse_combo("ctrl+foo").is_err());
        assert!(parse_combo("a+b").is_err());
        assert!(parse_combo("f99").is_err());
    }
}
