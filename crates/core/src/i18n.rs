//! All user-facing strings. German first, English second. Plain words for
//! non-technical people: "Brief" not "Dokument", "abschicken" not "senden",
//! "Krankschreibung" not "AU", "Frist" not "Deadline", "anhängen" not "hochladen".
//! Errors: what happened + next step. Never a code.
//!
//! OWNER: agent "app" (wording). Other agents: use existing keys; if you need a new
//! key, list it (key, de, en) in your final report, and use `t()` with that key anyway
//! (unknown keys fall back to the key itself, so code compiles and tests can match).
//! Placeholders: `{name}` style, filled by `tf`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    De,
    En,
}

impl Lang {
    /// "de"/"en" (profile value); anything else -> De.
    pub fn from_code(s: &str) -> Self {
        if s.trim().eq_ignore_ascii_case("en") { Lang::En } else { Lang::De }
    }
    pub fn code(self) -> &'static str {
        match self {
            Lang::De => "de",
            Lang::En => "en",
        }
    }
}

/// (key, de, en)
static TABLE: &[(&str, &str, &str)] = &[
    // --- onboarding ---
    (
        "onb_hello",
        "Hallo! Ich bin Pip. Ich helfe dir mit Briefen und Fristen.",
        "Hi! I'm Pip. I help you with letters and deadlines.",
    ),
    ("onb_ask_name", "Wie darf ich dich nennen?", "What should I call you?"),
    ("onb_next", "Weiter", "Next"),
    (
        "onb_privacy",
        "Deine Briefe bleiben auf diesem Computer. Zum Lesen schicke ich den Brief an Meta, und wenn ich für dich klicke, auch Bilder von deinem Bildschirm.",
        "Your letters stay on this computer. To read a letter, I send it to Meta, and when I click for you, also pictures of your screen.",
    ),
    ("onb_drop_letter", "Zieh einen Brief auf mich. Oder drück hier:", "Drag a letter onto me. Or press here:"),
    ("btn_pick_letter", "Brief auswählen", "Choose a letter"),
    ("onb_ask_state", "Wohnst du in Nordrhein-Westfalen?", "Do you live in North Rhine-Westphalia?"),
    ("btn_yes", "Ja", "Yes"),
    ("btn_elsewhere", "Woanders", "Somewhere else"),
    // --- talking ---
    ("ask_what", "Was soll ich tun?", "What should I do?"),
    ("thinking", "Ich schaue mir das an …", "Let me have a look …"),
    ("listening", "Ich höre zu …", "I'm listening …"),
    // --- acting / stop ---
    ("btn_stop", "Stopp", "Stop"),
    ("btn_resume", "Weiter", "Continue"),
    ("stopped", "Ich habe aufgehört. Nichts wurde abgeschickt.", "I stopped. Nothing was sent."),
    (
        "stopped_after_send",
        "Ich habe aufgehört. Abgeschickt habe ich schon: {what}.",
        "I stopped. I had already sent: {what}.",
    ),
    (
        "stopped_mouse",
        "Du hast die Maus bewegt, darum habe ich angehalten. Nichts wurde abgeschickt.",
        "You moved the mouse, so I stopped. Nothing was sent.",
    ),
    (
        "stopped_mouse_after_send",
        "Du hast die Maus bewegt, darum habe ich angehalten. Abgeschickt habe ich schon: {what}.",
        "You moved the mouse, so I stopped. I had already sent: {what}.",
    ),
    (
        "working_hint",
        "Ich arbeite jetzt. Wenn du die Maus bewegst, halte ich an.",
        "I'm working now. If you move the mouse, I'll stop.",
    ),
    (
        "screen_notice",
        "Damit ich für dich klicken kann, schaue ich auf deinen Bildschirm. Schließ bitte vorher private Fenster.",
        "To click for you, I look at your screen. Please close private windows first.",
    ),
    (
        "screen_notice_mouse",
        "Während ich arbeite, lass bitte die Maus los. Wenn du sie bewegst, halte ich an.",
        "While I work, please let go of the mouse. If you move it, I'll stop.",
    ),
    ("btn_understood", "Verstanden", "Got it"),
    ("btn_not_now", "Lieber nicht", "Not now"),
    ("progress_type_name", "Ich trage deinen Namen ein.", "I'm typing your name."),
    ("progress_open_page", "Ich öffne die Seite.", "I'm opening the page."),
    ("progress_click", "Ich klicke auf {target}.", "I'm clicking {target}."),
    ("progress_type", "Ich trage {what} ein.", "I'm filling in {what}."),
    ("progress_scroll", "Ich scrolle ein Stück.", "I'm scrolling a little."),
    ("progress_wait", "Ich warte kurz.", "I'm waiting a moment."),
    // --- approval card ---
    ("approve_title_send", "Darf ich das abschicken?", "May I send this?"),
    ("approve_title_attach", "Darf ich das anhängen?", "May I attach this?"),
    ("approve_what", "Was: {what}", "What: {what}"),
    ("approve_to", "An: {to}", "To: {to}"),
    ("btn_yes_send", "Ja, abschicken", "Yes, send it"),
    ("btn_yes_attach", "Ja, anhängen", "Yes, attach it"),
    ("btn_not_yet", "Noch nicht", "Not yet"),
    ("approve_title_delete", "Darf ich das löschen?", "May I delete this?"),
    ("btn_yes_delete", "Ja, löschen", "Yes, delete it"),
    ("approve_title_open", "Darf ich diese Seite öffnen?", "May I open this page?"),
    ("btn_yes_open", "Ja, öffnen", "Yes, open it"),
    ("approve_page", "Seite: {host}", "Page: {host}"),
    ("approve_title_remember", "Darf ich mir das merken?", "May I remember this?"),
    ("btn_yes_remember", "Ja, merken", "Yes, remember it"),
    ("btn_no", "Nein", "No"),
    ("approve_before", "Bisher: {old}", "Until now: {old}"),
    ("approve_filled", "Ausgefüllt:", "Filled in:"),
    ("page_local", "Seite auf diesem Computer", "page on this computer"),
    ("approve_speak_send", "Darf ich {what} an {to} abschicken?", "May I send {what} to {to}?"),
    ("approve_speak_attach", "Darf ich {what} anhängen?", "May I attach {what}?"),
    (
        "approve_timeout",
        "Du warst gerade nicht da, darum habe ich nichts abgeschickt. Soll ich weitermachen?",
        "You were away, so I didn't send anything. Shall I continue?",
    ),
    ("btn_continue_now", "Jetzt weitermachen", "Continue now"),
    ("btn_dont_send", "Nicht abschicken", "Don't send"),
    (
        "question_timeout",
        "Ich habe auf deine Antwort gewartet und dann aufgehört. Nichts wurde abgeschickt.",
        "I waited for your answer and then stopped. Nothing was sent.",
    ),
    ("btn_ask_again", "Nochmal fragen", "Ask again"),
    (
        "page_changed",
        "Die Seite hat sich gerade verändert. Ich schaue nochmal hin, bevor ich etwas drücke.",
        "The page just changed. I'll look again before I press anything.",
    ),
    ("attach_other_file", "Ich hänge nur die Datei an, die du erlaubt hast.", "I only attach the file you allowed."),
    (
        "pick_file_yourself",
        "Bitte wähle die Datei selbst aus. Ich warte.",
        "Please choose the file yourself. I'll wait.",
    ),
    (
        "no_passwords",
        "Passwörter tippe ich nicht ein. Bitte gib es selbst ein. Ich warte.",
        "I don't type passwords. Please enter it yourself. I'll wait.",
    ),
    (
        "login_needed",
        "Hier musst du dich selbst anmelden. Sag Bescheid, wenn du fertig bist.",
        "You need to log in here yourself. Tell me when you're done.",
    ),
    // --- documents ---
    ("doc_reading", "Ich lese den Brief.", "I'm reading the letter."),
    (
        "doc_unsure",
        "Bitte schau auf dem Brief nach, ob die Daten stimmen.",
        "Please check on the letter that the dates are right.",
    ),
    (
        "doc_unsure_date",
        "Ich habe als Frist {due} gelesen. Bitte schau auf dem Brief nach, ob das stimmt.",
        "I read {due} as the deadline. Please check on the letter that this is right.",
    ),
    ("btn_correct", "Stimmt", "That's right"),
    ("btn_wrong", "Stimmt nicht", "That's wrong"),
    ("ans_date_ok", "Danke! Dann passe ich auf die Frist auf.", "Thanks! Then I'll watch the deadline."),
    (
        "ans_date_wrong",
        "Gut, ich habe die Frist gelöscht. Sag mir das richtige Datum, dann trage ich es neu ein.",
        "OK, I deleted the deadline. Tell me the right date and I'll add it again.",
    ),
    (
        "doc_au_summary",
        "Krankschreibung von {from} bis {until}. Ich passe auf, dass keine Lücke entsteht.",
        "Sick note from {from} to {until}. I'll make sure there is no gap.",
    ),
    // --- reminders ---
    ("rem_au_ends_tomorrow", "Deine Krankschreibung endet morgen.", "Your sick note ends tomorrow."),
    (
        "rem_au_need_new",
        "Wenn du noch krank bist, brauchst du bis {due} eine neue.",
        "If you're still sick, you need a new one by {due}.",
    ),
    (
        "rem_au_why",
        "Ohne lückenlose Krankschreibung kann die Krankenkasse für die fehlenden Tage kein Krankengeld zahlen. Regel: § 46 Satz 2 SGB V.",
        "Without a gap-free sick note, the health insurer may not pay sick pay for the missing days. Rule: § 46 sentence 2 SGB V.",
    ),
    ("rem_deadline_soon", "Die Frist „{title}“ endet am {due}.", "The deadline \"{title}\" ends on {due}."),
    (
        "rem_widerspruch_why",
        "Gegen einen Bescheid kannst du einen Monat lang Widerspruch einlegen. Der Brief gilt vier Tage nach dem Datum als angekommen.",
        "You can object to a decision for one month. The letter counts as received four days after its date.",
    ),
    ("btn_remind_monday", "Erinnere mich Montag", "Remind me Monday"),
    ("btn_remind_on", "Erinnere mich {day}", "Remind me {day}"),
    ("btn_healthy", "Ich bin gesund", "I'm well again"),
    ("btn_later", "In 2 Stunden", "In 2 hours"),
    ("btn_onb_later", "Später", "Later"),
    ("btn_undo", "Rückgängig", "Undo"),
    ("ans_undone", "Gut, ich erinnere dich weiter daran.", "OK, I'll keep reminding you."),
    ("btn_why", "Warum?", "Why?"),
    ("hint_not_legal_advice", "Das ist ein Hinweis, keine Rechtsberatung.", "This is a hint, not legal advice."),
    // --- done ---
    ("done_remember", "Erledigt. Ich merke mir: {fact}", "Done. I'll remember: {fact}"),
    // --- errors ---
    (
        "err_offline",
        "Ich komme gerade nicht ins Internet. Prüf bitte das WLAN.",
        "I can't reach the internet right now. Please check the Wi-Fi.",
    ),
    (
        "err_generic",
        "Das hat nicht geklappt. Wir können es gleich nochmal versuchen.",
        "That didn't work. We can try again in a moment.",
    ),
    (
        "err_service_busy",
        "Mein Dienst ist gerade überlastet. Bitte versuch es in ein paar Minuten nochmal.",
        "My service is busy right now. Please try again in a few minutes.",
    ),
    (
        "err_setup",
        "Ich bin noch nicht fertig eingerichtet: Mir fehlt mein Schlüssel. Bitte frag die Person, die mich installiert hat.",
        "I'm not fully set up yet: my key is missing. Please ask the person who installed me.",
    ),
    ("setup_key_label", "Schlüssel hier einfügen:", "Paste the key here:"),
    ("setup_key_save", "Speichern", "Save"),
    ("setup_key_saved", "Danke! Ich starte kurz neu.", "Thanks! I'll restart for a moment."),
    (
        "setup_key_bad",
        "Das sieht nicht wie ein Schlüssel aus. Bitte nochmal einfügen.",
        "That doesn't look like a key. Please paste it again.",
    ),
    ("btn_other_photo", "Anderes Foto wählen", "Choose another photo"),
    (
        "err_not_found",
        "Ich finde das auf dem Bildschirm nicht. Kannst du mir zeigen, wo es ist?",
        "I can't find that on the screen. Can you show me where it is?",
    ),
    (
        "err_too_long",
        "Das dauert zu lange. Ich höre lieber auf. Nichts wurde abgeschickt.",
        "This is taking too long. I'll stop. Nothing was sent.",
    ),
    (
        "err_too_long_after_send",
        "Das dauert zu lange. Ich höre lieber auf. Abgeschickt habe ich schon: {what}.",
        "This is taking too long. I'll stop. I had already sent: {what}.",
    ),
    ("btn_retry", "Nochmal", "Try again"),
    // --- tray ---
    ("tray_pause", "Pause", "Pause"),
    ("tray_deadlines", "Meine Fristen", "My deadlines"),
    ("tray_quit", "Beenden", "Quit"),
    ("tray_resume", "Weiter", "Continue"),
    ("tray_next_reminder", "Pip – nächste Erinnerung: {when}", "Pip – next reminder: {when}"),
    (
        "quit_confirm",
        "Wenn ich aus bin, kann ich dich nicht erinnern. Trotzdem beenden?",
        "When I'm off, I can't remind you. Quit anyway?",
    ),
    ("btn_quit_yes", "Ja, beenden", "Yes, quit"),
    ("btn_rather_pause", "Lieber Pause", "Pause instead"),
    (
        "onb_autostart",
        "Ich starte ab jetzt mit dem Computer, damit ich dich erinnern kann.",
        "From now on I start with the computer, so I can remind you.",
    ),
    // --- overlay (app) ---
    ("onb_name_placeholder", "Dein Vorname", "Your first name"),
    ("onb_greet", "Schön, dich kennenzulernen, {name}!", "Nice to meet you, {name}!"),
    ("onb_state_thanks", "Danke, das habe ich mir gemerkt.", "Thanks, I'll remember that."),
    ("onb_state_pick", "In welchem Bundesland wohnst du?", "Which German state do you live in?"),
    (
        "states_list",
        "Baden-Württemberg|Bayern|Berlin|Brandenburg|Bremen|Hamburg|Hessen|Mecklenburg-Vorpommern|Niedersachsen|Nordrhein-Westfalen|Rheinland-Pfalz|Saarland|Sachsen|Sachsen-Anhalt|Schleswig-Holstein|Thüringen",
        "Baden-Württemberg|Bavaria|Berlin|Brandenburg|Bremen|Hamburg|Hesse|Mecklenburg-Western Pomerania|Lower Saxony|North Rhine-Westphalia|Rhineland-Palatinate|Saarland|Saxony|Saxony-Anhalt|Schleswig-Holstein|Thuringia",
    ),
    (
        "welcome_back",
        "Hallo {name}! Klick einfach auf mich, wenn du mich brauchst.",
        "Hi {name}! Just click me when you need me.",
    ),
    (
        "welcome_back_noname",
        "Hallo! Klick einfach auf mich, wenn du mich brauchst.",
        "Hi! Just click me when you need me.",
    ),
    ("later_hint", "Gut. Klick einfach auf mich, wenn du mich brauchst.", "OK. Just click me when you need me."),
    ("chip_send_au", "Schick meine Krankschreibung an die Kasse", "Send my sick note to my insurer"),
    ("chip_last_letter", "Was stand im letzten Brief?", "What did the last letter say?"),
    ("chip_deadlines", "Zeig meine Fristen", "Show my deadlines"),
    ("btn_pause_link", "Pause machen", "Take a break"),
    ("queue_of", "Nachricht {i} von {n}", "Message {i} of {n}"),
    ("no_letter_known", "Ich habe noch keinen Brief von dir.", "I don't have a letter from you yet."),
    (
        "ask_placeholder",
        "Schreib es hier hin. Oder halte das Mikrofon gedrückt.",
        "Type it here. Or hold the microphone.",
    ),
    ("btn_go", "Los", "Go"),
    ("btn_mic", "Gedrückt halten und sprechen", "Hold and speak"),
    ("btn_close", "Schließen", "Close"),
    ("btn_ok", "Gut", "OK"),
    ("drop_hint", "Lass den Brief los. Ich lese ihn.", "Let go of the letter. I'll read it."),
    (
        "err_file_type",
        "Dieses Format kenne ich nicht. Nimm bitte ein Foto als JPG oder PNG, oder ein PDF.",
        "I don't know this format. Please use a JPG or PNG photo, or a PDF.",
    ),
    (
        "err_read_letter",
        "Ich konnte den Brief nicht lesen. Ein helles, scharfes Foto hilft mir.",
        "I couldn't read the letter. A bright, sharp photo helps me.",
    ),
    ("doc_known", "Diesen Brief kenne ich schon.", "I already know this letter."),
    ("paused", "Ich mache Pause. Klick mich an, dann bin ich wieder da.", "I'm taking a break. Click me and I'm back."),
    ("woke", "Ich bin wieder da.", "I'm back."),
    ("busy", "Einen Moment, ich bin noch dabei.", "One moment, I'm still on it."),
    ("heard", "Ich habe verstanden: „{text}“", "I understood: \"{text}\""),
    (
        "voice_too_short",
        "Ich habe nichts gehört. Halte den Knopf gedrückt, während du sprichst.",
        "I didn't hear anything. Keep holding the button while you speak.",
    ),
    (
        "err_mic",
        "Ich kann dich gerade nicht hören. Bitte schreib es mir.",
        "I can't hear you right now. Please type it.",
    ),
    (
        "err_not_ready",
        "Das kann ich gerade noch nicht. Bitte versuch es später nochmal.",
        "I can't do that yet. Please try again later.",
    ),
    ("deadlines_title", "Deine Fristen", "Your deadlines"),
    ("deadlines_none", "Du hast gerade keine offenen Fristen.", "You have no open deadlines right now."),
    ("deadline_until", "bis {due}", "by {due}"),
    ("deadline_in_days", "in {n} Tagen", "in {n} days"),
    ("deadline_overdue", "vorbei", "passed"),
    ("approve_attachments", "Anhang", "Attachment"),
    ("approve_button_preview", "Diesen Knopf drücke ich:", "I will press this button:"),
    ("approve_not_sent", "Gut, ich schicke nichts ab.", "OK, I won't send anything."),
    (
        "weekdays",
        "Montag|Dienstag|Mittwoch|Donnerstag|Freitag|Samstag|Sonntag",
        "Monday|Tuesday|Wednesday|Thursday|Friday|Saturday|Sunday",
    ),
    (
        "months",
        "Januar|Februar|März|April|Mai|Juni|Juli|August|September|Oktober|November|Dezember",
        "January|February|March|April|May|June|July|August|September|October|November|December",
    ),
    ("date_long", "{weekday}, {day}. {month}", "{weekday}, {month} {day}"),
    // --- reminders (rule engine) ---
    ("rem_au_ends_today", "Deine Krankschreibung endet heute.", "Your sick note ends today."),
    ("rem_au_ends_on", "Deine Krankschreibung endet am {date}.", "Your sick note ends on {date}."),
    ("rem_au_ended", "Deine Krankschreibung ist am {date} zu Ende gegangen.", "Your sick note ended on {date}."),
    (
        "rem_au_need_new_today",
        "Wenn du noch krank bist, brauchst du heute eine neue.",
        "If you're still sick, you need a new one today.",
    ),
    (
        "rem_au_why_detail",
        "Die neue Krankschreibung muss spätestens am nächsten Werktag nach dem letzten krankgeschriebenen Tag ausgestellt sein. Samstage zählen dabei nicht als Werktag.",
        "The new sick note must be issued at the latest on the next working day after the last day on the old one. Saturdays do not count as working days.",
    ),
    (
        "rem_widerspruch_ends",
        "Die Frist für einen Widerspruch gegen den Bescheid endet am {due}.",
        "The deadline to object to the decision ends on {due}.",
    ),
    (
        "rem_widerspruch_ends_today",
        "Die Frist für einen Widerspruch gegen den Bescheid endet heute.",
        "The deadline to object to the decision ends today.",
    ),
    (
        "rem_widerspruch_action",
        "Wenn du nicht einverstanden bist, musst du bis dahin widersprechen.",
        "If you don't agree, you have to object by then.",
    ),
    (
        "rem_widerspruch_source",
        "Fällt das Ende auf ein Wochenende oder einen Feiertag, endet die Frist am nächsten Werktag. Regel: § 84 SGG oder § 70 VwGO, Bekanntgabe nach § 37 Abs. 2 SGB X.",
        "If the end falls on a weekend or public holiday, the deadline ends on the next working day. Rule: § 84 SGG or § 70 VwGO, notification under § 37 (2) SGB X.",
    ),
    ("rem_deadline_today", "Die Frist „{title}“ endet heute.", "The deadline \"{title}\" ends today."),
    ("rem_deadline_passed", "Die Frist „{title}“ ist am {due} abgelaufen.", "The deadline \"{title}\" ended on {due}."),
    (
        "rem_widerspruch_passed",
        "Die Frist für einen Widerspruch gegen den Bescheid ist am {due} abgelaufen.",
        "The deadline to object to the decision ended on {due}.",
    ),
    (
        "rem_passed_action",
        "Wenn noch etwas offen ist, frag am besten direkt beim Absender nach.",
        "If something is still open, it's best to ask the sender directly.",
    ),
    ("rem_brief_action", "Das steht so im Brief.", "That's what the letter says."),
    ("rem_brief_quote", "Im Brief steht: „{quote}“", "The letter says: \"{quote}\""),
    (
        "rem_brief_unsure",
        "Das Datum habe ich aus dem Brief gelesen. Bitte prüfe es noch einmal.",
        "I read this date from the letter. Please check it again.",
    ),
    ("btn_done", "Erledigt", "Done"),
    (
        "ans_healthy",
        "Schön, dass es dir besser geht. Ich erinnere dich nicht mehr daran.",
        "Glad you're feeling better. I won't remind you about it again.",
    ),
    ("ans_done", "Gut. Ich erinnere dich nicht mehr daran.", "Good. I won't remind you about it again."),
    ("ans_later", "Okay, ich melde mich in zwei Stunden wieder.", "Okay, I'll check back in two hours."),
    ("ans_remind_on", "Okay, ich erinnere dich {date} um {time} Uhr.", "Okay, I'll remind you {date} at {time}."),
    ("ev_deadline_done", "Frist erledigt: {title}", "Deadline done: {title}"),
    ("day_today", "heute", "today"),
    ("day_tomorrow", "morgen", "tomorrow"),
    // --- documents (pipeline) ---
    (
        "doc_deadline_found",
        "Ich erinnere dich rechtzeitig an die Frist am {due}.",
        "I'll remind you in time about the deadline on {due}.",
    ),
    ("doc_read_done", "Ich habe den Brief gelesen.", "I've read the letter."),
    (
        "doc_unsupported",
        "Diese Datei kann ich nicht lesen. Nimm bitte ein Foto oder ein PDF.",
        "I can't open this file. Please use a photo or a PDF.",
    ),
    (
        "doc_unreadable",
        "Ich konnte den Brief nicht gut lesen. Machst du bitte ein schärferes Foto?",
        "I couldn't read the letter well. Could you take a sharper photo?",
    ),
    // --- agent ---
    ("progress_key", "Ich drücke {key}.", "I'm pressing {key}."),
    ("progress_search", "Ich schaue in meinen Unterlagen nach.", "I'm checking my notes."),
    (
        "progress_retry",
        "Die Verbindung hakt kurz. Ich versuche es gleich nochmal.",
        "The connection is slow. I'll try again in a moment.",
    ),
    (
        "no_letter_yet",
        "Ich habe diesen Brief noch nicht. Zieh ihn auf mich.",
        "I don't have that letter yet. Drag it onto me.",
    ),
    (
        "demo_blocked",
        "Hier darf ich in der Demo nichts abschicken. Nichts wurde abgeschickt.",
        "In the demo I can't send anything here. Nothing was sent.",
    ),
];

/// Lookup. Unknown key -> the key itself (visible in tests, never panics).
pub fn t(key: &str, lang: Lang) -> &str {
    for (k, de, en) in TABLE {
        if *k == key {
            return match lang {
                Lang::De => de,
                Lang::En => en,
            };
        }
    }
    key
}

/// Lookup + replace `{name}` placeholders.
pub fn tf(key: &str, lang: Lang, args: &[(&str, &str)]) -> String {
    let mut s = t(key, lang).to_string();
    for (name, value) in args {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_unique_and_both_languages_filled() {
        let mut seen = std::collections::HashSet::new();
        for (k, de, en) in TABLE {
            assert!(seen.insert(*k), "duplicate key {k}");
            assert!(!de.is_empty() && !en.is_empty(), "empty text for {k}");
        }
    }

    #[test]
    fn placeholders() {
        assert_eq!(tf("approve_to", Lang::De, &[("to", "Gesundkasse")]), "An: Gesundkasse");
        assert_eq!(t("no_such_key", Lang::En), "no_such_key");
    }
}
