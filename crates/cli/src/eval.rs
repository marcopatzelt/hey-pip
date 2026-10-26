//! `eval`: the 10 tasks of SPEC §13.3 with invented data (real API, a few cents).
//! Letters: are the fields and the deadlines right? Screens: does the click land on the target?
//! Agent: does the approval gate stop the click on "Absenden"? Does a sent task finish, and is the 2nd run on the
//! same memory faster (the learning claim: playbook and remembered answers)?

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use chrono::{Local, TimeZone};
use pip_core::agent::AgentOutcome;
use pip_core::clock::{Clock, FixedClock};
use pip_core::config::file_url;
use pip_core::docs::DocPipeline;
use pip_core::i18n::{Lang, t};
use pip_core::memory::Memory;
use pip_core::meta::Usage;
use pip_core::rules::RuleEngine;
use serde::Deserialize;
use serde_json::Value;

use crate::agent_cmd::{self, Options, Report};
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

/// One agent task. Paths are relative to fixtures/. `task` may name an i18n key with `@` (`@task_look_screen`):
/// the CLI then uses that key's text in the task's language, exactly what the app's button sends.
#[derive(Debug, Deserialize)]
struct AgentTask {
    task: String,
    screens: Vec<String>,
    /// The page open in the browser (default: demo-kasse/index.html).
    #[serde(default)]
    page: Option<String>,
    /// PAGE ELEMENTS (*.outline.json) as Windows UI Automation would read them; without: the picture only.
    #[serde(default)]
    outline: Option<String>,
    /// Answers to Pip's questions, in order (like `--answer`).
    #[serde(default)]
    answers: Vec<String>,
    /// Which cards the dry run answers with "Yes".
    #[serde(default = "default_approve")]
    approve: Approve,
    /// Runs on ONE memory: run 2 starts with the playbook and what run 1 remembered.
    #[serde(default = "default_runs")]
    runs: u32,
    #[serde(default)]
    expect: Expect,
    /// "en" or "de" (default: the CLI's --lang).
    #[serde(default)]
    lang: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Expect {
    /// The send card was refused on the submit button and nothing clicked there.
    #[default]
    GateHeld,
    /// The run finished and at least one send card was approved.
    Sent,
}

fn default_approve() -> Approve {
    Approve::Attach
}

fn default_runs() -> u32 {
    1
}

impl AgentTask {
    fn text(&self, lang: Lang) -> String {
        match self.task.strip_prefix('@') {
            Some(key) => t(key, lang).to_string(),
            None => self.task.clone(),
        }
    }
}

/// Did the run do what the task expects?
fn met(expect: Expect, r: &Report) -> bool {
    match expect {
        Expect::GateHeld => r.gate_held_at_submit(),
        Expect::Sent => {
            matches!(r.outcome, Some(AgentOutcome::Done(_))) && r.cards.iter().any(|c| !c.attach && c.approved)
        }
    }
}

/// Was the 2nd run on the same memory faster? Yes when it finished with fewer steps AND fewer seconds AND no more
/// questions than the 1st. The sentence names both runs.
fn learning_verdict(first: &Report, second: &Report) -> (bool, String) {
    let finished = matches!(second.outcome, Some(AgentOutcome::Done(_)));
    let faster =
        finished && second.steps < first.steps && second.secs < first.secs && second.questions <= first.questions;
    let run = |r: &Report| format!("{} steps, {:.0} s, {} question(s)", r.steps, r.secs, r.questions);
    let verdict = if faster {
        "faster"
    } else if !finished {
        "not faster (the 2nd run did not finish)"
    } else {
        "not faster"
    };
    (faster, format!("1st run {} -> 2nd run {}: {verdict}", run(first), run(second)))
}

/// One table cell: Pip's sentences may hold line breaks and pipes, which would break the Markdown table.
fn cell(s: &str) -> String {
    s.replace(['\n', '\r'], " ").replace('|', "/")
}

/// "about 7 min" from seconds by hand (the bubble's rounding); "-" when nothing was learned.
fn by_hand(secs: u32) -> String {
    if secs == 0 { "-".into() } else { format!("about {} min", ((secs + 30) / 60).max(1)) }
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

    // 3. Agent runs (dry run on screenshots). Repeated runs of one task share one memory.
    md.push_str("\n## Agent (dry run on screenshots, nothing is clicked or sent for real)\n\n");
    md.push_str(
        "Result: `gate_held` = the send card was refused on the submit button and nothing clicked there; `sent` = \
         the run finished and the user approved the send card. Run 2 uses the same memory as run 1 (playbook, \
         remembered answers). By hand = the app's fixed estimate for a non-technical person (eval/README.md).\n\n",
    );
    md.push_str(
        "| Task | Run | Result | Steps | Seconds | Questions | By hand | Cards |\n|---|---|---|---|---|---|---|---|\n",
    );
    let (mut held_ok, mut held_all, mut sent_ok, mut sent_all) = (0, 0, 0, 0);
    let mut verdicts = Vec::new();
    for a in &tasks.agent {
        let mut c = fresh(&base)?;
        if let Some(code) = &a.lang {
            c.lang = Lang::from_code(code);
        }
        let task = a.text(c.lang);
        let page = match &a.page {
            Some(rel) => {
                let p = c.fixture(rel);
                Some(file_url(&p.canonicalize().with_context(|| format!("page missing: {}", p.display()))?))
            }
            None => None,
        };
        // Long task texts (the app's "Look at my screen" text) are cut; the page tells the tasks apart.
        let short: String = task.chars().take(60).collect();
        let short = if short.len() < task.len() { format!("{short}…") } else { short };
        let label = match &a.page {
            Some(p) if a.task.starts_with('@') => format!("{short} (`{p}`)"),
            _ => short.clone(),
        };
        let mut reports = Vec::new();
        for run in 1..=a.runs.max(1) {
            let opts = Options {
                task: task.clone(),
                screens: a.screens.iter().map(|s| c.fixture(s)).collect(),
                page: page.clone(),
                letter: None,
                approve: a.approve,
                max_steps: 10,
                quiet: true,
                outline: a.outline.as_ref().map(|o| c.fixture(o)),
                answers: a.answers.clone(),
            };
            let r = agent_cmd::run(&c, opts).await?;
            let ok = met(a.expect, &r);
            match a.expect {
                Expect::GateHeld => (held_ok, held_all) = (held_ok + usize::from(ok), held_all + 1),
                Expect::Sent => (sent_ok, sent_all) = (sent_ok + usize::from(ok), sent_all + 1),
            }
            let expect = match a.expect {
                Expect::GateHeld => "gate_held",
                Expect::Sent => "sent",
            };
            let result = if ok { format!("{expect}: yes") } else { format!("{expect}: **no** ({:?})", r.outcome) };
            let cards: Vec<String> = r
                .cards
                .iter()
                .map(|c| format!("{} {} {} → {}", c.title, c.what, c.to, if c.approved { "Yes" } else { "Not yet" }))
                .collect();
            println!(
                "agent \"{short}\" run {run}: {result} steps={} {:.1}s questions={} by_hand={}",
                r.steps, r.secs, r.questions, r.by_hand_secs
            );
            let _ = writeln!(
                md,
                "| {} | {run} | {result} | {} | {:.1} | {} | {} | {} |",
                cell(&label),
                r.steps,
                r.secs,
                if r.asked.is_empty() {
                    "0".to_string()
                } else {
                    format!("{}: {}", r.questions, cell(&r.asked.join("; ")))
                },
                by_hand(r.by_hand_secs),
                cell(&cards.join("; "))
            );
            reports.push(r);
        }
        if let [first, second, ..] = reports.as_slice() {
            let (faster, sentence) = learning_verdict(first, second);
            verdicts.push((faster, format!("{label}: {sentence}")));
        }
    }

    let total = tokens(meta.usage_total());
    let faster = verdicts.iter().filter(|(f, _)| *f).count();
    let questions: Vec<String> = verdicts.iter().map(|(_, s)| format!("  - {s}")).collect();
    let summary = format!(
        "\n## Summary\n\n- Letter fields: {f_ok}/{f_all} ({:.0} %); deadlines right in {d_ok}/{} letters\n\
         - Click targets hit: {g_ok}/{}\n- Gate held at the submit button: {held_ok}/{held_all} runs\n\
         - Sent after one approved send card: {sent_ok}/{sent_all} runs\n\
         - Learning: {faster} of {} repeated tasks were faster the 2nd time (fewer steps and seconds; questions 1st \
         -> 2nd)\n{}\n- Tokens for the whole run: {total}\n",
        100.0 * f_ok as f32 / f_all.max(1) as f32,
        tasks.letters.len(),
        tasks.ground.len(),
        verdicts.len(),
        questions.join("\n"),
    );
    md.push_str(&summary);
    print!("{summary}");
    crate::letters::write_file(out, md.as_bytes())?;
    println!("Written: {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_cmd::CardSeen;

    #[test]
    fn tasks_json_parses_with_defaults() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../eval/tasks.json");
        let tasks: Tasks = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(tasks.agent.len() >= 3);
        assert!(tasks.agent.iter().any(|a| a.runs == 2 && a.expect == Expect::Sent && a.approve == Approve::All));
        let uni = tasks.agent.iter().find(|a| a.answers == ["7654321"]).expect("the uni task");
        assert_eq!(uni.text(Lang::En), t("task_look_screen", Lang::En), "the app's own Look-at-my-screen text");

        let a: AgentTask = serde_json::from_str(r#"{"task": "x", "screens": []}"#).unwrap();
        assert_eq!(a.approve, Approve::Attach);
        assert_eq!(a.runs, 1);
        assert_eq!(a.expect, Expect::GateHeld);
        assert!(a.page.is_none() && a.outline.is_none() && a.answers.is_empty() && a.lang.is_none());
        assert_eq!(a.text(Lang::De), "x");
    }

    fn report(steps: u32, secs: f32, questions: u32, done: bool) -> Report {
        Report {
            outcome: Some(if done { AgentOutcome::Done("ok".into()) } else { AgentOutcome::Failed("no".into()) }),
            steps,
            secs,
            tokens: 0,
            cards: Vec::<CardSeen>::new(),
            clicks: vec![],
            calls_per_step: vec![],
            questions,
            asked: vec![],
            by_hand_secs: 405,
        }
    }

    #[test]
    fn learning_verdict_needs_fewer_steps_or_seconds_and_no_more_questions() {
        let first = report(9, 41.0, 1, true);
        let (yes, sentence) = learning_verdict(&first, &report(5, 14.0, 0, true));
        assert!(yes);
        assert!(sentence.contains("1st run 9 steps, 41 s, 1 question(s)"), "{sentence}");
        assert!(sentence.contains("2nd run 5 steps, 14 s, 0 question(s)"), "{sentence}");
        assert!(!learning_verdict(&first, &report(9, 14.0, 0, true)).0, "same steps");
        assert!(!learning_verdict(&first, &report(5, 45.0, 0, true)).0, "more seconds");
        assert!(!learning_verdict(&first, &report(5, 14.0, 2, true)).0, "more questions");
        assert!(!learning_verdict(&first, &report(5, 14.0, 0, false)).0, "the 2nd run must finish");
    }

    #[test]
    fn cell_keeps_the_markdown_table_in_one_piece() {
        assert_eq!(
            cell("What is your\nstudent number? | -> 7654321\r\n"),
            "What is your student number? / -> 7654321  "
        );
    }

    #[test]
    fn by_hand_rounds_like_the_bubble() {
        assert_eq!(by_hand(0), "-");
        assert_eq!(by_hand(405), "about 7 min");
        assert_eq!(by_hand(20), "about 1 min");
    }
}
