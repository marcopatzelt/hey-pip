//! Tray menu: Pause / Always show Pip / Fewer tips / My deadlines / What Pip knows / What Pip did for me / Quit
//! (strings via core::i18n).

use anyhow::Result;
use pip_core::i18n::{Lang, t, tf};
use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Wry};

pub struct Tray {
    pause: MenuItem<Wry>,
    always: CheckMenuItem<Wry>,
    fewer_tips: CheckMenuItem<Wry>,
    icon: TrayIcon<Wry>,
    lang: Lang,
}

impl Tray {
    /// Label of the pause entry follows the state: "Pause" while awake, "Weiter" while sleeping.
    pub fn set_paused(&self, paused: bool) {
        let key = if paused { "tray_resume" } else { "tray_pause" };
        let _ = self.pause.set_text(t(key, self.lang));
    }

    /// The check mark of "Always show Pip" follows the stored mode (the menu toggles it on its own too).
    pub fn set_always(&self, on: bool) {
        let _ = self.always.set_checked(on);
    }

    /// The check mark of "Fewer tips" follows the profile key `tips` ("off" = checked).
    pub fn set_fewer_tips(&self, on: bool) {
        let _ = self.fewer_tips.set_checked(on);
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

/// `always`: the stored mode is "always" (Pip never rests at the edge), so the item starts checked.
/// `fewer_tips`: the profile says `tips` = "off", so "Fewer tips" starts checked.
pub fn setup(app: &AppHandle, lang: Lang, always: bool, fewer_tips: bool) -> Result<Tray> {
    let pause = MenuItem::with_id(app, "pause", t("tray_pause", lang), true, None::<&str>)?;
    let always_item = CheckMenuItem::with_id(app, "always", t("tray_always_show", lang), true, always, None::<&str>)?;
    let fewer = CheckMenuItem::with_id(app, "fewer_tips", t("tray_fewer_tips", lang), true, fewer_tips, None::<&str>)?;
    let deadlines = MenuItem::with_id(app, "deadlines", t("tray_deadlines", lang), true, None::<&str>)?;
    let knowledge = MenuItem::with_id(app, "knowledge", t("tray_knowledge", lang), true, None::<&str>)?;
    let card = MenuItem::with_id(app, "card", t("btn_my_card", lang), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", t("tray_quit", lang), true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&pause, &always_item, &fewer, &deadlines, &knowledge, &card, &quit])?;
    let mut builder = TrayIconBuilder::with_id("pip").tooltip("Pip").menu(&menu).show_menu_on_left_click(true);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let icon = builder
        .on_menu_event(|app, ev| match ev.id().as_ref() {
            "pause" => crate::bridge::toggle_pause(app),
            "always" => crate::bridge::toggle_always(app),
            "fewer_tips" => crate::bridge::toggle_fewer_tips(app),
            "deadlines" => crate::bridge::show_deadlines(app),
            "knowledge" => crate::bridge::show_knowledge(app),
            "card" => crate::bridge::show_card(app),
            "quit" => crate::bridge::ask_quit(app),
            _ => {}
        })
        .build(app)?;
    Ok(Tray { pause, always: always_item, fewer_tips: fewer, icon, lang })
}
