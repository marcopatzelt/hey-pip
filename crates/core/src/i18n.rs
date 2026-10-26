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
    De,
    /// Default: the judges and the demo are English. German stays for later.
    #[default]
    En,
}

impl Lang {
    /// "de"/"en" (profile value); anything else -> En.
    pub fn from_code(s: &str) -> Self {
        if s.trim().eq_ignore_ascii_case("de") { Lang::De } else { Lang::En }
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
        "Hallo! Ich bin Pip. Ich helfe dir mit Briefen, Formularen und allem am Computer.",
        "Hi! I'm Pip. I help you with letters, forms and everyday things on your computer.",
    ),
    ("onb_ask_name", "Wie darf ich dich nennen?", "What should I call you?"),
    ("onb_next", "Weiter", "Next"),
    (
        "onb_privacy",
        "Deine Dateien bleiben auf diesem Computer. Nur was du mir zeigst, schicke ich zum Verstehen an Metas KI.",
        "Your files stay on this computer. Only what you show me goes to Meta's AI, so I can understand it.",
    ),
    ("onb_drop_letter", "Wobei kann ich dir helfen?", "What can I help you with?"),
    ("btn_look_screen", "Schau auf meinen Bildschirm", "Look at my screen"),
    ("btn_read_letter", "Lies einen Brief für mich", "Read a letter for me"),
    ("btn_type_or_say", "Oder sag mir einfach, was du brauchst", "Or just tell me what you need"),
    (
        "task_look_screen",
        "Schau auf meinen Bildschirm. Sag EINEN kurzen Satz: was offen ist und was du jetzt tust. Dann tu es sofort: Füll jedes Feld aus, das du aus WHAT YOU KNOW kennst, häng den passenden Brief an und halte erst am Senden-Knopf an (is_commit). Frag mich nur, wenn ein Wert weder im Gedächtnis noch auf dem Bildschirm steht oder zwei Aufgaben gleich wahrscheinlich sind. Gibt es hier nichts zu tun, biete mir 2 oder 3 konkrete Dinge an.",
        "Look at my screen. Say ONE short sentence: what is open and what you will do about it. Then do it right away: fill every field you can from WHAT YOU KNOW, attach the letter that fits, and stop only at the send button (is_commit). Ask me only if a value is missing from memory AND the screen, or if two different tasks are equally likely. If there is nothing here for you to do, offer 2 or 3 concrete things you can do instead.",
    ),
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
    ("approve_title_send", "Bereit zum Abschicken an {to}?", "Ready to send to {to}?"),
    ("approve_remember_site", "Für {to} nicht mehr fragen", "Don't ask again for {to}"),
    ("approve_remembered", "Ich merke mir: {fact}", "I'll remember: {fact}"),
    (
        "sent_trusted",
        "Abgeschickt. Du hast gesagt, dass ich bei {to} nicht fragen muss.",
        "Sent. You told me I don't need to ask for {to}.",
    ),
    ("approve_title_attach", "Darf ich das anhängen?", "May I attach this?"),
    ("approve_what", "Was: {what}", "What: {what}"),
    ("approve_to", "An: {to}", "To: {to}"),
    ("btn_yes_send", "Abschicken", "Send"),
    ("btn_answer_ok", "OK", "OK"),
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
    (
        "done_learned_form",
        "Ich habe mir auch gemerkt, wie dieses Formular geht. Nächstes Mal geht es schneller.",
        "I also learned how this form works. Next time will be faster.",
    ),
    // --- "What Pip knows" ---
    ("tray_knowledge", "Was Pip weiß", "What Pip knows"),
    ("know_title", "Was ich weiß", "What I know"),
    ("know_about_you", "Über dich", "About you"),
    ("know_letters", "Deine Briefe", "Your letters"),
    ("know_places", "Seiten, die ich kenne", "Places I know"),
    ("know_trusted", "Hier schicke ich ab, ohne zu fragen", "Where I may send without asking"),
    ("know_empty", "Noch nichts.", "Nothing yet."),
    ("btn_forget", "Vergessen", "Forget"),
    ("know_forgot", "Gut, das habe ich vergessen.", "OK, I've forgotten that."),
    ("know_src_user", "du hast es mir gesagt", "you told me"),
    ("know_src_typed", "du hast es bestätigt", "you confirmed it"),
    ("know_src_letter_date", "aus deinem Brief vom {date}", "from your letter of {date}"),
    ("know_src_letter", "aus einem Brief", "from a letter"),
    ("know_src_model", "beim Helfen bemerkt", "I noticed it while helping"),
    ("know_place_once", "{task} · einmal, am {date}", "{task} · once, on {date}"),
    ("know_place_runs", "{task} · {n}-mal, zuletzt am {date}", "{task} · {n} times, last on {date}"),
    ("know_task_send_au", "Krankschreibung schicken", "Sending a sick note"),
    ("know_task_send_letter", "Brief schicken", "Sending a letter"),
    ("know_task_fill_form", "Formular ausfüllen", "Filling in a form"),
    ("know_task_other", "Helfen auf dieser Seite", "Helping on this page"),
    ("know_trusted_detail", "Ich schicke dort ab, ohne vorher zu fragen.", "I send there without asking first."),
    ("know_letter_from", "{type} von {sender}", "{type} from {sender}"),
    ("know_type_au", "Krankschreibung", "Sick note"),
    ("know_type_decision", "Bescheid", "Decision"),
    ("know_type_invoice", "Rechnung", "Bill"),
    ("know_type_reminder", "Mahnung", "Payment reminder"),
    ("know_type_contract", "Vertrag", "Contract"),
    ("know_type_letter", "Brief", "Letter"),
    ("know_key_full_name", "Name", "Name"),
    ("know_key_name", "Vorname", "First name"),
    ("know_key_krankenkasse", "Krankenkasse", "Health insurer"),
    ("know_key_versichertennummer", "Versichertennummer", "Insurance number"),
    ("know_key_arbeitgeber", "Arbeitgeber", "Employer"),
    ("know_key_address", "Adresse", "Address"),
    ("know_key_date_of_birth", "Geburtstag", "Birthday"),
    ("know_key_phone", "Telefon", "Phone"),
    ("know_key_email", "E-Mail", "Email"),
    ("know_key_iban", "Bankkonto", "Bank account"),
    ("know_key_bundesland", "Bundesland", "State"),
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
    ("brief_hi", "Hallo {name}! Das steht an.", "Hi {name}! Here's what's coming up."),
    ("brief_hi_noname", "Hallo! Das steht an.", "Hi! Here's what's coming up."),
    ("brief_offer", "Ich kann „{quote}“ jetzt für dich erledigen.", "I can do \"{quote}\" for you right now."),
    ("btn_brief_go", "Ja, mach das", "Yes, do it now"),
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
        "Frist: {title}, bis {due}. Ich erinnere dich rechtzeitig.",
        "Deadline: {title}, by {due}. I'll remind you in time.",
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
    // --- after-task discovery (Pip offers one more thing that fits) ---
    (
        "discover_progress",
        "Ich schaue kurz, ob deine Kasse noch etwas für dich hat.",
        "Let me quickly check if your insurer has something else for you.",
    ),
    (
        "sugg_text",
        "{insurer} bietet auch „{quote}“ an. Das passt zu {reason}. Sollen wir das zusammen machen?",
        "{insurer} also offers \"{quote}\", which fits {reason}. Shall we do it together?",
    ),
    ("sugg_reason_letter", "ihrem Brief vom {date}", "their letter of {date}"),
    ("sugg_reason_deadline", "deiner Frist am {date}", "your deadline on {date}"),
    ("sugg_insurer", "Deine Krankenkasse", "Your health insurer"),
    (
        "sugg_why",
        "Das habe ich auf der Service-Seite deiner Kasse gesehen. Es passt zu {reason}. Das ist ein Hinweis, keine Rechtsberatung. Nichts wird ohne dein Ja abgeschickt.",
        "I saw this on your insurer's services page. It fits {reason}. This is a hint, not legal advice. Nothing is sent without your yes.",
    ),
    ("btn_sugg_yes", "Ja, machen wir", "Yes, let's do it"),
    ("btn_sugg_why", "Warum?", "Why?"),
    ("sugg_declined", "Alles klar, ich frage nicht nochmal.", "Alright, I won't ask again."),
    (
        "done_time",
        "Fertig in {secs} s. Von Hand dauert das etwa {minutes} Minuten.",
        "Done in {secs} s. By hand this takes about {minutes} minutes.",
    ),
    (
        "done_total",
        "Seit wir uns kennen, habe ich dir etwa {total} gespart.",
        "That's about {total} saved since we met.",
    ),
    ("done_deadline_closed", "Die Frist „{title}“ habe ich abgehakt.", "I've ticked off the deadline \"{title}\"."),
    (
        "done_time_one",
        "Fertig in {secs} s. Von Hand dauert das etwa eine Minute.",
        "Done in {secs} s. By hand this takes about a minute.",
    ),
    // --- private mode (letters read by a Meta open-weights model on this computer) ---
    (
        "doc_private_note",
        "Hier auf deinem Computer gelesen. Nichts hat ihn verlassen.",
        "Read here on your computer. Nothing left it.",
    ),
    (
        "doc_private_pdf",
        "Im Privat-Modus lese ich Fotos und Scans. Bei einer PDF mach bitte ein Foto vom Brief, oder schalte den Privat-Modus aus.",
        "In private mode I can read photos and scans. For a PDF, please take a picture of the letter, or turn private mode off.",
    ),
    (
        "doc_private_offline",
        "Mein privates Lesemodell antwortet nicht. Bitte starte es, oder schalte den Privat-Modus aus.",
        "My private reading model isn't answering. Please start it, or turn private mode off.",
    ),
    // --- "Pip did this for me" card (tray entry and ask-box link) ---
    ("btn_my_card", "Was Pip für mich getan hat", "What Pip did for me"),
    // --- loop guard (the same action a third time) ---
    (
        "loop_field",
        "Ich bekomme das Feld „{label}“ nicht richtig hin. Kannst du kurz nachsehen?",
        "I can't get the {label} field right. Can you check it?",
    ),
    ("btn_fixed", "Ich hab's korrigiert", "I fixed it"),
    ("btn_skip", "Überspringen", "Skip it"),
    (
        "loop_stuck",
        "Ich komme hier nicht weiter. Kannst du mal draufschauen?",
        "I keep getting stuck here. Could you have a look?",
    ),
    // --- the guard (warning signs on a page, checked by rules before Pip acts) ---
    ("guard_title", "Moment. Diese Seite kommt mir nicht richtig vor.", "Wait. This page doesn't look right to me."),
    (
        "guard_claims",
        "Hier steht {org}, aber deine {org} ist {site} ({source}).",
        "It says {org}, but your {org} is {site} ({source}).",
    ),
    (
        "guard_lookalike",
        "Die Adresse sieht aus wie {site}, ist es aber nicht.",
        "Its address looks like {site}, but it is not that site.",
    ),
    (
        "guard_sensitive",
        "Sie fragt nach: {field}. Deine Briefe fragen hier nie danach.",
        "It asks for a {field}. Your letters never ask for that here.",
    ),
    ("guard_rushes", "Sie macht Druck: „{phrase}“.", "It tries to rush you: \"{phrase}\"."),
    (
        "guard_fake_warning",
        "Echte Warnungen zeigen nie eine Nummer zum Anrufen. Das ist eine Webseite, nicht dein Computer.",
        "Real warnings never show a phone number to call. This is a web page, not your computer.",
    ),
    (
        "guard_source",
        "Ich weiß nicht sicher, ob das Betrug ist, aber ich würde hier nichts eintippen.",
        "I am not sure it is a scam, but I would not type anything here.",
    ),
    ("guard_from_letter", "aus deinem Brief vom {date}", "from your letter of {date}"),
    ("guard_from_you", "das hast du mir gesagt", "you told me"),
    ("btn_guard_real_site", "Zur echten {org}", "Take me to the real {org}"),
    ("btn_guard_stop", "Hier aufhören", "Stop here"),
    ("btn_guard_continue", "Ich bin sicher, weiter", "I'm sure, continue"),
    (
        "guard_redirected",
        "Ich habe die echte Seite von {org} für dich geöffnet. Drück dort auf „Schau auf meinen Bildschirm“.",
        "I opened the real {org} page for you. Press Look at my screen there.",
    ),
    (
        "guard_stopped",
        "Ich habe aufgehört. Nichts wurde eingetippt oder abgeschickt.",
        "I stopped. Nothing was typed or sent.",
    ),
    ("share_scam_one", "1 falsche Seite gestoppt", "1 fake page stopped"),
    ("share_scam_many", "{n} falsche Seiten gestoppt", "{n} fake pages stopped"),
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
        assert_eq!(
            tf("approve_title_send", Lang::En, &[("to", "www.kasse.example")]),
            "Ready to send to www.kasse.example?"
        );
        assert_eq!(t("btn_yes_send", Lang::En), "Send");
        assert_eq!(t("btn_not_yet", Lang::En), "Not yet");
        assert_eq!(tf("approve_remember_site", Lang::En, &[("to", "x.example")]), "Don't ask again for x.example");
        assert_eq!(
            tf("sent_trusted", Lang::En, &[("to", "x.example")]),
            "Sent. You told me I don't need to ask for x.example."
        );
    }

    #[test]
    fn suggestion_texts_are_honest() {
        assert!(t("sugg_why", Lang::En).contains("not legal advice"));
        assert!(t("sugg_why", Lang::De).contains("keine Rechtsberatung"));
        for lang in [Lang::De, Lang::En] {
            let s = tf("sugg_text", lang, &[("insurer", "X"), ("quote", "Q"), ("reason", "R")]);
            assert!(!s.contains('{'), "{s}");
            assert!(t("sugg_why", lang).contains("{reason}"));
        }
    }
}
