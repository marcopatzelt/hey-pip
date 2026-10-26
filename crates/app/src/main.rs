//! Pip: the overlay app (Tauri 2). One full-screen transparent window "pip";
//! click-through toggled from Rust by polling the cursor against HitRects.
//!
//! Env switches: PIP_MOCK=1 (scripted demo without core), PIP_WINDOWED=1 (corner window fallback),
//! PIP_MUTE=1 (no voice), PIP_LANG=en, PIP_SKIP_ONBOARDING=1, PIP_TICK_SECS, PIP_MOCK_REMINDER_SECS.

// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;
mod bridge;
mod mock;
mod overlay;
mod tray;
mod voice;

fn main() {
    use tracing_subscriber::fmt::writer::{BoxMakeWriter, MakeWriterExt};
    // Log to stdout and to data_dir/pip.log: the Windows release build has no console.
    let log = pip_core::config::log_file();
    let _ = std::fs::create_dir_all(log.parent().unwrap_or(std::path::Path::new(".")));
    let too_big = std::fs::metadata(&log).is_ok_and(|m| m.len() > 2_000_000);
    let file = std::fs::OpenOptions::new().create(true).append(!too_big).write(true).truncate(too_big).open(&log);
    let writer = match file {
        Ok(f) => BoxMakeWriter::new(std::io::stdout.and(std::sync::Mutex::new(f))),
        Err(_) => BoxMakeWriter::new(std::io::stdout),
    };
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(writer)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tao=warn,wry=warn".into())
                // enigo logs every typed text at debug level: never let RUST_LOG=debug write it to the log.
                .add_directive("enigo=warn".parse().expect("static directive")),
        )
        .init();
    // A panic in a core part becomes a friendly sentence (backend::guard),
    // so log it once as a warning instead of a scary stderr dump.
    std::panic::set_hook(Box::new(|info| {
        let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        tracing::warn!("panic at {loc}: {msg}");
    }));
    if let Err(e) = bridge::run() {
        tracing::error!("app failed: {e:#}");
        std::process::exit(1);
    }
}
