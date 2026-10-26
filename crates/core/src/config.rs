//! Runtime configuration from the environment / .env. OWNER: agent "meta".
//! The API key is NOT part of Config (only MetaClient reads it).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Config {
    /// MODEL_API_BASE, default https://api.meta.ai/v1
    pub api_base: String,
    /// AGENT_MODEL, default muse-spark-1.3
    pub agent_model: String,
    /// DEMO_MODE (default ON; `DEMO_MODE=0` turns it off): open_url and commit actions only on
    /// localhost / file://…/fixtures/. On by default so a hackathon build can never send to a real site.
    pub demo_mode: bool,
    /// DEBUG_SHOTS=1: write screenshots to data_dir/shots (demo data only)
    pub debug_shots: bool,
    /// %APPDATA%\Pip (Windows) or ~/.local/share/Pip (Linux)
    pub data_dir: PathBuf,
    /// FIXTURES_DIR, default <repo>/fixtures (used by DEMO_MODE url checks)
    pub fixtures_dir: PathBuf,
    /// PRIVATE_MODEL_BASE: OpenAI-compatible endpoint of a Meta open-weights model for private mode,
    /// e.g. `http://127.0.0.1:8080/v1` (llama.cpp `llama-server`) or a Baseten URL ending in `/v1`.
    /// The key (PRIVATE_MODEL_KEY, optional) is NOT part of Config (only OpenWeightsClient reads it).
    pub private_model_base: Option<String>,
    /// PRIVATE_MODEL, default muse-glimmer (Baseten e.g. `meta-models/muse-spark-open-1026`)
    pub private_model: String,
    /// PIP_PRIVATE=0 turns private mode off even when the base is set.
    pub private_disabled: bool,
}

impl Config {
    /// Loads `.env` first (current folder, then next to the exe; see `meta::load_dotenv`).
    /// Creates data_dir if missing.
    pub fn from_env() -> Result<Self> {
        crate::meta::load_dotenv();
        let cfg = Self::from_lookup(|k| std::env::var(k).ok());
        std::fs::create_dir_all(&cfg.data_dir)
            .with_context(|| format!("cannot create data folder {}", cfg.data_dir.display()))?;
        Ok(cfg)
    }

    /// Pure version of `from_env` (no .env, no folder creation) for tests.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let var = |k: &str| get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let flag =
            |k: &str| var(k).is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"));
        Self {
            api_base: var("MODEL_API_BASE").unwrap_or_else(|| crate::meta::DEFAULT_BASE.to_string()),
            agent_model: var("AGENT_MODEL").unwrap_or_else(|| crate::meta::MODEL_SPARK.to_string()),
            demo_mode: !var("DEMO_MODE")
                .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off")),
            debug_shots: flag("DEBUG_SHOTS"),
            data_dir: data_dir(&var),
            fixtures_dir: var("FIXTURES_DIR").map(PathBuf::from).unwrap_or_else(default_fixtures_dir),
            private_model_base: var("PRIVATE_MODEL_BASE"),
            private_model: var("PRIVATE_MODEL").unwrap_or_else(|| crate::meta::MODEL_GLIMMER.to_string()),
            private_disabled: var("PIP_PRIVATE")
                .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off")),
        }
    }

    /// Private mode: letters are read by the open-weights model at PRIVATE_MODEL_BASE
    /// (set, and not turned off with PIP_PRIVATE=0).
    pub fn private_mode(&self) -> bool {
        self.private_model_base.is_some() && !self.private_disabled
    }

    /// data_dir/memory.db
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("memory.db")
    }
}

const APP_DIR: &str = "Pip";

/// data_dir/pip.log: the app's log on machines without a console (Windows release build).
pub fn log_file() -> PathBuf {
    data_dir(&|k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty())).join("pip.log")
}

/// data_dir/.env: where the key pasted into Pip is kept (read by `meta::load_dotenv`).
pub fn env_file() -> PathBuf {
    data_dir(&|k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty())).join(".env")
}

/// A pasted API key looks plausible: one line, no spaces, 16..512 printable ASCII characters.
pub fn plausible_api_key(key: &str) -> bool {
    let k = key.trim();
    (16..=512).contains(&k.len()) && k.chars().all(|c| c.is_ascii_graphic()) && !k.contains('=')
}

/// Store the key pasted into Pip in `env_file` (replaces an older MODEL_API_KEY line, keeps other lines).
/// The key is never logged. Takes effect at the next start.
pub fn save_api_key(key: &str) -> Result<PathBuf> {
    anyhow::ensure!(plausible_api_key(key), "this does not look like an API key");
    let path = env_file();
    write_key_line(&path, key.trim())?;
    Ok(path)
}

fn write_key_line(path: &Path, key: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    let old = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> =
        old.lines().filter(|l| !l.trim_start().starts_with("MODEL_API_KEY")).map(str::to_string).collect();
    lines.push(format!("MODEL_API_KEY={key}"));
    std::fs::write(path, lines.join("\n") + "\n").with_context(|| format!("cannot write {}", path.display()))
}

/// `file:///C:/x/y.html` (Windows) or `file:///x/y.html` for a local file, e.g. the demo form.
/// Backslashes become slashes, a Windows `\\?\` prefix is dropped, spaces and `%` are escaped.
pub fn file_url(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s);
    let s = s.replace('%', "%25").replace(' ', "%20");
    if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") }
}

fn data_dir(var: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    if let Some(d) = var("PAPIERKRAM_DATA_DIR") {
        return PathBuf::from(d);
    }
    if cfg!(windows) {
        if let Some(app) = var("APPDATA") {
            return Path::new(&app).join(APP_DIR);
        }
    } else if let Some(xdg) = var("XDG_DATA_HOME") {
        return Path::new(&xdg).join(APP_DIR);
    } else if let Some(home) = var("HOME") {
        return Path::new(&home).join(".local/share").join(APP_DIR);
    }
    std::env::temp_dir().join(APP_DIR)
}

/// `fixtures/` next to the exe (shipped demo), else the repo's `fixtures/` (cargo run).
fn default_fixtures_dir() -> PathBuf {
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("fixtures")))
        && dir.is_dir()
    {
        return dir;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> Config {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Config::from_lookup(|k| m.get(k).cloned())
    }

    #[test]
    fn defaults() {
        let c = cfg(&[("HOME", "/home/x")]);
        assert_eq!(c.api_base, "https://api.meta.ai/v1");
        assert_eq!(c.agent_model, "muse-spark-1.3");
        assert!(c.demo_mode, "demo mode is on unless turned off");
        assert!(!c.debug_shots);
        if !cfg!(windows) {
            assert_eq!(c.data_dir, PathBuf::from("/home/x/.local/share/Pip"));
            assert_eq!(c.db_path(), PathBuf::from("/home/x/.local/share/Pip/memory.db"));
        }
    }

    #[test]
    fn file_urls() {
        assert_eq!(file_url(Path::new("/home/x/fixtures/a.html")), "file:///home/x/fixtures/a.html");
        assert_eq!(file_url(Path::new("/a b/c%.html")), "file:///a%20b/c%25.html");
        assert_eq!(file_url(Path::new(r"\\?\C:\Users\E\fixtures\x.html")), "file:///C:/Users/E/fixtures/x.html");
        let gate = crate::agent::Gate::new(true).with_fixtures(&[r"\\?\C:\Pip\fixtures"]);
        assert!(gate.url_allowed(&file_url(Path::new(r"\\?\C:\Pip\fixtures\demo-insurer\index.html"))));
    }

    #[test]
    fn pasted_key_is_checked_and_stored() {
        assert!(plausible_api_key(" sk-abcdefghijklmnop1234 "));
        assert!(!plausible_api_key("short"));
        assert!(!plausible_api_key("two words here and more"));
        assert!(!plausible_api_key("MODEL_API_KEY=abcdefghijklmnopq"));
        let dir = tempfile::tempdir().unwrap();
        let env = dir.path().join("sub").join(".env");
        std::fs::create_dir_all(env.parent().unwrap()).unwrap();
        std::fs::write(&env, "DEMO_MODE=1\nMODEL_API_KEY=old\n").unwrap();
        write_key_line(&env, "new-key-0123456789").unwrap();
        assert_eq!(std::fs::read_to_string(&env).unwrap(), "DEMO_MODE=1\nMODEL_API_KEY=new-key-0123456789\n");
    }

    #[test]
    fn overrides() {
        let c = cfg(&[
            ("MODEL_API_BASE", "http://localhost:9/v1"),
            ("AGENT_MODEL", "m2"),
            ("DEMO_MODE", "1"),
            ("DEBUG_SHOTS", "true"),
            ("PAPIERKRAM_DATA_DIR", "/tmp/pk"),
            ("FIXTURES_DIR", "/tmp/fx"),
        ]);
        assert_eq!(c.api_base, "http://localhost:9/v1");
        assert_eq!(c.agent_model, "m2");
        assert!(c.demo_mode && c.debug_shots);
        assert_eq!(c.data_dir, PathBuf::from("/tmp/pk"));
        assert_eq!(c.fixtures_dir, PathBuf::from("/tmp/fx"));
        assert!(!cfg(&[("DEMO_MODE", "0")]).demo_mode);
        assert!(!cfg(&[("DEMO_MODE", "off")]).demo_mode);
        assert!(cfg(&[("DEMO_MODE", " ")]).demo_mode);
    }

    #[test]
    fn private_mode_switch() {
        let off = cfg(&[]);
        assert!(!off.private_mode(), "no base, no private mode");
        assert_eq!(off.private_model, "muse-glimmer");
        assert_eq!(off.private_model_base, None);

        let on = cfg(&[("PRIVATE_MODEL_BASE", " http://127.0.0.1:8080/v1 ")]);
        assert!(on.private_mode());
        assert_eq!(on.private_model_base.as_deref(), Some("http://127.0.0.1:8080/v1"));
        assert_eq!(on.private_model, "muse-glimmer");

        let baseten = cfg(&[
            ("PRIVATE_MODEL_BASE", "https://inference.baseten.co/v1"),
            ("PRIVATE_MODEL", "meta-models/muse-spark-open-1026"),
            ("PIP_PRIVATE", "1"),
        ]);
        assert!(baseten.private_mode());
        assert_eq!(baseten.private_model, "meta-models/muse-spark-open-1026");

        for no in ["0", "off", "false"] {
            assert!(!cfg(&[("PRIVATE_MODEL_BASE", "http://x/v1"), ("PIP_PRIVATE", no)]).private_mode(), "{no}");
        }
        assert!(!cfg(&[("PRIVATE_MODEL_BASE", "  ")]).private_mode(), "blank base is no base");
    }
}
