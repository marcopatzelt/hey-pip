//! Platform probe: lists screens, takes one screenshot, then prints the UI element under the
//! mouse once per second (move the mouse around to test).
//!
//!   probe [seconds=10] [--shot out.png] [--find "Absenden"] [--click X Y]
//!
//! `--shot` saves the primary-screen capture as sent to the model (long side <= 1600).
//! `--click` moves the mouse there and clicks (to check that clicks land on the right pixel).

use std::time::{Duration, Instant};

use anyhow::Result;
use copilot_core::platform::PhysPoint;

fn main() -> Result<()> {
    copilot_platform::set_dpi_awareness();
    let mut args = std::env::args().skip(1);
    let mut seconds = 10u64;
    let mut shot = None;
    let mut find = None;
    let mut click = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--shot" => shot = args.next(),
            "--find" => find = args.next(),
            "--click" => {
                let x = args.next().and_then(|v| v.parse().ok());
                let y = args.next().and_then(|v| v.parse().ok());
                click = x.zip(y).map(|(x, y)| PhysPoint { x, y });
            }
            n => seconds = n.parse().unwrap_or(seconds),
        }
    }

    match copilot_platform::monitors() {
        Ok(ms) => {
            for m in ms {
                println!(
                    "monitor {:?}: {}x{} at ({},{}) scale {:.2}{}",
                    m.name,
                    m.w,
                    m.h,
                    m.x,
                    m.y,
                    m.scale,
                    if m.primary { " PRIMARY" } else { "" }
                );
            }
        }
        Err(e) => println!("monitors: {e:#}"),
    }

    let platform = copilot_platform::native()?;
    let t = Instant::now();
    match platform.screen.capture_primary() {
        Ok(f) => {
            let cap_ms = t.elapsed().as_millis();
            let api = f.for_api(1600)?;
            println!(
                "capture: {}x{} origin ({},{}) scale {:.2} in {cap_ms} ms; api image {}x{} factor {:.3}, {} KB",
                f.image.width(),
                f.image.height(),
                f.origin.x,
                f.origin.y,
                f.scale,
                api.w,
                api.h,
                api.factor,
                api.png.len() / 1024
            );
            if let Some(path) = &shot {
                std::fs::write(path, &api.png)?;
                println!("saved {path}");
            }
        }
        Err(e) => println!("capture: {e:#}"),
    }

    #[cfg(windows)]
    if let Some(name) = &find {
        let win = copilot_platform::windows::WinPlatform::new_arc()?;
        let hits = win.find(name);
        println!("find {name:?} in foreground window: {} hit(s)", hits.len());
        for h in hits {
            println!(
                "  {:?} [{}] password={} at {:?}",
                h.element.name, h.element.control, h.element.is_password, h.rect
            );
        }
    }
    #[cfg(not(windows))]
    if find.is_some() {
        println!("find: only on Windows");
    }

    if let Some(p) = click {
        println!("click at ({},{}) in 3 s ...", p.x, p.y);
        std::thread::sleep(Duration::from_secs(3));
        match platform.input.click(p) {
            Ok(()) => println!("clicked; cursor now {:?}", copilot_platform::cursor_pos()),
            Err(e) => println!("click: {e:#}"),
        }
    }

    println!("element under the mouse, {seconds} s:");
    let mut last = None;
    for _ in 0..seconds {
        let pos = copilot_platform::cursor_pos();
        let el = pos.and_then(|p| platform.ui.element_at(p));
        let line = format!("  cursor {pos:?} -> {el:?}");
        if last.as_ref() != Some(&line) {
            println!("{line}");
            last = Some(line);
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}
