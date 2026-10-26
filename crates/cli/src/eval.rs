//! `eval`: the 10 tasks of SPEC §13.3 with invented data (real API, a few cents).
//! Letters: are the fields and the deadlines right? Screens: does the click land on the target?
//! Agent: does the approval gate stop the click on "Absenden"?

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use chrono::{Local, TimeZone};
use pip_core::clock::{Clock, FixedClock};
use pip_core::docs::DocPipeline;
use pip_core::memory::Memory;
use pip_core::meta::Usage;
use pip_core::rules::RuleEngine;
use serde::Deserialize;
use serde_json::Value;

use crate::agent_cmd::{self, Options};
use crate::{Approve, Ctx, expected, screen};

#[derive(Debug, Deserialize)]
struct Tasks {
    letters: Vec<String>,
    ground: Vec<GroundTask>,
    agent: Vec<AgentTask>,
}

#[derive(Debug, Deserialize)]
struct GroundTask {
    screen: String,
    target: String,
}

#[derive(Debug, Deserialize)]
struct AgentTask {
    task: String,
    screens: Vec<String>,
}

fn tokens(u: Usage) -> u64 {
    u.input_tokens + u.output_tokens
}

/// A fresh, empty memory per task so tasks do not influence each other.
fn fresh(ctx: &Ctx) -> Result<Ctx> {
    let now = ctx.now;
    let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(now));
    Ok(Ctx {
        config: ctx.config.clone(),
        memory: Arc::new(Memory::open_in_memory()?),
        lang: ctx.lang,
        now,
        clock,
        meta: ctx.meta.clone(),
    })
}

pub async fn run(ctx: &Ctx, tasks_path: &Path, out: &Path) -> Result<()> {
    let tasks: Tasks = serde_json::from_str(
        &std::fs::read_to_string(tasks_path).with_context(|| format!("cannot read {}", tasks_path.display()))?,
    )
    .with_context(|| format!("{} has the wrong format", tasks_path.display()))?;
    // The fixtures' expected deadlines were computed with this clock (fixtures/README.md).
    let base = Ctx {
        config: ctx.config.clone(),
        memory: ctx.memory.clone(),
        lang: ctx.lang,
        now: Local.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).single().unwrap_or(ctx.now),
        clock: ctx.clock.clone(),
        meta: ctx.meta.clone(),
    };
    let meta = ctx.meta()?;
    let mut md = String::from("# Eval results\n\n");
    let _ = writeln!(
        md,
        "Run: {} · `pip-cli eval` · invented data only · real Meta API\n",
        Local::now().format("%Y-%m-%d %H:%M")
    );

    // 1. Letters.
    md.push_str("## Letters (Muse Spark reads, SAM 3.1 crops photos, rules compute deadlines)\n\n");
    md.push_str("| Letter | Fields correct | Deadline correct | Time | Tokens |\n|---|---|---|---|---|\n");
    let (mut f_ok, mut f_all, mut d_ok) = (0, 0, 0);
    for rel in &tasks.letters {
        let c = fresh(&base)?;
        let path = c.fixture(rel);
        let rules = Arc::new(RuleEngine::new(c.memory.clone(), c.clock.clone()));
        let pipeline = DocPipeline::new(meta.clone(), c.memory.clone(), rules);
        let before = tokens(meta.usage_total());
        let started = Instant::now();
        let row = match pipeline.ingest(&path, c.lang).await {
            Ok(r) => {
                let x = expected::load(&expected::path_for(&path).context("expected.json missing")?)?;
                let checks = expected::check(&x, &r.extract, &r.deadlines);
                let ok = checks.iter().filter(|c| c.ok).count();
                let dl = checks.iter().filter(|c| c.name.ends_with("deadline")).all(|c| c.ok);
                f_ok += ok;
                f_all += checks.len();
                d_ok += usize::from(dl);
                let wrong: Vec<String> =
                    checks.iter().filter(|c| !c.ok).map(|c| format!("{}={}", c.name, c.got)).collect();
                let note = if wrong.is_empty() { String::new() } else { format!(" ({})", wrong.join("; ")) };
                format!("{ok}/{}{note} | {} |", checks.len(), if dl { "yes" } else { "**no**" })
            }
            Err(e) => {
                f_all += 1;
                format!("failed: {e} | **no** |")
            }
        };
        let secs = started.elapsed().as_secs_f32();
        let used = tokens(meta.usage_total()) - before;
        println!("letter {rel}: {row} {secs:.1}s");
        let _ = writeln!(md, "| `{rel}` | {row} {secs:.1} s | {used} |");
    }

    // 2. Grounding.
    md.push_str("\n## Screen targets (SAM 3.1 + Muse Spark point, on 1600x1000 screenshots)\n\n");
    md.push_str("| Screen | Target | Hit | Click | Time | Tokens |\n|---|---|---|---|---|---|\n");
    let mut g_ok = 0;
    for g in &tasks.ground {
        let c = fresh(&base)?;
        let shot = c.fixture(&g.screen);
        let stem = shot.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        let boxes: Value = expected::load(&shot.with_file_name(format!("{stem}.boxes.json")))?;
        let t = &boxes["targets"][&g.target];
        let (desc, query) =
            (t["description"].as_str().unwrap_or(&g.target), t["sam_query"].as_str().unwrap_or("button"));
        let b: Vec<i64> = t["box"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect();
        let before = tokens(meta.usage_total());
        let started = Instant::now();
        let p = screen::ground_point(&c, &shot, desc, query).await?;
        let secs = started.elapsed().as_secs_f32();
        let used = tokens(meta.usage_total()) - before;
        let hit = match (p, b.as_slice()) {
            (Some(p), [x1, y1, x2, y2]) => {
                (*x1..=*x2).contains(&i64::from(p.x)) && (*y1..=*y2).contains(&i64::from(p.y))
            }
            _ => false,
        };
        g_ok += usize::from(hit);
        let click = p.map(|p| format!("({}, {})", p.x, p.y)).unwrap_or_else(|| "none".into());
        println!("ground {} {}: hit={hit} {click} {secs:.1}s", g.screen, g.target);
        let _ = writeln!(
            md,
            "| `{}` | {desc} | {} | {click} | {secs:.1} s | {used} |",
            g.screen,
            if hit { "yes" } else { "**no**" }
        );
    }

    // 3. Agent runs (dry run on screenshots).
    md.push_str("\n## Agent with approval gate (dry run on screenshots, nothing is clicked for real)\n\n");
    md.push_str("| Task | Gate stopped \"Absenden\" | Cards | Steps | Time | Tokens |\n|---|---|---|---|---|---|\n");
    let mut a_ok = 0;
    for a in &tasks.agent {
        let c = fresh(&base)?;
        let opts = Options {
            task: a.task.clone(),
            screens: a.screens.iter().map(|s| c.fixture(s)).collect(),
            page: None,
            letter: None,
            approve: Approve::Attach,
            max_steps: 10,
            quiet: true,
            outline: None,
        };
        let r = agent_cmd::run(&c, opts).await?;
        let held = r.gate_held_at_submit();
        a_ok += usize::from(held);
        let cards: Vec<String> = r
            .cards
            .iter()
            .map(|c| format!("{} {} {} → {}", c.title, c.what, c.to, if c.approved { "Ja" } else { "Noch nicht" }))
            .collect();
        println!("agent \"{}\": gate={held} steps={} {:.0}s", a.task, r.steps, r.secs);
        let _ = writeln!(
            md,
            "| {} | {} | {} | {} | {:.0} s | {} |",
            a.task,
            if held { "yes" } else { "**no**" },
            cards.join(", "),
            r.steps,
            r.secs,
            r.tokens
        );
    }

    let total = tokens(meta.usage_total());
    let summary = format!(
        "\n## Summary\n\n- Letter fields: {f_ok}/{f_all} ({:.0} %); deadlines right in {d_ok}/{} letters\n\
         - Click targets hit: {g_ok}/{}\n- Gate held at \"Absenden\": {a_ok}/{}\n- Tokens for the whole run: {total}\n",
        100.0 * f_ok as f32 / f_all.max(1) as f32,
        tasks.letters.len(),
        tasks.ground.len(),
        tasks.agent.len(),
    );
    md.push_str(&summary);
    print!("{summary}");
    crate::letters::write_file(out, md.as_bytes())?;
    println!("Written: {}", out.display());
    Ok(())
}
