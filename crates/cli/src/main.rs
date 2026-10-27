//! pip-cli: developer tool, no overlay. Uses the real Meta API (key from `.env`) and the fake
//! platform (a PNG as the screen), so every command runs the same on Linux and Windows.
//!
//! Examples:
//!   pip-cli read fixtures/letters/au_erika_erst_foto.jpg
//!   pip-cli sam fixtures/screens/insurer_form.png "button" --out target/cli/sam.png
//!   pip-cli ground fixtures/screens/insurer_form.png "Knopf Absenden" --query button
//!   pip-cli rules --now 2026-10-29T18:00
//!   pip-cli agent "Schick die Krankschreibung an Evergreen Health"
//!   pip-cli agent "Send my sick note" --screen fixtures/screens/demo-insurer.png \
//!       --outline fixtures/screens/demo-insurer.outline.json
//!   pip-cli --lang en agent "…" --page file://…/fixtures/demo-uni/index.html --answer 7654321
//!   pip-cli transcribe sample.wav
//!   pip-cli --lang en agent "@task_look_screen" --memory cold --answer "name => Erika Beispiel"
//!   EVAL_MODEL=muse-spark-1.3 pip-cli eval [--real] [--set quick] [--only demo-uni-v2,account-check] [--jobs 4]
//!   pip-cli card --demo --out target/pip-did-this.html

mod agent_cmd;
mod card;
mod eval;
mod expected;
mod letters;
mod screen;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use clap::{Parser, Subcommand, ValueEnum};
use pip_core::clock::{Clock, DemoClock, FixedClock, clock_from_env};
use pip_core::config::Config;
use pip_core::i18n::Lang;
use pip_core::memory::Memory;
use pip_core::meta::MetaClient;

#[derive(Parser, Debug)]
#[command(name = "pip-cli", version, about = "Papierkram-Copilot developer CLI (real Meta API, fake screen)")]
struct Cli {
    /// SQLite file (default: a fresh in-memory database)
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    /// Language of Pip's sentences: de or en
    #[arg(long, global = true, default_value = "de")]
    lang: String,
    /// Clock for rules and the agent, e.g. 2026-10-29T18:00 (default: DEMO_NOW or real time)
    #[arg(long, global = true)]
    now: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Read a letter (photo, PNG or PDF) like a drop on Pip: crop, read, save, apply the rules.
    Read {
        file: PathBuf,
        /// Also save the SAM 3.1 sheet crop of a photo here (one extra SAM call)
        #[arg(long)]
        crop: Option<PathBuf>,
    },
    /// SAM 3.1 boxes for a short noun phrase on an image; writes an annotated PNG.
    Sam {
        image: PathBuf,
        query: String,
        /// Annotated PNG (default: <image name>.sam.png in the current folder)
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Full grounding (SAM 3.1 + Muse Spark point and pick) for a described target on a screenshot.
    Ground {
        image: PathBuf,
        /// What to find, in words ("Knopf Absenden unten rechts")
        description: String,
        /// SAM noun phrase
        #[arg(long, default_value = "button")]
        query: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Deadlines and the reminders that fire at --now (no API call). Letters come from *.expected.json.
    Rules {
        /// Letter fixtures (*.expected.json). Default: the demo set in fixtures/letters
        #[arg(long = "letter")]
        letters: Vec<PathBuf>,
    },
    /// Dry run of the agent on screenshot files: the model plans, SAM grounds, nothing is really clicked.
    Agent {
        /// What the user asks. "@task_look_screen" = the text of that i18n key (the app's "Look at my screen").
        task: String,
        /// Screens in order; the next one is shown after the agent typed or a card was approved
        /// (default: the empty, then the filled demo form)
        #[arg(long = "screen")]
        screens: Vec<PathBuf>,
        /// The page open in the browser (default: the local demo form, file://…/fixtures/demo-insurer/index.html).
        /// Pip's memory always knows the demo form as the insurer's contact form, whatever is open.
        #[arg(long)]
        page: Option<String>,
        /// The sick note the user has given Pip (*.expected.json, no API call)
        #[arg(long)]
        letter: Option<PathBuf>,
        /// Which approval cards the dry run answers with "Ja"
        #[arg(long, value_enum, default_value = "attach")]
        approve: Approve,
        #[arg(long, default_value_t = 10)]
        max_steps: u32,
        /// PAGE ELEMENTS as Windows would read them (*.outline.json); the agent then acts by number. Typing and
        /// ticking change it like a real form. Without it: the picture only (like Linux).
        #[arg(long)]
        outline: Option<PathBuf>,
        /// Answer to Pip's question, repeatable (in order). A button's label picks that button; any other text
        /// answers the next open question (a question with only buttons gets its first button and keeps the
        /// answer). Without (or after the last one) the first button is pressed.
        #[arg(long = "answer")]
        answers: Vec<String>,
        /// What Pip starts with: full (letter + profile facts), no_letters, no_profile, or cold (neither).
        /// no_playbook is the same as full here (a fresh --db has no playbook).
        #[arg(long, value_enum, default_value = "full")]
        memory: agent_cmd::MemoryCond,
        /// Named target (from <screen>.boxes.json) whose click shows the next screen, repeatable
        /// (a cookie button, "Next", a submit that leads to an error screen).
        #[arg(long = "advance-on-click")]
        advance_on_click: Vec<String>,
    },
    /// "Pip did this for me": print the numbers and the post text, write the shareable card (no API call).
    Card {
        /// The HTML page to write
        #[arg(long, default_value = "target/pip-did-this.html")]
        out: PathBuf,
        /// Seed the invented demo person first (needs fixtures/letters), so the card has numbers
        #[arg(long)]
        demo: bool,
    },
    /// Speech to text for a WAV file (Muse Voice Transcribe).
    Transcribe { wav: PathBuf },
    /// Run eval/tasks.json (real API, a few cents) and write a markdown table.
    Eval {
        #[arg(long, default_value = "eval/tasks.json")]
        tasks: PathBuf,
        /// Results file. Default: eval/results.md for a full run; `--set quick` or `--only` write
        /// target/eval-<set-or-only>.md (never eval/results.md).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Also run the real read-only websites of eval/real-sites.json (dry run up to the send card, DEMO_MODE off
        /// for those tasks only, every card refused: nothing is ever sent).
        #[arg(long)]
        real: bool,
        /// Level: `full` (everything) or `quick` (2 letters, no click targets, the tasks with "quick": true on the
        /// full memory only, a few minutes).
        #[arg(long, value_enum, default_value_t = eval::EvalSet::Full)]
        set: eval::EvalSet,
        /// Only the agent tasks whose page or task text contains one of these, comma-separated (skips letters and
        /// click targets; keeps all memory conditions, with --set quick only `full`).
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        /// How many agent units (one task x one memory condition), letters or click targets run at the same time.
        #[arg(long, default_value_t = 4)]
        jobs: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Approve {
    /// Answer every card with "Noch nicht"
    None,
    /// "Ja" for attaching a file, "Noch nicht" for sending (default: shows the gate at Absenden)
    Attach,
    /// "Ja" for everything (still nothing is sent: the screen is a picture)
    All,
}

/// Shared setup for the commands.
pub struct Ctx {
    pub config: Config,
    pub memory: Arc<Memory>,
    pub lang: Lang,
    /// The time from --now / DEMO_NOW / the system.
    pub now: DateTime<Local>,
    pub clock: Arc<dyn Clock>,
    /// One client for the whole run, so token counts add up (created on first use).
    pub meta: Arc<std::sync::Mutex<Option<Arc<MetaClient>>>>,
}

impl Ctx {
    pub fn meta(&self) -> Result<Arc<MetaClient>> {
        let mut slot = self.meta.lock().expect("meta lock");
        if let Some(m) = slot.as_ref() {
            return Ok(m.clone());
        }
        let m = Arc::new(MetaClient::from_env().context("MODEL_API_KEY missing: put it in .env")?);
        *slot = Some(m.clone());
        Ok(m)
    }
    /// A path in fixtures/ (FIXTURES_DIR, fixtures next to the exe, or the repo's fixtures).
    pub fn fixture(&self, rel: &str) -> PathBuf {
        self.config.fixtures_dir.join(rel)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into())
                .add_directive("enigo=warn".parse().expect("static directive")),
        )
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let config = Config::from_env()?;
    let memory = Arc::new(match &cli.db {
        Some(p) => Memory::open(p).with_context(|| format!("cannot open database {}", p.display()))?,
        None => Memory::open_in_memory()?,
    });
    let clock: Arc<dyn Clock> = match &cli.now {
        Some(s) => Arc::new(FixedClock::new(DemoClock::parse(s)?.now())),
        None => clock_from_env(),
    };
    let ctx =
        Ctx { config, memory, lang: Lang::from_code(&cli.lang), now: clock.now(), clock, meta: Default::default() };
    match cli.cmd {
        Cmd::Read { file, crop } => letters::read(&ctx, &file, crop.as_deref()).await,
        Cmd::Sam { image, query, out } => {
            let out = out.unwrap_or_else(|| default_out(&image, "sam"));
            screen::sam(&ctx, &image, &query, &out).await
        }
        Cmd::Ground { image, description, query, out } => {
            let out = out.unwrap_or_else(|| default_out(&image, "ground"));
            screen::ground(&ctx, &image, &description, &query, &out).await
        }
        Cmd::Rules { letters } => letters::rules(&ctx, &letters),
        Cmd::Agent { task, screens, page, letter, approve, max_steps, outline, answers, memory, advance_on_click } => {
            // "@task_look_screen" = the app's own button text for that i18n key (same rule as eval/tasks.json).
            let task = match task.strip_prefix('@') {
                Some(key) => pip_core::i18n::t(key, ctx.lang).to_string(),
                None => task,
            };
            let (seed_letter, seed_facts) = memory.seeds();
            let opts = agent_cmd::Options {
                task,
                screens,
                page,
                letter,
                approve,
                max_steps,
                quiet: false,
                outline,
                answers,
                seed_letter,
                seed_facts,
                advance_on_click,
                documents: Vec::new(),
            };
            agent_cmd::run(&ctx, opts).await.map(|_| ())
        }
        Cmd::Transcribe { wav } => {
            let bytes = std::fs::read(&wav).with_context(|| format!("cannot read {}", wav.display()))?;
            let started = std::time::Instant::now();
            let text = ctx.meta()?.transcribe(bytes, ctx.lang).await?;
            println!("{text}");
            eprintln!("({:.1} s)", started.elapsed().as_secs_f32());
            Ok(())
        }
        Cmd::Eval { tasks, out, real, set, only, jobs } => {
            let only: Vec<String> = only.into_iter().map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect();
            let out = out.unwrap_or_else(|| eval::default_out(set, &only));
            eval::run(&ctx, &tasks, &out, real, set, &only, jobs).await
        }
        Cmd::Card { out, demo } => card::run(&ctx, &out, demo),
    }
}

/// "<stem>.<what>.png" in the current folder.
fn default_out(image: &Path, what: &str) -> PathBuf {
    let stem = image.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    PathBuf::from(format!("{stem}.{what}.png"))
}
