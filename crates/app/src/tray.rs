//! Tray menu: Pause / My deadlines / What Pip knows / Quit (strings via core::i18n).

use anyhow::Result;
use pip_core::i18n::{Lang, t, tf};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Wry};

pub struct Tray {
    pause: MenuItem<Wry>,
    icon: TrayIcon<Wry>,
    lang: Lang,
}

impl Tray {
    /// Label of the pause entry follows the state: "Pause" while awake, "Weiter" while sleeping.
    pub fn set_paused(&self, paused: bool) {
        let key = if paused { "tray_resume" } else { "tray_pause" };
        let _ = self.pause.set_text(t(key, self.lang));
    }

    /// Tooltip: "Pip – nächste Erinnerung: Donnerstag, 29. Oktober, 18:00" (or just "Pip").
    pub fn set_next_reminder(&self, when: Option<&str>) {
        let tip = match when {
            Some(w) => tf("tray_next_reminder", self.lang, &[("when", w)]),
            None => "Pip".to_string(),
        };
        let _ = self.icon.set_tooltip(Some(tip));
    }
}

pub fn setup(app: &AppHandle, lang: Lang) -> Result<Tray> {
    let pause = MenuItem::with_id(app, "pause", t("tray_pause", lang), true, None::<&str>)?;
    let deadlines = MenuItem::with_id(app, "deadlines", t("tray_deadlines", lang), true, None::<&str>)?;
    let knowledge = MenuItem::with_id(app, "knowledge", t("tray_knowledge", lang), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", t("tray_quit", lang), true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&pause, &deadlines, &knowledge, &quit])?;
    let mut builder = TrayIconBuilder::with_id("pip").tooltip("Pip").menu(&menu).show_menu_on_left_click(true);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let icon = builder
        .on_menu_event(|app, ev| match ev.id().as_ref() {
            "pause" => crate::bridge::toggle_pause(app),
            "deadlines" => crate::bridge::show_deadlines(app),
            "knowledge" => crate::bridge::show_knowledge(app),
            "quit" => crate::bridge::ask_quit(app),
            _ => {}
        })
        .build(app)?;
    Ok(Tray { pause, icon, lang })
}
