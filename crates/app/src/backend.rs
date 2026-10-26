//! The core services the app talks to. Every piece is optional: if one cannot start
//! (no key, no database, a part of core not finished yet) the overlay still runs and
//! Pip says a friendly sentence instead. `PIP_MOCK=1` runs a scripted demo without core.

use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use chrono::{DateTime, Local};
use pip_core::agent::{Agent, Gate, PROFILE_LANG, PROFILE_SERVICES};
use pip_core::clock::{Clock, SystemClock, clock_from_env};
use pip_core::config::{Config, file_url};
use pip_core::docs::DocPipeline;
use pip_core::events::Bus;
use pip_core::i18n::Lang;
use pip_core::memory::{Memory, default_db_path, demo_seed_is_complete, seed_demo_profile};
use pip_core::meta::{MetaClient, OpenWeightsClient};
use pip_core::rules::RuleEngine;

pub struct Backend {
    pub mock: bool,
    pub memory: Option<Arc<Memory>>,
    pub meta: Option<Arc<MetaClient>>,
    pub rules: Option<Arc<RuleEngine>>,
    pub docs: Option<Arc<DocPipeline>>,
    pub agent: Option<Arc<Agent>>,
    /// The same clock the rules use (DEMO_NOW moves it), for "in 3 Tagen" in the deadline list.
    pub clock: Arc<dyn Clock>,
    /// Profile values when there is no database (mock mode or memory failed).
    profile_fallback: Mutex<HashMap<String, String>>,
}

/// Profile key with the demo insurer's contact form (file:// URL of fixtures/demo-insurer/index.html).
pub const DEMO_FORM_KEY: &str = "insurer_contact_form";

/// Profile key with the demo insurer's "Services for you" page (after-task discovery looks there).
pub const DEMO_SERVICES_KEY: &str = PROFILE_SERVICES;

/// The demo pages, built into the exe so a fresh download works without a `fixtures/` folder:
/// the invented insurer's form, its services page and the questionnaire behind it, and the invented
/// university's re-registration form (a second site: its own playbook and its own "Don't ask again").
/// Paths are relative to the fixtures folder.
const DEMO_PAGES: &[(&str, &str)] = &[
    ("demo-insurer/index.html", include_str!("../../../fixtures/demo-insurer/index.html")),
    ("demo-insurer/services.html", include_str!("../../../fixtures/demo-insurer/services.html")),
    ("demo-insurer/questionnaire.html", include_str!("../../../fixtures/demo-insurer/questionnaire.html")),
    ("demo-uni/index.html", include_str!("../../../fixtures/demo-uni/index.html")),
];

/// The invented letters of the demo profile (`PIP_DEMO_PROFILE=1`), written to `<data_dir>/demo-letters/`.
const DEMO_LETTERS: &[(&str, &[u8])] = &[
    ("bescheid_wohngeld.pdf", include_bytes!("../../../fixtures/letters/bescheid_wohngeld.pdf")),
    ("au_erika_erst_foto.jpg", include_bytes!("../../../fixtures/letters/au_erika_erst_foto.jpg")),
    ("insurer_letter.pdf", include_bytes!("../../../fixtures/letters/insurer_letter.pdf")),
];

/// The invented demo person (`seed_demo_profile`); PIP_DEMO_RESET wipes only her memory.
const DEMO_FULL_NAME: &str = "Erika Beispiel";

/// PIP_DEMO_RESET may wipe the memory only when it is the demo person's (`full_name` exactly DEMO_FULL_NAME)
/// or holds no name at all. Any other stored name may be a real person: never wiped.
fn demo_reset_allowed(full_name: Option<&str>, name: Option<&str>) -> bool {
    match full_name {
        Some(f) => f == DEMO_FULL_NAME,
        None => name.is_none(),
    }
}

pub fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| matches!(v.trim(), "1" | "true" | "yes" | "on"))
}

/// Run `f`; a panic in core becomes an error instead of killing the app.
pub fn guard<T>(what: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => Err(anyhow!("{what} panicked: {}", panic_text(&p))),
    }
}

/// Same for async work: runs on its own task so a panic only ends that task.
pub async fn guard_async<T, F>(what: &str, fut: F) -> Result<T>
where
    T: Send + 'static,
    F: Future<Output = Result<T>> + Send + 'static,
{
    match tokio::spawn(fut).await {
        Ok(r) => r,
        Err(e) if e.is_panic() => Err(anyhow!("{what} panicked: {}", panic_text(&e.into_panic()))),
        Err(e) => Err(anyhow!("{what} was cancelled: {e}")),
    }
}

fn panic_text(p: &Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

fn ok_or_log<T>(what: &str, r: Result<T>) -> Option<T> {
    r.map_err(|e| tracing::warn!("{what} not available: {e:#}")).ok()
}

impl Backend {
    pub fn mock() -> Self {
        Self {
            mock: true,
            memory: None,
            meta: None,
            rules: None,
            docs: None,
            agent: None,
            clock: Arc::new(SystemClock),
            profile_fallback: Mutex::default(),
        }
    }

    /// Start every core service that can start. Never fails.
    pub fn init(bus: &Bus) -> Self {
        if env_flag("PIP_MOCK") {
            tracing::info!("PIP_MOCK=1: scripted demo, no core services");
            return Self::mock();
        }
        let config = ok_or_log("config", guard("config", Config::from_env));
        // Without a config we stay on the safe side: commit actions only on demo pages.
        let demo_mode = config.as_ref().is_none_or(|c| c.demo_mode);
        let clock: Arc<dyn Clock> =
            ok_or_log("clock", guard("clock", || Ok(clock_from_env()))).unwrap_or_else(|| Arc::new(SystemClock));
        let db_path =
            config.as_ref().map(Config::db_path).or_else(|| ok_or_log("db path", guard("db path", default_db_path)));
        let memory = db_path
            .and_then(|p| {
                if let Some(dir) = p.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                ok_or_log("memory", guard("memory", || Memory::open(&p)))
            })
            .map(Arc::new);
        let meta = ok_or_log("meta client", guard("meta client", MetaClient::from_env)).map(Arc::new);
        let rules = memory.clone().map(|m| Arc::new(RuleEngine::new(m, clock.clone())));
        let shared_clock = clock.clone();
        // Private mode (PRIVATE_MODEL_BASE): letters are read by a Meta open-weights model, not the cloud.
        let private = config.as_ref().and_then(OpenWeightsClient::from_config).map(Arc::new);
        let private_mode = private.is_some();
        if let Some(p) = &private {
            tracing::info!(model = p.model(), base = p.base(), "private mode: letters stay on this computer");
        }
        // In private mode letters never need the cloud, so a missing MODEL_API_KEY must not stop
        // the letter pipeline: it gets a cloud client that points nowhere and is never called.
        let meta_for_docs = meta
            .clone()
            .or_else(|| private.as_ref().map(|_| Arc::new(MetaClient::new("http://127.0.0.1:9", "unused"))));
        let docs = match (&meta_for_docs, &memory, &rules) {
            (Some(meta), Some(mem), Some(rules)) => {
                let pipeline = DocPipeline::new(meta.clone(), mem.clone(), rules.clone());
                Some(Arc::new(match &private {
                    Some(p) => pipeline.with_private(p.clone()),
                    None => pipeline,
                }))
            }
            _ => None,
        };
        let platform = ok_or_log("platform", guard("platform", pip_platform::native));
        // DEMO_MODE pages: the shipped/repo fixtures folder and the copy of the demo form in the data folder.
        let builtin = config.as_ref().and_then(|c| write_demo_form(&c.data_dir.join("fixtures")));
        let mut fixture_dirs: Vec<std::path::PathBuf> = config.iter().map(|c| c.fixtures_dir.clone()).collect();
        fixture_dirs.extend(config.iter().map(|c| c.data_dir.join("fixtures")));
        let gate = Gate::new(demo_mode).with_fixtures(&fixture_dirs);
        let agent = match (&meta, platform, &memory) {
            (Some(meta), Some(platform), Some(mem)) => {
                Some(Arc::new(Agent::new(meta.clone(), platform, mem.clone(), gate, bus.clone(), clock)))
            }
            _ => None,
        };
        tracing::info!(
            memory = memory.is_some(),
            meta = meta.is_some(),
            docs = docs.is_some(),
            agent = agent.is_some(),
            demo_mode,
            private_mode,
            "core services"
        );
        let backend = Self {
            mock: false,
            memory,
            meta,
            rules,
            docs,
            agent,
            clock: shared_clock,
            profile_fallback: Mutex::default(),
        };
        // Before PIP_LANG below, so PIP_LANG still wins over the demo profile's language.
        if env_flag("PIP_DEMO_PROFILE")
            && let (Some(cfg), Some(mem), Some(rules)) = (&config, &backend.memory, &backend.rules)
        {
            let dir = cfg.data_dir.join("demo-letters");
            if let Err(e) = write_demo_letters(&dir) {
                tracing::warn!("cannot write the demo letters: {e}");
            }
            seed_demo_memory(mem, rules, &dir, backend.clock.now(), env_flag("PIP_DEMO_RESET"));
        }
        // The agent reads the language from the profile; PIP_LANG wins so app and agent always agree.
        if let Ok(code) = std::env::var("PIP_LANG") {
            backend.profile_set(PROFILE_LANG, Lang::from_code(&code).code());
        }
        if demo_mode && let Some(cfg) = &config {
            let seeded =
                backend.seed_demo_form(&cfg.fixtures_dir) || builtin.is_some_and(|dir| backend.seed_demo_form(&dir));
            if !seeded && backend.profile_get(DEMO_FORM_KEY).is_none() {
                tracing::warn!("no demo form found: the agent has no page to send the sick note to");
            }
        }
        backend
    }

    /// True when Pip cannot work at all (no key, no database): show the setup bubble, not the onboarding.
    pub fn needs_setup(&self) -> bool {
        !self.mock && (self.meta.is_none() || self.memory.is_none() || self.agent.is_none() || self.docs.is_none())
    }

    /// DEMO_MODE: tell the agent where the (invented) insurer's contact form is, so
    /// "Schick die Krankschreibung an meine Krankenkasse" has a target. Never overrides a stored value.
    /// True if the profile now has a form address.
    fn seed_demo_form(&self, fixtures_dir: &std::path::Path) -> bool {
        let dir = fixtures_dir.join("demo-insurer");
        // The services page for after-task discovery, if this folder has one and none is stored yet.
        if self.profile_get(DEMO_SERVICES_KEY).is_none()
            && let Ok(services) = dir.join("services.html").canonicalize()
        {
            self.profile_set(DEMO_SERVICES_KEY, &file_url(&services));
        }
        if self.profile_get(DEMO_FORM_KEY).is_some() {
            return true;
        }
        let Ok(page) = dir.join("index.html").canonicalize() else { return false };
        self.profile_set(DEMO_FORM_KEY, &file_url(&page));
        true
    }

    pub fn profile_get(&self, key: &str) -> Option<String> {
        if let Some(mem) = &self.memory
            && let Some(v) = ok_or_log("profile_get", guard("profile_get", || mem.profile_get(key)))
        {
            return v;
        }
        self.profile_fallback.lock().expect("profile lock").get(key).cloned()
    }

    pub fn profile_set(&self, key: &str, value: &str) {
        self.profile_fallback.lock().expect("profile lock").insert(key.into(), value.into());
        if let Some(mem) = &self.memory {
            ok_or_log("profile_set", guard("profile_set", || mem.profile_set(key, value)));
        }
    }

    /// Profile language (key `lang`, the one the agent reads), overridable with PIP_LANG. Default English.
    pub fn lang(&self) -> Lang {
        std::env::var("PIP_LANG")
            .ok()
            .or_else(|| self.profile_get(PROFILE_LANG))
            .map(|s| Lang::from_code(&s))
            .unwrap_or_default()
    }
}

/// PIP_DEMO_PROFILE: make sure the memory holds the demo person's whole story, then seed her.
/// `reset` (PIP_DEMO_RESET=1): every recording day starts with a fresh Erika (the story is told relative to
/// today). Without it, an Erika whose story is incomplete (seeded on an older day or by an older build:
/// a letter or the questionnaire deadline missing) is seeded again, so the proactive offer always has
/// something to offer. A questionnaire already sent (deadline Done) is progress, not a stale seed: what Pip
/// learned survives a restart. Only the invented person's memory is ever wiped, never a real one.
fn seed_demo_memory(
    mem: &Memory,
    rules: &RuleEngine,
    letters_dir: &std::path::Path,
    now: DateTime<Local>,
    reset: bool,
) {
    let full_name = mem.profile_get("full_name").ok().flatten();
    let name = mem.profile_get("name").ok().flatten();
    let allowed = demo_reset_allowed(full_name.as_deref(), name.as_deref());
    let wipe = |why: &str| match mem.wipe_everything() {
        Ok(()) => tracing::info!("{why}"),
        Err(e) => tracing::warn!("demo memory wipe failed: {e:#}"),
    };
    if reset {
        if allowed {
            wipe("demo memory wiped: seeding Erika again for today");
        } else {
            tracing::warn!("PIP_DEMO_RESET ignored: the memory belongs to someone other than the demo person");
        }
    } else if allowed && name.is_some() && demo_seed_is_complete(mem).ok() == Some(false) {
        let letters = mem.documents_count().unwrap_or(0);
        wipe(&format!("demo memory was stale (letters: {letters}): seeded Erika again for today"));
    }
    match guard("demo profile", || seed_demo_profile(mem, rules, letters_dir, now)) {
        Ok(true) => tracing::info!("seeded demo profile"),
        Ok(false) => tracing::info!("demo profile not seeded: the profile already has a name"),
        Err(e) => tracing::warn!("demo profile failed: {e:#}"),
    }
}

/// Write the built-in demo letters to `dir` (refreshed every start).
fn write_demo_letters(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    DEMO_LETTERS.iter().try_for_each(|(name, bytes)| std::fs::write(dir.join(name), bytes))
}

/// Write the built-in demo pages to `<dir>/demo-insurer/` and `<dir>/demo-uni/` (refreshed every start). Returns `dir`.
fn write_demo_form(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let r = DEMO_PAGES.iter().try_for_each(|(rel, html)| {
        let path = dir.join(rel);
        path.parent().map_or(Ok(()), std::fs::create_dir_all)?;
        std::fs::write(path, html)
    });
    match r {
        Ok(()) => Some(dir.to_path_buf()),
        Err(e) => {
            tracing::warn!("cannot write the demo form: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_turns_panic_into_error() {
        let r: Result<u8> = guard("x", || panic!("not implemented yet"));
        let e = r.unwrap_err().to_string();
        assert!(e.contains("not implemented yet"), "{e}");
        assert_eq!(guard("y", || Ok(3)).unwrap(), 3);
    }

    #[tokio::test]
    async fn guard_async_turns_panic_into_error() {
        let r: Result<u8> = guard_async("x", async { todo!("later") }).await;
        assert!(r.unwrap_err().to_string().contains("later"));
        assert_eq!(guard_async("y", async { Ok(5u8) }).await.unwrap(), 5);
    }

    #[test]
    fn demo_form_is_seeded_once_and_allowed_by_the_gate() {
        let mut b = Backend::mock();
        b.memory = Some(Arc::new(Memory::open_in_memory().unwrap()));
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        assert!(b.seed_demo_form(&fixtures));
        let url = b.profile_get(DEMO_FORM_KEY).expect("seeded");
        assert!(url.starts_with("file:///") && url.ends_with("/fixtures/demo-insurer/index.html"), "{url}");
        let services = b.profile_get(DEMO_SERVICES_KEY).expect("services seeded");
        assert!(services.ends_with("/fixtures/demo-insurer/services.html"), "{services}");
        assert!(Gate::new(true).with_fixtures(&[&fixtures]).url_allowed(&services));
        assert!(Gate::new(true).with_fixtures(&[&fixtures]).url_allowed(&url));
        b.profile_set(DEMO_FORM_KEY, "http://localhost:8080/form");
        b.seed_demo_form(&fixtures);
        assert_eq!(b.profile_get(DEMO_FORM_KEY).as_deref(), Some("http://localhost:8080/form"));
    }

    #[test]
    fn built_in_demo_form_works_without_a_fixtures_folder() {
        let dir = tempfile::tempdir().unwrap();
        let fx = write_demo_form(&dir.path().join("fixtures")).expect("written");
        let mut b = Backend::mock();
        b.memory = Some(Arc::new(Memory::open_in_memory().unwrap()));
        assert!(!b.seed_demo_form(&dir.path().join("missing")));
        assert!(b.seed_demo_form(&fx));
        let url = b.profile_get(DEMO_FORM_KEY).unwrap();
        assert!(Gate::new(true).with_fixtures(&[&fx]).url_allowed(&url), "{url}");
        let html = std::fs::read_to_string(fx.join("demo-insurer/index.html")).unwrap();
        assert!(html.contains("Evergreen Health") && html.contains("Submit"));
        // The services page and the questionnaire come along, are seeded and allowed by the gate.
        let services = b.profile_get(DEMO_SERVICES_KEY).expect("services seeded");
        assert!(services.ends_with("/demo-insurer/services.html"), "{services}");
        let gate = Gate::new(true).with_fixtures(&[&fx]);
        assert!(gate.url_allowed(&services));
        assert!(gate.url_allowed(&services.replace("services.html", "questionnaire.html")));
        let page = std::fs::read_to_string(fx.join("demo-insurer/services.html")).unwrap();
        assert!(page.contains("Return your questionnaire online") && page.contains("questionnaire.html"));
        let form = std::fs::read_to_string(fx.join("demo-insurer/questionnaire.html")).unwrap();
        assert!(form.contains("Still unable to work?") && form.contains("Thank you, received on"));
    }

    #[test]
    fn built_in_uni_page_is_written_and_is_its_own_site() {
        let dir = tempfile::tempdir().unwrap();
        let fx = write_demo_form(&dir.path().join("fixtures")).expect("written");
        for rel in [
            "demo-insurer/index.html",
            "demo-insurer/services.html",
            "demo-insurer/questionnaire.html",
            "demo-uni/index.html",
        ] {
            assert!(fx.join(rel).is_file(), "{rel}");
        }
        let gate = Gate::new(true).with_fixtures(&[&fx]);
        let uni = file_url(&fx.join("demo-uni/index.html").canonicalize().unwrap());
        assert!(gate.url_allowed(&uni), "{uni}");
        let insurer = file_url(&fx.join("demo-insurer/index.html").canonicalize().unwrap());
        let (uni_site, insurer_site) = (gate.site_key(&uni).unwrap(), gate.site_key(&insurer).unwrap());
        assert!(
            uni_site.ends_with("/demo-uni") && insurer_site.ends_with("/demo-insurer"),
            "{uni_site} {insurer_site}"
        );
        // Seeding still points the insurer's form and services page at demo-insurer/, never at the university.
        let mut b = Backend::mock();
        b.memory = Some(Arc::new(Memory::open_in_memory().unwrap()));
        assert!(b.seed_demo_form(&fx));
        let services = b.profile_get(DEMO_SERVICES_KEY).expect("services seeded");
        assert!(services.ends_with("/demo-insurer/services.html"), "{services}");
        assert!(b.profile_get(DEMO_FORM_KEY).unwrap().ends_with("/demo-insurer/index.html"));
    }

    #[test]
    fn built_in_uni_page_has_the_form_and_nothing_of_the_insurer() {
        let (_, html) = DEMO_PAGES.iter().find(|(rel, _)| *rel == "demo-uni/index.html").expect("uni page built in");
        for label in [
            "Full name",
            "Student number",
            "Programme",
            "I have paid the semester fee of 310.00 euros",
            "Submit re-registration",
            "Nothing is sent.",
            "Thank you, your re-registration was received on",
        ] {
            assert!(html.contains(label), "{label}");
        }
        assert!(!html.to_lowercase().contains("evergreen health"));
    }

    #[test]
    fn demo_profile_from_built_in_letters() {
        let names: Vec<&str> = DEMO_LETTERS.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, pip_core::memory::DEMO_LETTER_FILES);
        let dir = tempfile::tempdir().unwrap();
        let letters = dir.path().join("demo-letters");
        write_demo_letters(&letters).unwrap();
        let mem = Arc::new(Memory::open_in_memory().unwrap());
        let now = chrono::Local::now();
        let rules = RuleEngine::new(mem.clone(), Arc::new(pip_core::clock::FixedClock::new(now)));
        assert!(seed_demo_profile(&mem, &rules, &letters, now).unwrap());
        assert_eq!(mem.profile_get("name").unwrap().as_deref(), Some("Erika"));
        let wohngeld = mem.search_documents("Wohngeld", 5).unwrap();
        assert!(wohngeld.iter().any(|d| d.path == letters.join("bescheid_wohngeld.pdf") && d.path.exists()));
        assert!(!seed_demo_profile(&mem, &rules, &letters, now).unwrap());
    }

    #[test]
    fn demo_reset_wipes_only_the_invented_person() {
        assert!(demo_reset_allowed(Some("Erika Beispiel"), Some("Erika")));
        assert!(demo_reset_allowed(None, None), "an empty memory");
        assert!(!demo_reset_allowed(Some("Marta Muster"), Some("Marta")));
        assert!(!demo_reset_allowed(Some("erika beispiel"), None), "exactly the invented name");
        assert!(!demo_reset_allowed(None, Some("Marta")), "a first name only: maybe a real person");
        // The guard and the seed agree on who the demo person is.
        let dir = tempfile::tempdir().unwrap();
        let letters = dir.path().join("demo-letters");
        write_demo_letters(&letters).unwrap();
        let mem = Arc::new(Memory::open_in_memory().unwrap());
        let now = chrono::Local::now();
        let rules = RuleEngine::new(mem.clone(), Arc::new(pip_core::clock::FixedClock::new(now)));
        assert!(seed_demo_profile(&mem, &rules, &letters, now).unwrap());
        let full = mem.profile_get("full_name").unwrap();
        assert!(demo_reset_allowed(full.as_deref(), mem.profile_get("name").unwrap().as_deref()));
        mem.wipe_everything().unwrap();
        assert!(seed_demo_profile(&mem, &rules, &letters, now).unwrap(), "after the wipe Erika is seeded again");
    }

    fn demo_setup() -> (tempfile::TempDir, std::path::PathBuf, Arc<Memory>, RuleEngine, DateTime<Local>) {
        let dir = tempfile::tempdir().unwrap();
        let letters = dir.path().join("demo-letters");
        write_demo_letters(&letters).unwrap();
        let mem = Arc::new(Memory::open_in_memory().unwrap());
        let now = chrono::Local::now();
        let rules = RuleEngine::new(mem.clone(), Arc::new(pip_core::clock::FixedClock::new(now)));
        (dir, letters, mem, rules, now)
    }

    #[test]
    fn stale_demo_memory_is_seeded_again() {
        let (_dir, letters, mem, rules, now) = demo_setup();
        // Erika from an older build: her name, no letters, no deadlines.
        mem.profile_set("name", "Erika").unwrap();
        mem.profile_set("full_name", "Erika Beispiel").unwrap();
        mem.profile_set("old_note", "from an older build").unwrap();
        seed_demo_memory(&mem, &rules, &letters, now, false);
        assert!(demo_seed_is_complete(&mem).unwrap());
        assert_eq!(mem.documents_count().unwrap(), 3);
        assert!(mem.open_deadlines().unwrap().iter().any(|dl| dl.rule_id.as_deref() == Some("frist_aus_brief")));
        assert_eq!(mem.profile_get("old_note").unwrap(), None, "wiped before seeding");
        // A complete Erika is left alone (her playbooks and facts stay).
        mem.profile_set("learned", "yes").unwrap();
        seed_demo_memory(&mem, &rules, &letters, now, false);
        assert_eq!(mem.profile_get("learned").unwrap().as_deref(), Some("yes"));
        assert_eq!(mem.documents_count().unwrap(), 3);
    }

    #[test]
    fn another_persons_memory_is_never_wiped() {
        for reset in [false, true] {
            let (_dir, letters, mem, rules, now) = demo_setup();
            mem.profile_set("name", "Jonas").unwrap();
            mem.profile_set("full_name", "Jonas Muster").unwrap();
            mem.profile_set("insurer", "Real Insurer").unwrap();
            seed_demo_memory(&mem, &rules, &letters, now, reset);
            assert_eq!(mem.profile_get("full_name").unwrap().as_deref(), Some("Jonas Muster"), "reset={reset}");
            assert_eq!(mem.profile_get("insurer").unwrap().as_deref(), Some("Real Insurer"));
            assert_eq!(mem.documents_count().unwrap(), 0);
            // A first name only may be a real person too.
            let (_dir, letters, mem, rules, now) = demo_setup();
            mem.profile_set("name", "Marta").unwrap();
            seed_demo_memory(&mem, &rules, &letters, now, reset);
            assert_eq!(mem.profile_get("name").unwrap().as_deref(), Some("Marta"));
            assert_eq!(mem.documents_count().unwrap(), 0);
        }
    }

    #[test]
    fn mock_profile_roundtrip_and_lang() {
        let b = Backend::mock();
        assert_eq!(b.profile_get("name"), None);
        b.profile_set("name", "Erika");
        assert_eq!(b.profile_get("name").as_deref(), Some("Erika"));
        b.profile_set(PROFILE_LANG, "en");
        if std::env::var("PIP_LANG").is_err() {
            assert_eq!(b.lang(), Lang::En);
        }
    }
}
