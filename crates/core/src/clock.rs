//! Clock abstraction (demo time jumps). OWNER: agent "memory-rules".

use std::sync::Arc;

use anyhow::{Context, bail};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Local>;
}

/// Real time.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Local> {
        Local::now()
    }
}

/// Starts at a fixed local time and runs forward with real time.
#[derive(Debug, Clone)]
pub struct DemoClock {
    start_fake: DateTime<Local>,
    start_real: std::time::Instant,
}

impl DemoClock {
    pub fn new(start: DateTime<Local>) -> Self {
        Self { start_fake: start, start_real: std::time::Instant::now() }
    }
    /// Parse "2026-10-29T18:00" (local time). Also accepts seconds, a space instead of "T",
    /// or a full RFC 3339 time with offset.
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        let s = s.trim();
        if let Ok(t) = DateTime::parse_from_rfc3339(s) {
            return Ok(Self::new(t.with_timezone(&Local)));
        }
        let naive = ["%Y-%m-%dT%H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%d %H:%M:%S"]
            .iter()
            .find_map(|f| NaiveDateTime::parse_from_str(s, f).ok())
            .with_context(|| format!("DEMO_NOW {s:?} is not like 2026-10-29T18:00"))?;
        match Local.from_local_datetime(&naive).earliest() {
            Some(t) => Ok(Self::new(t)),
            None => bail!("DEMO_NOW {s:?} does not exist in local time (clock change)"),
        }
    }
}

impl Clock for DemoClock {
    fn now(&self) -> DateTime<Local> {
        let elapsed = chrono::Duration::from_std(self.start_real.elapsed()).unwrap_or_default();
        self.start_fake + elapsed
    }
}

/// DEMO_NOW set -> DemoClock, else SystemClock.
/// A bad DEMO_NOW is logged and ignored (real time is the safe fallback).
pub fn clock_from_env() -> Arc<dyn Clock> {
    match std::env::var("DEMO_NOW") {
        Ok(v) if !v.trim().is_empty() => match DemoClock::parse(&v) {
            Ok(c) => {
                tracing::info!(start = %c.start_fake, "demo clock");
                Arc::new(c)
            }
            Err(e) => {
                tracing::warn!("{e:#}; using real time");
                Arc::new(SystemClock)
            }
        },
        _ => Arc::new(SystemClock),
    }
}

/// Fixed time, for tests and the CLI (`remind --at`).
#[derive(Debug)]
pub struct FixedClock(pub std::sync::Mutex<DateTime<Local>>);

impl FixedClock {
    pub fn new(t: DateTime<Local>) -> Self {
        Self(std::sync::Mutex::new(t))
    }
    pub fn set(&self, t: DateTime<Local>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = t;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Local> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn parse_formats() {
        for s in ["2026-10-29T18:00", "2026-10-29T18:00:00", "2026-10-29 18:00", " 2026-10-29T18:00 "] {
            let c = DemoClock::parse(s).unwrap();
            assert_eq!(c.start_fake.format("%Y-%m-%d %H:%M").to_string(), "2026-10-29 18:00", "{s}");
        }
        let c = DemoClock::parse("2026-10-29T17:00:00Z").unwrap();
        assert_eq!(c.start_fake.timestamp(), 1_793_293_200);
        assert!(DemoClock::parse("morgen").is_err());
        assert!(DemoClock::parse("2026-13-01T10:00").is_err());
    }

    #[test]
    fn demo_clock_runs_forward() {
        let c = DemoClock::parse("2026-10-29T18:00").unwrap();
        let a = c.now();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let b = c.now();
        assert!(b > a);
        assert_eq!(a.hour(), 18);
        assert!((b - c.start_fake).num_seconds() < 5);
    }

    #[test]
    fn fixed_clock_set() {
        let t = Local::now();
        let c = FixedClock::new(t);
        assert_eq!(c.now(), t);
        c.set(t + chrono::Duration::hours(1));
        assert_eq!(c.now(), t + chrono::Duration::hours(1));
    }
}
