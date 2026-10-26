//! `read` (one letter through the real pipeline) and `rules` (deadlines + reminders at a given time).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local};
use pip_core::clock::{Clock, FixedClock};
use pip_core::docs::{DocError, DocPipeline, DocumentExtract};
use pip_core::events::UiEvent;
use pip_core::memory::{Deadline, Memory};
use pip_core::meta::{MetaClient, OpenWeightsClient};
use pip_core::rules::{RuleEngine, local_at};

use crate::{Ctx, expected};

/// The demo letters `rules` loads when none are given.
pub const DEMO_SET: &[&str] = &[
    "au_erika_erst.expected.json",
    "kasse_brief.expected.json",
    "bescheid_wohngeld.expected.json",
    "uni_rueckmeldung.expected.json",
];

pub async fn read(ctx: &Ctx, file: &Path, crop: Option<&Path>) -> Result<()> {
    // Private mode (PRIVATE_MODEL_BASE): a Meta open-weights model reads the letter; the cloud
    // client is never called for it, so a missing MODEL_API_KEY does not matter there.
    let private = OpenWeightsClient::from_config(&ctx.config).map(Arc::new);
    let meta = match (ctx.meta(), &private) {
        (Ok(m), _) => m,
        (Err(_), Some(_)) => Arc::new(MetaClient::new("http://127.0.0.1:9", "unused")),
        (Err(e), None) => return Err(e),
    };
    let rules = Arc::new(RuleEngine::new(ctx.memory.clone(), ctx.clock.clone()));
    let mut pipeline = DocPipeline::new(meta.clone(), ctx.memory.clone(), rules);
    if let Some(p) = &private {
        println!(
            "Private mode: reading with {} at {} (Meta open weights; the letter stays local, SAM crop skipped)",
            p.model(),
            p.base()
        );
        pipeline = pipeline.with_private(p.clone());
    }
    if let Some(out) = crop.filter(|_| private.is_some()) {
        println!("--crop skipped: in private mode the photo is not sent to SAM ({}).", out.display());
    } else if let Some(out) = crop {
        let bytes = std::fs::read(file).with_context(|| format!("cannot read {}", file.display()))?;
        let png = pipeline.crop_sheet(&bytes).await?;
        write_file(out, &png)?;
        println!("SAM 3.1 sheet crop saved: {}", out.display());
    }
    let started = Instant::now();
    let r = match pipeline.ingest(file, ctx.lang).await {
        Ok(r) => r,
        Err(e) => {
            if let Some(d) = e.downcast_ref::<DocError>() {
                println!("Pip says: {}", d.user_text(ctx.lang));
            }
            return Err(e);
        }
    };
    let secs = started.elapsed().as_secs_f32();
    println!("== {} ({secs:.1} s{})", file.display(), if r.duplicate { ", already known" } else { "" });
    println!("Pip says: {}", r.bubble);
    println!("\nDocumentExtract:\n{}", serde_json::to_string_pretty(&r.extract)?);
    print_deadlines(&r.deadlines);
    if let Some(xp) = expected::path_for(file) {
        let checks = expected::check(&expected::load(&xp)?, &r.extract, &r.deadlines);
        let ok = checks.iter().filter(|c| c.ok).count();
        println!("\nCheck against {}: {ok}/{} fields", xp.display(), checks.len());
        for c in &checks {
            println!("  [{}] {}: {}", if c.ok { "ok" } else { "XX" }, c.name, c.got);
        }
    }
    if r.private {
        println!("\nTokens: n/a (private mode)");
    } else {
        let u = meta.usage_total();
        println!("\nTokens: {} in, {} out", u.input_tokens, u.output_tokens);
    }
    Ok(())
}

pub fn print_deadlines(deadlines: &[Deadline]) {
    if deadlines.is_empty() {
        println!("\nDeadlines: none");
        return;
    }
    println!("\nDeadlines:");
    for d in deadlines {
        let times: Vec<String> = d.remind_at.iter().map(|t| t.format("%a %d.%m. %H:%M").to_string()).collect();
        println!(
            "  {} [{}] {} | reminders: {}",
            d.due.format("%a %d.%m.%Y"),
            d.rule_id.as_deref().unwrap_or("-"),
            d.title,
            if times.is_empty() { "-".into() } else { times.join(", ") }
        );
        if !d.why.is_empty() {
            println!("      why: {}", d.why);
        }
    }
}

/// Store a fixture letter without the API: the extract comes from its expected.json.
/// The letter counts as read the day after its date at 12:00 (never after `latest`).
pub fn seed_letter(
    memory: &Arc<Memory>,
    rules: &RuleEngine,
    clock: &FixedClock,
    expected_path: &Path,
    latest: DateTime<Local>,
) -> Result<(i64, DocumentExtract, Vec<Deadline>)> {
    let x = expected::load(expected_path)?;
    let extract = expected::to_extract(&x)?;
    let read_at = extract.date.map(|d| local_at(d, 12, 0) + Duration::days(1)).unwrap_or(latest).min(latest);
    clock.set(read_at);
    let file = expected::letter_file(expected_path);
    let sha = format!("fixture:{}", file.file_name().and_then(|s| s.to_str()).unwrap_or_default());
    let id = memory.insert_document(&file, &sha, &extract, &extract.summary_simple)?;
    let deadlines = rules.apply_document(id, &extract)?;
    Ok((id, extract, deadlines))
}

pub fn rules(ctx: &Ctx, letters: &[PathBuf]) -> Result<()> {
    let clock = Arc::new(FixedClock::new(ctx.now));
    let engine = RuleEngine::new(ctx.memory.clone(), clock.clone() as Arc<dyn Clock>);
    let letters: Vec<PathBuf> = if letters.is_empty() {
        DEMO_SET.iter().map(|f| ctx.fixture(&format!("letters/{f}"))).collect()
    } else {
        letters.to_vec()
    };
    println!("Letters (from expected.json, no API call):");
    for p in &letters {
        let (id, x, _) = seed_letter(&ctx.memory, &engine, &clock, p, ctx.now)?;
        println!(
            "  #{id} {} {} from {} (read {})",
            x.doc_type.as_str(),
            x.date.map(|d| d.to_string()).unwrap_or_default(),
            x.sender,
            clock.now().format("%d.%m. %H:%M")
        );
    }
    clock.set(ctx.now);
    print_deadlines(&ctx.memory.open_deadlines()?);
    println!("\nNow: {}", ctx.now.format("%A %d.%m.%Y %H:%M"));
    let fired = engine.tick()?;
    if fired.is_empty() {
        println!("No reminder is due now.");
    }
    for r in &fired {
        if let UiEvent::Reminder { text, why, buttons, .. } = engine.reminder_event(r, ctx.lang) {
            println!("\nPip reminds (for \"{}\", due {}):", r.deadline.title, r.deadline.due);
            println!("  {text}");
            let labels: Vec<String> = buttons.iter().map(|b| format!("[{}]", b.label)).collect();
            println!("  {}", labels.join(" "));
            println!("  Warum? {why}");
        }
    }
    let again = engine.tick()?;
    println!("\nSecond tick at the same time fires {} reminder(s) (never twice).", again.len());
    Ok(())
}

pub fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}
