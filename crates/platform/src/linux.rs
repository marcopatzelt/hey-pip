//! Linux "in theory", best effort without extra crates:
//! - capture: `grim` (wlroots Wayland) or ImageMagick `import` (X11); GNOME Wayland is not supported.
//! - input: `xdotool` on X11 only.
//! - UI tree: none.
//!
//! Every failure is a friendly error. OWNER: agent "platform".

use std::process::{Command, Stdio};
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use copilot_core::platform::{Frame, Input, PhysPoint, Platform, Screen, UiElement, UiTree};

use crate::MonitorInfo;
use crate::keys::{KeyName, Modifier, check_typeable, parse_combo};

pub struct LinuxPlatform;

impl LinuxPlatform {
    #[allow(clippy::new_ret_no_self)] // design: `new` hands out the trait bundle
    pub fn new() -> Result<Platform> {
        let me = Arc::new(LinuxPlatform);
        Ok(Platform { screen: me.clone(), input: me.clone(), ui: me })
    }
}

fn is_x11() -> bool {
    std::env::var("XDG_SESSION_TYPE").map(|s| s == "x11").unwrap_or(false)
        || (std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_some())
}

fn is_wlroots() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_CURRENT_DESKTOP").map(|d| !d.to_lowercase().contains("gnome")).unwrap_or(true)
}

/// Run a helper program and return stdout; a missing program becomes a readable error.
fn run(cmd: &str, args: &[&str]) -> Result<Vec<u8>> {
    run_with_input(cmd, args, None)
}

/// Like `run`, but feeds `input` on stdin (keeps typed text out of the process list).
fn run_with_input(cmd: &str, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("{cmd} is not available on this desktop ({e})"))?;
    if let (Some(data), Some(mut stdin)) = (input, child.stdin.take()) {
        use std::io::Write;
        stdin.write_all(data)?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("{cmd} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

fn need_x11() -> Result<()> {
    if is_x11() { Ok(()) } else { bail!("not supported on this desktop") }
}

pub fn monitors() -> Result<Vec<MonitorInfo>> {
    let f = LinuxPlatform.capture_primary()?;
    Ok(vec![MonitorInfo {
        name: "screen".into(),
        x: 0,
        y: 0,
        w: f.image.width(),
        h: f.image.height(),
        scale: 1.0,
        primary: true,
    }])
}

pub fn cursor_pos() -> Option<PhysPoint> {
    if !is_x11() {
        return None;
    }
    // "x:12 y:34 screen:0 window:..."
    let out = String::from_utf8(run("xdotool", &["getmouselocation"]).ok()?).ok()?;
    let mut x = None;
    let mut y = None;
    for part in out.split_whitespace() {
        if let Some(v) = part.strip_prefix("x:") {
            x = v.parse().ok();
        } else if let Some(v) = part.strip_prefix("y:") {
            y = v.parse().ok();
        }
    }
    Some(PhysPoint { x: x?, y: y? })
}

impl Screen for LinuxPlatform {
    fn capture_primary(&self) -> Result<Frame> {
        let png = if is_wlroots() {
            run("grim", &["-t", "png", "-"])?
        } else if is_x11() {
            run("import", &["-silent", "-window", "root", "png:-"])?
        } else {
            bail!("screen capture is not supported on this desktop");
        };
        let image = image::load_from_memory(&png)?.into_rgba8();
        Ok(Frame { image, origin: PhysPoint { x: 0, y: 0 }, scale: 1.0 })
    }
}

impl Input for LinuxPlatform {
    fn move_to(&self, p: PhysPoint) -> Result<()> {
        need_x11()?;
        run("xdotool", &["mousemove", &p.x.to_string(), &p.y.to_string()]).map(drop)
    }
    fn click(&self, p: PhysPoint) -> Result<()> {
        need_x11()?;
        run("xdotool", &["mousemove", &p.x.to_string(), &p.y.to_string(), "sleep", "0.06", "click", "1"]).map(drop)
    }
    fn type_text(&self, s: &str) -> Result<()> {
        check_typeable(s)?;
        need_x11()?;
        // Text via stdin, not argv: argv is visible to every user in the process list.
        run_with_input("xdotool", &["type", "--delay", "12", "--file", "-"], Some(s.as_bytes())).map(drop)
    }
    fn key(&self, combo: &str) -> Result<()> {
        let sym = xdotool_combo(combo)?;
        need_x11()?;
        run("xdotool", &["key", "--", &sym]).map(drop)
    }
    fn scroll(&self, p: PhysPoint, dy: i32) -> Result<()> {
        need_x11()?;
        if dy == 0 {
            return Ok(());
        }
        let button = if dy > 0 { "5" } else { "4" };
        let n = dy.unsigned_abs().min(50).to_string();
        run("xdotool", &["mousemove", &p.x.to_string(), &p.y.to_string(), "click", "--repeat", &n, button]).map(drop)
    }
}

/// "ctrl+shift+a" -> "ctrl+shift+a" in xdotool keysym spelling.
fn xdotool_combo(combo: &str) -> Result<String> {
    let c = parse_combo(combo)?;
    let mut parts: Vec<String> = c
        .mods
        .iter()
        .map(|m| match m {
            Modifier::Ctrl => "ctrl",
            Modifier::Shift => "shift",
            Modifier::Alt => "alt",
            Modifier::Win => "super",
        })
        .map(String::from)
        .collect();
    if let Some(k) = c.key {
        parts.push(match k {
            KeyName::Enter => "Return".into(),
            KeyName::Tab => "Tab".into(),
            KeyName::Escape => "Escape".into(),
            KeyName::Space => "space".into(),
            KeyName::Backspace => "BackSpace".into(),
            KeyName::Delete => "Delete".into(),
            KeyName::Insert => "Insert".into(),
            KeyName::Up => "Up".into(),
            KeyName::Down => "Down".into(),
            KeyName::Left => "Left".into(),
            KeyName::Right => "Right".into(),
            KeyName::Home => "Home".into(),
            KeyName::End => "End".into(),
            KeyName::PageUp => "Prior".into(),
            KeyName::PageDown => "Next".into(),
            KeyName::F(n) => format!("F{n}"),
            KeyName::Char('+') => "plus".into(),
            KeyName::Char(ch) => ch.to_string(),
        });
    }
    Ok(parts.join("+"))
}

impl UiTree for LinuxPlatform {
    fn element_at(&self, _p: PhysPoint) -> Option<UiElement> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdotool_spelling() {
        assert_eq!(xdotool_combo("ctrl+a").unwrap(), "ctrl+a");
        assert_eq!(xdotool_combo("Enter").unwrap(), "Return");
        assert_eq!(xdotool_combo("shift+pagedown").unwrap(), "shift+Next");
        assert!(xdotool_combo("ctrl+zz").is_err());
    }

    #[test]
    fn helper_errors_are_readable_and_stdin_is_fed() {
        let e = run("papierkram-no-such-helper", &[]).unwrap_err().to_string();
        assert!(e.contains("not available"), "{e}");
        assert_eq!(run_with_input("cat", &[], Some("Grüße".as_bytes())).unwrap(), "Grüße".as_bytes());
        assert!(run("false", &[]).is_err());
    }
}
