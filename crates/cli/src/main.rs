//! copilot-cli: developer tool, no overlay. Uses the real Meta API (key from `.env`) and the fake
//! platform (a PNG as the screen), so every command runs the same on Linux and Windows.
//!
//! Examples:
//!   copilot-cli read fixtures/letters/au_erika_erst_foto.jpg
//!   copilot-cli sam fixtures/screens/kasse_form.png "button" --out target/cli/sam.png
//!   copilot-cli ground fixtures/screens/kasse_form.png "Knopf Absenden" --query button
//!   copilot-cli rules --now 2026-10-29T18:00
//!   copilot-cli agent "Schick die Krankschreibung an die Gesundkasse"
//!   copilot-cli agent "Send my sick note" --screen fixtures/screens/demo-kasse.png \
//!       --outline fixtures/screens/demo-kasse.outline.json
//!   copilot-cli transcribe sample.wav
//!   copilot-cli eval

mod agent_cmd;
mod eval;
mod expected;
mod letters;
mod screen;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use clap::{Parser, Subcommand, ValueEnum};
use copilot_core::clock::{Clock, DemoClock, FixedClock, clock_from_env};
use copilot_core::config::Config;
use copilot_core::i18n::Lang;
use copilot_core::memory::Memory;
use copilot_core::meta::MetaClient;

#[derive(Parser, Debug)]
#[command(name = "copilot-cli", version, about = "Papierkram-Copilot developer CLI (real Meta API, fake screen)")]
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
        task: String,
        /// Screens in order; the next one is shown after the agent typed or a card was approved
        /// (default: the empty, then the filled demo form)
        #[arg(long = "screen")]
        screens: Vec<PathBuf>,
        /// The page open in the browser (default: the local demo form, file://…/fixtures/demo-kasse/index.html)
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
    },
    /// Speech to text for a WAV file (Muse Voice Transcribe).
    Transcribe { wav: PathBuf },
    /// Run eval/tasks.json (real API, a few cents) and write a markdown table.
    Eval {
        #[arg(long, default_value = "eval/tasks.json")]
        tasks: PathBuf,
        #[arg(long, default_value = "eval/results.md")]
        out: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
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
        Cmd::Agent { task, screens, page, letter, approve, max_steps, outline } => {
            let opts = agent_cmd::Options { task, screens, page, letter, approve, max_steps, quiet: false, outline };
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
        Cmd::Eval { tasks, out } => eval::run(&ctx, &tasks, &out).await,
    }
}

/// "<stem>.<what>.png" in the current folder.
fn default_out(image: &Path, what: &str) -> PathBuf {
    let stem = image.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    PathBuf::from(format!("{stem}.{what}.png"))
}
