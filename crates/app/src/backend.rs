//! The core services the app talks to. Every piece is optional: if one cannot start
//! (no key, no database, a part of core not finished yet) the overlay still runs and
//! Pip says a friendly sentence instead. `PIP_MOCK=1` runs a scripted demo without core.

use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use copilot_core::agent::{Agent, Gate, PROFILE_LANG};
use copilot_core::clock::{Clock, SystemClock, clock_from_env};
use copilot_core::config::{Config, file_url};
use copilot_core::docs::DocPipeline;
use copilot_core::events::Bus;
use copilot_core::i18n::Lang;
use copilot_core::memory::{Memory, default_db_path};
use copilot_core::meta::MetaClient;
use copilot_core::rules::RuleEngine;

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

/// Profile key with the demo insurer's contact form (file:// URL of fixtures/demo-kasse/index.html).
pub const DEMO_FORM_KEY: &str = "krankenkasse_kontaktformular";

/// The invented insurer's form, built into the exe so a fresh download works without a `fixtures/` folder.
const DEMO_FORM_HTML: &str = include_str!("../../../fixtures/demo-kasse/index.html");

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
        let docs = match (&meta, &memory, &rules) {
            (Some(meta), Some(mem), Some(rules)) => {
                Some(Arc::new(DocPipeline::new(meta.clone(), mem.clone(), rules.clone())))
            }
            _ => None,
        };
        let platform = ok_or_log("platform", guard("platform", copilot_platform::native));
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
        let page = fixtures_dir.join("demo-kasse").join("index.html");
        if self.profile_get(DEMO_FORM_KEY).is_some() {
            return true;
        }
        let Ok(page) = page.canonicalize() else { return false };
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

/// Write the built-in demo form to `<dir>/demo-kasse/index.html` (refreshed every start). Returns `dir`.
fn write_demo_form(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let page_dir = dir.join("demo-kasse");
    let r =
        std::fs::create_dir_all(&page_dir).and_then(|_| std::fs::write(page_dir.join("index.html"), DEMO_FORM_HTML));
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
        assert!(url.starts_with("file:///") && url.ends_with("/fixtures/demo-kasse/index.html"), "{url}");
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
        let html = std::fs::read_to_string(fx.join("demo-kasse/index.html")).unwrap();
        assert!(html.contains("Gesundkasse") && html.contains("Submit"));
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
