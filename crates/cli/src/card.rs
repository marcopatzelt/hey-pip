//! `card`: the "Pip did this for me" card from memory (no API call). Prints the numbers and the post text,
//! writes the self-contained HTML page.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local};
use pip_core::i18n::Lang;
use pip_core::memory::learn::PROFILE_SECONDS_SAVED;
use pip_core::memory::{ActionLog, DeadlineStatus, Memory, NewDeadline, Playbook, seed_demo_profile};
use pip_core::rules::RuleEngine;

use crate::Ctx;

pub fn run(ctx: &Ctx, out: &Path, demo: bool) -> Result<()> {
    if demo {
        let rules = RuleEngine::new(ctx.memory.clone(), ctx.clock.clone());
        let letters = ctx.fixture("letters");
        if seed_demo_profile(&ctx.memory, &rules, &letters, ctx.now)? {
            demo_history(&ctx.memory, ctx.now)?;
            println!("Demo profile seeded (invented person, letters from {}).", letters.display());
        } else {
            println!("Memory already has a profile: --demo left it as it is.");
        }
    }
    // The profile language wins (like the app); without one, --lang.
    let lang = ctx.memory.profile_get("lang")?.map(|l| Lang::from_code(&l)).unwrap_or(ctx.lang);
    let t = pip_core::memory::tally(&ctx.memory)?;
    println!("letters read:      {}", t.letters_read);
    println!("forms filled in:   {}", t.forms_filled);
    println!("sent after a yes:  {}", t.sent_after_yes);
    println!("deadlines kept:    {}", t.deadlines_kept);
    println!("reminders on time: {}", t.reminders_on_time);
    println!("seconds saved:     {}", t.seconds_saved);
    println!("since:             {}", t.since.map(|d| d.to_string()).unwrap_or_else(|| "-".into()));
    let post = pip_core::memory::share_text(&t, lang);
    println!("\nPost ({} characters):\n{post}\n", post.chars().count());
    let name = ctx.memory.profile_get("name")?;
    let html = pip_core::memory::card_html(&t, name.as_deref(), lang);
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    std::fs::write(out, html).with_context(|| format!("cannot write {}", out.display()))?;
    println!("Card written: {}", out.display());
    Ok(())
}

/// Invented earlier weeks for the demo card: the sick note Pip sent two weeks ago (see the demo history
/// "Sent the sick note …"), with its playbook, the yes on the card, the kept deadline and its reminder.
fn demo_history(mem: &Memory, now: DateTime<Local>) -> Result<()> {
    let sent_at = now - Duration::days(14);
    mem.playbook_upsert(&Playbook {
        site: "fixtures/demo-kasse".into(),
        task: "send_au".into(),
        url: String::new(),
        fields: vec![("Name".into(), "full_name".into()), ("Insurance number".into(), "versichertennummer".into())],
        attach_label: Some("Attach sick note".into()),
        send_label: Some("Send".into()),
        steps: 6,
        seconds: 41,
        runs: 2,
        failed_since_ok: 0,
        updated_at: sent_at,
    })?;
    for at in [sent_at - Duration::days(21), sent_at] {
        mem.log_action(&ActionLog {
            at,
            tool: "click".into(),
            args_redacted: "{\"description\":\"Send\"}".into(),
            risk: "commit".into(),
            approved: Some(true),
            result: "clicked Send".into(),
            tokens: 0,
        })?;
    }
    let id = mem.add_deadline(&NewDeadline {
        title: "Send the sick note to the insurer".into(),
        due: (sent_at + Duration::days(2)).date_naive(),
        rule_id: None,
        doc_id: None,
        remind_at: vec![sent_at - Duration::hours(2)],
        why: "Invented demo history".into(),
        source: "demo".into(),
    })?;
    if let Some(r) = mem.due_reminders(sent_at)?.into_iter().find(|r| r.deadline.id == id) {
        mem.mark_fired(r.id, r.at, Some("done"))?;
    }
    mem.set_deadline_status(id, DeadlineStatus::Done)?;
    if mem.profile_get(PROFILE_SECONDS_SAVED)?.is_none() {
        mem.profile_set(PROFILE_SECONDS_SAVED, "1500")?;
    }
    Ok(())
}
