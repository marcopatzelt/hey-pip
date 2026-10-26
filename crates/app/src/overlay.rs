//! Full-screen transparent overlay with click-through: every 33 ms we compare the cursor with the
//! rects the UI reports (Pip, bubble, card) and let clicks pass everywhere else.
//! Fallback (`PIP_WINDOWED=1`): a 420x360 corner window that is always clickable.
//! The same poll also notices a person moving the mouse while Pip acts (SPEC §0: move = stop).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use copilot_core::events::Bus;
use copilot_core::platform::{PhysPoint, PhysRect};
use serde::Serialize;
use tauri::{PhysicalPosition, PhysicalSize, WebviewWindow};

pub const CORNER_W: u32 = 420;
pub const CORNER_H: u32 = 360;

/// What the UI needs to turn physical screen pixels into CSS pixels: css = (phys - origin) / scale.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct OverlayInfo {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub scale: f64,
    pub windowed: bool,
}

pub fn info(win: &WebviewWindow, windowed: bool) -> Result<OverlayInfo> {
    let pos = win.inner_position()?;
    let size = win.inner_size()?;
    Ok(OverlayInfo { x: pos.x, y: pos.y, w: size.width, h: size.height, scale: win.scale_factor()?, windowed })
}

/// While Pip acts, a person moving the mouse stops him. `armed` says when to watch;
/// `on_user_move` runs once per detected move.
pub struct MouseStop {
    pub armed: Arc<AtomicBool>,
    /// No watching until then (e.g. a few seconds after "Ja": the presenter's hand moves on).
    pub quiet_until: Arc<Mutex<Option<Instant>>>,
    pub on_user_move: Box<dyn Fn() + Send + Sync>,
}

impl MouseStop {
    fn watching(&self, now: Instant) -> bool {
        let quiet = self.quiet_until.lock().map(|q| q.is_some_and(|t| now < t)).unwrap_or(false);
        self.armed.load(Ordering::SeqCst) && !quiet
    }
}

/// Keyboard focus. The overlay never takes it by a click (not focusable: clicking "Ja" keeps the browser in
/// front, so Pip's next Enter or Tab goes to the page). Text fields (ask box, name) take it on purpose and
/// give it back to the window that had it.
#[derive(Default)]
pub struct FocusKeeper {
    prev: Mutex<Option<isize>>,
}

impl FocusKeeper {
    pub fn take(&self, win: &WebviewWindow) {
        #[cfg(windows)]
        {
            let fg = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() } as isize;
            let ours = win.hwnd().map(|h| h.0 as isize).unwrap_or(0);
            if fg != 0 && fg != ours {
                *self.prev.lock().expect("focus lock") = Some(fg);
            }
        }
        let _ = win.set_focusable(true);
        let _ = win.set_focus();
    }

    /// Back to the window that had the keyboard before (if any); the overlay is not focusable again.
    pub fn give_back(&self, win: &WebviewWindow) {
        let _ = win.set_focusable(false);
        let prev = self.prev.lock().expect("focus lock").take();
        #[cfg(windows)]
        if let Some(h) = prev {
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(h as _) };
        }
        #[cfg(not(windows))]
        let _ = prev;
    }
}

/// Size the window to the primary monitor (or the corner fallback), show it, start the cursor poll
/// (click-through outside Pip's rects; mouse-move stop).
pub fn setup(win: &WebviewWindow, bus: Bus, windowed: bool, mouse_stop: MouseStop) -> Result<()> {
    if let Some(m) = win.primary_monitor()? {
        // The work area leaves the taskbar free (Pip must never sit on it).
        let wa = m.work_area();
        let (mpos, msize) =
            if wa.size.width > 0 && wa.size.height > 0 { (wa.position, wa.size) } else { (*m.position(), *m.size()) };
        if windowed {
            let s = m.scale_factor();
            let (w, h) = ((CORNER_W as f64 * s) as u32, (CORNER_H as f64 * s) as u32);
            win.set_size(PhysicalSize::new(w, h))?;
            let x = mpos.x + msize.width as i32 - w as i32 - (16.0 * s) as i32;
            let y = mpos.y + msize.height as i32 - h as i32 - (56.0 * s) as i32;
            win.set_position(PhysicalPosition::new(x, y))?;
        } else {
            win.set_position(mpos)?;
            win.set_size(msize)?;
        }
    }
    win.show()?;
    // Clicks on Pip, the card or a bubble must not pull the keyboard away from the page Pip works on.
    if let Err(e) = win.set_focusable(false) {
        tracing::debug!("set_focusable: {e}");
    }
    // Wayland ignores positions; maximizing is the only way to cover the screen there ("Linux in theory").
    #[cfg(target_os = "linux")]
    if !windowed {
        let _ = win.set_resizable(true);
        let _ = win.maximize();
    }
    if !windowed {
        win.set_ignore_cursor_events(true)?;
    }
    let win = win.clone();
    tauri::async_runtime::spawn(async move {
        let mut ignoring = true;
        let mut watch = MouseWatch::default();
        let mut tick = tokio::time::interval(Duration::from_millis(33));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let Ok(pos) = win.cursor_position() else { continue };
            let p = PhysPoint { x: pos.x.round() as i32, y: pos.y.round() as i32 };
            let now = Instant::now();
            if watch.feed(p, mouse_stop.watching(now), now) {
                (mouse_stop.on_user_move)();
            }
            if windowed {
                continue;
            }
            let ignore = !inside_any(&bus.hit_rects(), p);
            if ignore != ignoring {
                match win.set_ignore_cursor_events(ignore) {
                    Ok(()) => ignoring = ignore,
                    Err(e) => tracing::debug!("set_ignore_cursor_events: {e}"),
                }
            }
        }
    });
    Ok(())
}

/// Ticks of continuous motion that count as a person (the agent jumps the cursor in one tick).
const MOVE_TICKS: u32 = 3;
/// Minimum path of that motion in physical px (a resting hand jitters a few px).
const MOVE_PATH: f64 = 60.0;
/// After arming (Pip starts, or the user just clicked "Ja"), the hand is still on the mouse.
const MOVE_GRACE: Duration = Duration::from_millis(2000);

/// Tells a person moving the mouse from the agent's own single cursor jumps.
#[derive(Debug, Default)]
pub struct MouseWatch {
    last: Option<PhysPoint>,
    armed_at: Option<Instant>,
    streak: u32,
    path: f64,
}

impl MouseWatch {
    /// Feed one cursor sample (every ~33 ms). True once when a person moved the mouse while armed.
    pub fn feed(&mut self, p: PhysPoint, armed: bool, now: Instant) -> bool {
        if !armed {
            *self = Self { last: Some(p), ..Self::default() };
            return false;
        }
        let since = *self.armed_at.get_or_insert(now);
        let Some(last) = self.last.replace(p) else { return false };
        let d = f64::from(p.x - last.x).hypot(f64::from(p.y - last.y));
        if d < 2.0 || now.duration_since(since) < MOVE_GRACE {
            self.streak = 0;
            self.path = 0.0;
            return false;
        }
        self.streak += 1;
        self.path += d;
        if self.streak >= MOVE_TICKS && self.path >= MOVE_PATH {
            self.streak = 0;
            self.path = 0.0;
            return true;
        }
        false
    }
}

pub fn inside_any(rects: &[PhysRect], p: PhysPoint) -> bool {
    rects.iter().any(|r| r.contains(p))
}

/// Keep only sane rects (the UI may send empty or absurd ones while animating).
pub fn sanitize(rects: Vec<PhysRect>) -> Vec<PhysRect> {
    rects.into_iter().filter(|r| r.w > 0 && r.h > 0 && r.w < 20_000 && r.h < 20_000).take(32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_test() {
        let rects = vec![PhysRect { x: 100, y: 100, w: 50, h: 50 }, PhysRect { x: 0, y: 0, w: 10, h: 10 }];
        assert!(inside_any(&rects, PhysPoint { x: 120, y: 149 }));
        assert!(inside_any(&rects, PhysPoint { x: 0, y: 0 }));
        assert!(!inside_any(&rects, PhysPoint { x: 150, y: 120 }));
        assert!(!inside_any(&[], PhysPoint { x: 1, y: 1 }));
    }

    #[test]
    fn sanitize_drops_empty() {
        let r = sanitize(vec![PhysRect { x: 0, y: 0, w: 0, h: 5 }, PhysRect { x: 1, y: 1, w: 5, h: 5 }]);
        assert_eq!(r.len(), 1);
    }

    fn feed_path(w: &mut MouseWatch, t0: Instant, start_ms: u64, pts: &[(i32, i32)]) -> bool {
        let mut hit = false;
        for (i, (x, y)) in pts.iter().enumerate() {
            let now = t0 + Duration::from_millis(start_ms + 33 * i as u64);
            hit |= w.feed(PhysPoint { x: *x, y: *y }, true, now);
        }
        hit
    }

    #[test]
    fn person_moving_the_mouse_is_noticed() {
        let t0 = Instant::now();
        let mut w = MouseWatch::default();
        assert!(!w.feed(PhysPoint { x: 500, y: 500 }, true, t0));
        // a hand gliding across the screen after the grace time
        assert!(feed_path(&mut w, t0, 2100, &[(500, 500), (520, 505), (545, 512), (575, 520), (610, 530)]));
    }

    #[test]
    fn agent_jumps_and_jitter_are_not_a_person() {
        let t0 = Instant::now();
        let mut w = MouseWatch::default();
        w.feed(PhysPoint { x: 100, y: 100 }, true, t0);
        // the agent clicks: one jump, then the cursor rests (repeatedly)
        assert!(!feed_path(&mut w, t0, 2100, &[(900, 700), (900, 700), (900, 700), (300, 200), (300, 200)]));
        // a resting hand: 1 px jitter
        assert!(!feed_path(&mut w, t0, 3000, &[(300, 201), (301, 201), (301, 200), (300, 200), (301, 201)]));
    }

    #[test]
    fn quiet_time_after_yes() {
        let ms = MouseStop {
            armed: Arc::new(AtomicBool::new(true)),
            quiet_until: Arc::new(Mutex::new(None)),
            on_user_move: Box::new(|| {}),
        };
        let t0 = Instant::now();
        assert!(ms.watching(t0));
        *ms.quiet_until.lock().unwrap() = Some(t0 + Duration::from_secs(5));
        assert!(!ms.watching(t0 + Duration::from_secs(1)));
        assert!(ms.watching(t0 + Duration::from_secs(6)));
        ms.armed.store(false, Ordering::SeqCst);
        assert!(!ms.watching(t0 + Duration::from_secs(6)));
    }

    #[test]
    fn grace_time_and_disarm() {
        let t0 = Instant::now();
        let mut w = MouseWatch::default();
        w.feed(PhysPoint { x: 0, y: 0 }, true, t0);
        // the user still moves away from the "Ja" button right after clicking it
        assert!(!feed_path(&mut w, t0, 100, &[(30, 0), (60, 0), (90, 0), (120, 0), (150, 0)]));
        // not armed (card open, Pip idle): never fires
        let mut w = MouseWatch::default();
        for i in 0..200 {
            assert!(!w.feed(PhysPoint { x: i * 20, y: 0 }, false, t0 + Duration::from_millis(33 * i as u64)));
        }
    }
}
