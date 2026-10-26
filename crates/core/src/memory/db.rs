//! OWNER: agent "memory-rules". Signatures frozen (other agents call them).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Local, NaiveDate, SecondsFormat, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::docs::{DocType, DocumentExtract};

pub struct Memory(Mutex<rusqlite::Connection>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentRow {
    pub id: i64,
    pub path: PathBuf,
    pub sha256: String,
    pub doc_type: DocType,
    pub extract: DocumentExtract,
    pub created_at: DateTime<Local>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeadlineStatus {
    Open,
    Done,
    Dismissed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDeadline {
    pub title: String,
    pub due: NaiveDate,
    pub rule_id: Option<String>,
    pub doc_id: Option<i64>,
    /// One reminders row is created per entry.
    pub remind_at: Vec<DateTime<Local>>,
    pub why: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deadline {
    pub id: i64,
    pub title: String,
    pub due: NaiveDate,
    pub rule_id: Option<String>,
    pub doc_id: Option<i64>,
    pub status: DeadlineStatus,
    pub remind_at: Vec<DateTime<Local>>,
    pub why: String,
    pub source: String,
}

/// A reminder joined with its deadline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reminder {
    pub id: i64,
    pub at: DateTime<Local>,
    pub deadline: Deadline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub at: DateTime<Local>,
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionLog {
    pub at: DateTime<Local>,
    pub tool: String,
    /// Text fields truncated to 80 chars; never secrets.
    pub args_redacted: String,
    /// "free" | "act" | "commit"
    pub risk: String,
    pub approved: Option<bool>,
    pub result: String,
    pub tokens: u64,
}

/// %APPDATA%\papierkram-copilot\memory.db (Windows, via env APPDATA) or
/// ~/.local/share/papierkram-copilot/memory.db (Linux, via HOME / XDG_DATA_HOME). No `dirs` crate.
pub fn default_db_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("memory.db"))
}

fn data_dir() -> Result<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("APPDATA").context("APPDATA is not set")?
    } else if let Some(x) = env("XDG_DATA_HOME") {
        x
    } else {
        env("HOME").context("HOME is not set")?.join(".local").join("share")
    };
    Ok(base.join("papierkram-copilot"))
}

/// Schema migrations; entry i brings the db to `user_version = i + 1`. Append, never edit.
const MIGRATIONS: &[&str] = &[super::SCHEMA_SQL];

/// Stored time format: RFC 3339, UTC, seconds, "Z" (sorts like time).
fn ts(t: DateTime<Local>) -> String {
    t.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn parse_ts(s: &str) -> rusqlite::Result<DateTime<Local>> {
    DateTime::parse_from_rfc3339(s).map(|t| t.with_timezone(&Local)).map_err(|e| conv_err(e.into()))
}

fn parse_date(s: &str) -> rusqlite::Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| conv_err(e.into()))
}

fn conv_err(e: anyhow::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
}

impl DeadlineStatus {
    fn as_str(self) -> &'static str {
        match self {
            DeadlineStatus::Open => "open",
            DeadlineStatus::Done => "done",
            DeadlineStatus::Dismissed => "dismissed",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "done" => DeadlineStatus::Done,
            "dismissed" => DeadlineStatus::Dismissed,
            _ => DeadlineStatus::Open,
        }
    }
}

const DOC_COLS: &str = "d.id, d.path, d.sha256, d.doc_type, d.extract_json, d.created_at";
const DEADLINE_COLS: &str =
    "dl.id, dl.title, dl.due, dl.rule_id, dl.doc_id, dl.status, dl.remind_at, dl.why, dl.source";

fn doc_from_row(r: &Row) -> rusqlite::Result<DocumentRow> {
    let extract: DocumentExtract = serde_json::from_str(&r.get::<_, String>(4)?).map_err(|e| conv_err(e.into()))?;
    let doc_type = DocType::parse(&r.get::<_, String>(3)?).unwrap_or(extract.doc_type);
    Ok(DocumentRow {
        id: r.get(0)?,
        path: PathBuf::from(r.get::<_, String>(1)?),
        sha256: r.get(2)?,
        doc_type,
        extract,
        created_at: parse_ts(&r.get::<_, String>(5)?)?,
    })
}

/// Reads DEADLINE_COLS starting at column `o`.
fn deadline_from_row(r: &Row, o: usize) -> rusqlite::Result<Deadline> {
    let remind: Vec<String> = serde_json::from_str(&r.get::<_, String>(o + 6)?).unwrap_or_default();
    Ok(Deadline {
        id: r.get(o)?,
        title: r.get(o + 1)?,
        due: parse_date(&r.get::<_, String>(o + 2)?)?,
        rule_id: r.get(o + 3)?,
        doc_id: r.get(o + 4)?,
        status: DeadlineStatus::parse(&r.get::<_, String>(o + 5)?),
        remind_at: remind.iter().filter_map(|s| parse_ts(s).ok()).collect(),
        why: r.get(o + 7)?,
        source: r.get(o + 8)?,
    })
}

/// German words for a letter type, indexed so "Krankschreibung" finds AU letters.
fn type_words(t: DocType) -> &'static str {
    match t {
        DocType::AuBescheinigung => "Krankschreibung Arbeitsunfähigkeitsbescheinigung AU sick note",
        DocType::Bescheid => "Bescheid decision",
        DocType::Rechnung => "Rechnung invoice",
        DocType::Mahnung => "Mahnung reminder",
        DocType::Vertrag => "Vertrag contract",
        DocType::Brief => "Brief letter",
        DocType::Sonstiges => "",
    }
}

/// Turns free user text into a safe FTS5 query: every word quoted (quotes doubled),
/// words of 4+ chars as prefix, joined with OR (ranked by bm25). None if no word left.
fn fts_query(q: &str) -> Option<String> {
    let words: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(16)
        .map(|w| {
            let quoted = format!("\"{}\"", w.replace('"', "\"\""));
            if w.chars().count() >= 4 { format!("{quoted}*") } else { quoted }
        })
        .collect();
    (!words.is_empty()).then(|| words.join(" OR "))
}

impl Memory {
    /// Creates parent dirs, runs SCHEMA_SQL.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version.max(0) as usize) {
            let tx = conn.transaction()?;
            tx.execute_batch(sql).with_context(|| format!("memory migration {}", i + 1))?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(Self(Mutex::new(conn)))
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock cannot leave SQLite half-written (transactions roll back).
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn profile_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn().query_row("SELECT value FROM profile WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn profile_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO profile(key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value, ts(Local::now())],
        )?;
        Ok(())
    }

    /// All profile entries (for the prompt context), sorted by key.
    pub fn profile_all(&self) -> Result<Vec<(String, String)>> {
        let c = self.conn();
        let mut st = c.prepare("SELECT key, value FROM profile ORDER BY key")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Also fills documents_fts (text + extract.summary_simple). Same sha256 again -> returns existing id.
    pub fn insert_document(&self, path: &Path, sha256: &str, extract: &DocumentExtract, text: &str) -> Result<i64> {
        let mut c = self.conn();
        if let Some(id) =
            c.query_row("SELECT id FROM documents WHERE sha256 = ?1", [sha256], |r| r.get::<_, i64>(0)).optional()?
        {
            return Ok(id);
        }
        let json = serde_json::to_string(extract)?;
        let summary = [
            extract.summary_simple.as_str(),
            extract.sender.as_str(),
            extract.recipient.as_str(),
            extract.reference.as_deref().unwrap_or(""),
            type_words(extract.doc_type),
            &extract.requests.join(" "),
            &extract.deadlines.iter().map(|d| d.title.as_str()).collect::<Vec<_>>().join(" "),
        ]
        .join(" ");
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO documents(path, sha256, doc_type, extract_json, text, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![path.to_string_lossy(), sha256, extract.doc_type.as_str(), json, text, ts(Local::now())],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute("INSERT INTO documents_fts(rowid, text, summary) VALUES (?1, ?2, ?3)", params![id, text, summary])?;
        tx.commit()?;
        Ok(id)
    }

    pub fn document_by_sha(&self, sha256: &str) -> Result<Option<DocumentRow>> {
        let sql = format!("SELECT {DOC_COLS} FROM documents d WHERE d.sha256 = ?1");
        Ok(self.conn().query_row(&sql, [sha256], doc_from_row).optional()?)
    }

    pub fn document_by_id(&self, id: i64) -> Result<Option<DocumentRow>> {
        let sql = format!("SELECT {DOC_COLS} FROM documents d WHERE d.id = ?1");
        Ok(self.conn().query_row(&sql, [id], doc_from_row).optional()?)
    }

    /// FTS5 MATCH; quote/escape user input so "§ 46" or "AU-Bescheinigung" cannot break the query.
    /// Words are OR-ed and ranked (best first). Empty query -> newest letters.
    pub fn search_documents(&self, query: &str, limit: u32) -> Result<Vec<DocumentRow>> {
        let c = self.conn();
        let rows = match fts_query(query) {
            Some(q) => {
                let sql = format!(
                    "SELECT {DOC_COLS} FROM documents_fts f JOIN documents d ON d.id = f.rowid
                     WHERE documents_fts MATCH ?1 ORDER BY f.rank, d.id DESC LIMIT ?2"
                );
                let mut st = c.prepare(&sql)?;
                st.query_map(params![q, limit], doc_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?
            }
            None => {
                let sql = format!("SELECT {DOC_COLS} FROM documents d ORDER BY d.id DESC LIMIT ?1");
                let mut st = c.prepare(&sql)?;
                st.query_map(params![limit], doc_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?
            }
        };
        Ok(rows)
    }

    /// The stored letter whose file is `path` (compared without case and with either slash), newest first.
    pub fn document_by_path(&self, path: &str) -> Result<Option<DocumentRow>> {
        let norm = |s: &str| s.trim().trim_matches('"').replace('\\', "/").to_lowercase();
        let want = norm(path);
        if want.is_empty() {
            return Ok(None);
        }
        let c = self.conn();
        let sql = format!("SELECT {DOC_COLS} FROM documents d ORDER BY d.id DESC");
        let mut st = c.prepare(&sql)?;
        let rows = st.query_map([], doc_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().find(|d| norm(&d.path.to_string_lossy()) == want))
    }

    pub fn latest_document(&self, doc_type: DocType) -> Result<Option<DocumentRow>> {
        Ok(self.documents_of_type(doc_type)?.into_iter().next())
    }

    /// All letters of one type, newest first.
    pub fn documents_of_type(&self, doc_type: DocType) -> Result<Vec<DocumentRow>> {
        let c = self.conn();
        let sql =
            format!("SELECT {DOC_COLS} FROM documents d WHERE d.doc_type = ?1 ORDER BY d.created_at DESC, d.id DESC");
        let mut st = c.prepare(&sql)?;
        let rows = st.query_map([doc_type.as_str()], doc_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Inserts the deadline and one reminders row per `remind_at` entry.
    pub fn add_deadline(&self, d: &NewDeadline) -> Result<i64> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        let remind: Vec<String> = d.remind_at.iter().map(|t| ts(*t)).collect();
        tx.execute(
            "INSERT INTO deadlines(title, due, rule_id, doc_id, status, remind_at, why, source)
             VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?6, ?7)",
            params![
                d.title,
                d.due.format("%Y-%m-%d").to_string(),
                d.rule_id,
                d.doc_id,
                serde_json::to_string(&remind)?,
                d.why,
                d.source
            ],
        )?;
        let id = tx.last_insert_rowid();
        for at in &remind {
            tx.execute("INSERT INTO reminders(deadline_id, at) VALUES (?1, ?2)", params![id, at])?;
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn deadline(&self, id: i64) -> Result<Option<Deadline>> {
        let sql = format!("SELECT {DEADLINE_COLS} FROM deadlines dl WHERE dl.id = ?1");
        Ok(self.conn().query_row(&sql, [id], |r| deadline_from_row(r, 0)).optional()?)
    }

    /// status = open, ordered by due.
    pub fn open_deadlines(&self) -> Result<Vec<Deadline>> {
        let c = self.conn();
        let sql = format!("SELECT {DEADLINE_COLS} FROM deadlines dl WHERE dl.status = 'open' ORDER BY dl.due, dl.id");
        let mut st = c.prepare(&sql)?;
        let rows = st.query_map([], |r| deadline_from_row(r, 0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// All deadlines (any status) that came from one letter, ordered by due.
    pub fn deadlines_for_document(&self, doc_id: i64) -> Result<Vec<Deadline>> {
        let c = self.conn();
        let sql = format!("SELECT {DEADLINE_COLS} FROM deadlines dl WHERE dl.doc_id = ?1 ORDER BY dl.due, dl.id");
        let mut st = c.prepare(&sql)?;
        let rows = st.query_map([doc_id], |r| deadline_from_row(r, 0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_deadline_status(&self, id: i64, status: DeadlineStatus) -> Result<()> {
        let n = self.conn().execute("UPDATE deadlines SET status = ?1 WHERE id = ?2", params![status.as_str(), id])?;
        if n == 0 {
            bail!("no deadline with id {id}");
        }
        Ok(())
    }

    /// Extra reminder (e.g. "Erinnere mich Montag"). Also appended to the deadline's remind_at.
    pub fn add_reminder(&self, deadline_id: i64, at: DateTime<Local>) -> Result<i64> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        let remind: String = tx
            .query_row("SELECT remind_at FROM deadlines WHERE id = ?1", [deadline_id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| anyhow!("no deadline with id {deadline_id}"))?;
        let mut list: Vec<String> = serde_json::from_str(&remind).unwrap_or_default();
        list.push(ts(at));
        tx.execute(
            "UPDATE deadlines SET remind_at = ?1 WHERE id = ?2",
            params![serde_json::to_string(&list)?, deadline_id],
        )?;
        tx.execute("INSERT INTO reminders(deadline_id, at) VALUES (?1, ?2)", params![deadline_id, ts(at)])?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    /// One reminder with its deadline (fired or not).
    pub fn reminder(&self, reminder_id: i64) -> Result<Option<Reminder>> {
        let sql = format!(
            "SELECT r.id, r.at, {DEADLINE_COLS} FROM reminders r JOIN deadlines dl ON dl.id = r.deadline_id WHERE r.id = ?1"
        );
        Ok(self
            .conn()
            .query_row(&sql, [reminder_id], |r| {
                Ok(Reminder {
                    id: r.get(0)?,
                    at: parse_ts(&r.get::<_, String>(1)?)?,
                    deadline: deadline_from_row(r, 2)?,
                })
            })
            .optional()?)
    }

    /// fired_at IS NULL AND at <= now AND deadline status = open, ordered by at.
    pub fn due_reminders(&self, now: DateTime<Local>) -> Result<Vec<Reminder>> {
        let c = self.conn();
        let sql = format!(
            "SELECT r.id, r.at, {DEADLINE_COLS} FROM reminders r JOIN deadlines dl ON dl.id = r.deadline_id
             WHERE r.fired_at IS NULL AND r.at <= ?1 AND dl.status = 'open' ORDER BY r.at, r.id"
        );
        let mut st = c.prepare(&sql)?;
        let rows = st
            .query_map([ts(now)], |r| {
                Ok(Reminder {
                    id: r.get(0)?,
                    at: parse_ts(&r.get::<_, String>(1)?)?,
                    deadline: deadline_from_row(r, 2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The next reminder that has not rung yet (open deadlines only), for the tray tooltip.
    pub fn next_reminder_at(&self, now: DateTime<Local>) -> Result<Option<DateTime<Local>>> {
        let at: Option<String> = self
            .conn()
            .query_row(
                "SELECT r.at FROM reminders r JOIN deadlines dl ON dl.id = r.deadline_id
                 WHERE r.fired_at IS NULL AND r.at > ?1 AND dl.status = 'open' ORDER BY r.at LIMIT 1",
                [ts(now)],
                |r| r.get(0),
            )
            .optional()?;
        at.map(|s| parse_ts(&s).map_err(anyhow::Error::from)).transpose()
    }

    /// Sets fired_at once (a second call keeps the first time); `answer` is stored if given.
    pub fn mark_fired(&self, reminder_id: i64, now: DateTime<Local>, answer: Option<&str>) -> Result<()> {
        let n = self.conn().execute(
            "UPDATE reminders SET fired_at = COALESCE(fired_at, ?1), answer = COALESCE(?2, answer) WHERE id = ?3",
            params![ts(now), answer, reminder_id],
        )?;
        if n == 0 {
            bail!("no reminder with id {reminder_id}");
        }
        Ok(())
    }

    /// Marks the unfired reminders of one deadline with `after < at <= until` as fired (with
    /// `answer`, e.g. "replaced"), so they never show. Returns how many.
    pub fn skip_reminders(
        &self,
        deadline_id: i64,
        after: DateTime<Local>,
        until: DateTime<Local>,
        now: DateTime<Local>,
        answer: &str,
    ) -> Result<usize> {
        Ok(self.conn().execute(
            "UPDATE reminders SET fired_at = ?1, answer = ?2
             WHERE deadline_id = ?3 AND fired_at IS NULL AND at > ?4 AND at <= ?5",
            params![ts(now), answer, deadline_id, ts(after), ts(until)],
        )?)
    }

    /// Set the answer of an already fired reminder.
    pub fn set_reminder_answer(&self, reminder_id: i64, answer: &str) -> Result<()> {
        let n = self.conn().execute("UPDATE reminders SET answer = ?1 WHERE id = ?2", params![answer, reminder_id])?;
        if n == 0 {
            bail!("no reminder with id {reminder_id}");
        }
        Ok(())
    }

    pub fn add_event(&self, kind: &str, text: &str) -> Result<()> {
        self.conn()
            .execute("INSERT INTO events(at, kind, text) VALUES (?1, ?2, ?3)", params![ts(Local::now()), kind, text])?;
        Ok(())
    }

    /// Same as add_event with a given time (demo profile: events of earlier days).
    pub fn add_event_at(&self, at: DateTime<Local>, kind: &str, text: &str) -> Result<()> {
        self.conn().execute("INSERT INTO events(at, kind, text) VALUES (?1, ?2, ?3)", params![ts(at), kind, text])?;
        Ok(())
    }

    /// Newest first.
    pub fn recent_events(&self, n: u32) -> Result<Vec<Event>> {
        let c = self.conn();
        let mut st = c.prepare("SELECT id, at, kind, text FROM events ORDER BY at DESC, id DESC LIMIT ?1")?;
        let rows = st
            .query_map([n], |r| {
                Ok(Event { id: r.get(0)?, at: parse_ts(&r.get::<_, String>(1)?)?, kind: r.get(2)?, text: r.get(3)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn log_action(&self, a: &ActionLog) -> Result<()> {
        self.conn().execute(
            "INSERT INTO actions_log(at, tool, args_redacted, risk, approved, result, tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![ts(a.at), a.tool, a.args_redacted, a.risk, a.approved, a.result, a.tokens as i64],
        )?;
        Ok(())
    }

    /// The last `n` actions_log rows, oldest first (CLI dry run, eval).
    pub fn recent_actions(&self, n: u32) -> Result<Vec<ActionLog>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT at, tool, args_redacted, risk, approved, result, tokens FROM actions_log
             ORDER BY id DESC LIMIT ?1",
        )?;
        let mut rows = st
            .query_map([n], |r| {
                Ok(ActionLog {
                    at: parse_ts(&r.get::<_, String>(0)?)?,
                    tool: r.get(1)?,
                    args_redacted: r.get(2)?,
                    risk: r.get(3)?,
                    approved: r.get(4)?,
                    result: r.get(5)?,
                    tokens: r.get::<_, i64>(6)?.max(0) as u64,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// Sum of actions_log.tokens since local midnight.
    pub fn tokens_today(&self) -> Result<u64> {
        self.tokens_since(local_midnight(Local::now()))
    }

    /// Sum of actions_log.tokens with at >= since.
    pub fn tokens_since(&self, since: DateTime<Local>) -> Result<u64> {
        let sum: i64 = self.conn().query_row(
            "SELECT COALESCE(SUM(tokens), 0) FROM actions_log WHERE at >= ?1",
            [ts(since)],
            |r| r.get(0),
        )?;
        Ok(sum.max(0) as u64)
    }
}

fn local_midnight(now: DateTime<Local>) -> DateTime<Local> {
    let naive = now.date_naive().and_hms_opt(0, 0, 0).expect("midnight");
    Local.from_local_datetime(&naive).earliest().unwrap_or(now)
}

#[cfg(test)]
mod tests;
