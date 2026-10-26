//! Implementations of `pip_core::platform` traits.
//! - windows: xcap (capture) + enigo (mouse/keyboard) + uiautomation (element names, IsPassword, find by name)
//! - linux: best effort via helper programs (grim/import, xdotool on X11); no UI tree ("Linux in theory")
//! - fake: a PNG as the screen, input recorded (CLI, eval, tests)
//!
//! OWNER: agent "platform" (crates/platform/**, crates/core/src/platform.rs).

pub mod fake;
pub mod keys;
pub mod outline_cache;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod windows;

pub use pip_core::platform::Platform;
use pip_core::platform::{PhysPoint, PhysRect, UiElement};

/// One screen as the OS reports it (physical pixels).
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub scale: f32,
    pub primary: bool,
}

/// A UI element found by name (Windows UI Automation), with its physical bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundElement {
    pub element: UiElement,
    pub rect: PhysRect,
}

/// The real platform for this OS (Windows: WinPlatform, Linux: LinuxPlatform).
pub fn native() -> anyhow::Result<Platform> {
    #[cfg(windows)]
    {
        windows::WinPlatform::new()
    }
    #[cfg(target_os = "linux")]
    {
        linux::LinuxPlatform::new()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        anyhow::bail!("this operating system is not supported")
    }
}

/// Screens of this computer (Windows: all monitors; Linux: the captured screen).
pub fn monitors() -> anyhow::Result<Vec<MonitorInfo>> {
    #[cfg(windows)]
    {
        windows::monitors()
    }
    #[cfg(target_os = "linux")]
    {
        linux::monitors()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        anyhow::bail!("this operating system is not supported")
    }
}

/// Mouse position in physical pixels, if the OS lets us read it.
pub fn cursor_pos() -> Option<PhysPoint> {
    #[cfg(windows)]
    {
        windows::cursor_pos()
    }
    #[cfg(target_os = "linux")]
    {
        linux::cursor_pos()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        None
    }
}

/// Make coordinates physical pixels (Windows; no-op elsewhere). Call once at CLI start.
pub fn set_dpi_awareness() {
    #[cfg(windows)]
    windows::set_dpi_awareness();
}
