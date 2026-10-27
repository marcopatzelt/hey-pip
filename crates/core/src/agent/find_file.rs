//! The user's answer to "Where is your boarding pass?": a file, a folder ("in my Downloads"), or no file. In a
//! folder the file is found by its name: the words of the upload button and the task's codes (flight number,
//! booking reference). The search stays inside that folder and its direct subfolders.
//!
//! OWNER: agent "agent" (crates/core/src/agent/**).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What the user's answer names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileAnswer {
    /// A file ("C:\Users\…\boarding_pass.pdf", "boarding_pass.pdf").
    Path(PathBuf),
    /// A folder: an absolute path, or a folder's everyday name under the user's home ("in my Downloads").
    Folder(PathBuf),
    /// No file ("I don't have it", "no", "not on this computer"), or nothing this module understands.
    None,
}

/// Everyday folder names (English and German) -> the folder's name in the user's home.
const FOLDER_WORDS: &[(&str, &str)] = &[
    ("downloads", "Downloads"),
    ("download", "Downloads"),
    ("desktop", "Desktop"),
    ("schreibtisch", "Desktop"),
    ("documents", "Documents"),
    ("dokumente", "Documents"),
    ("pictures", "Pictures"),
    ("bilder", "Pictures"),
];

/// Answers that say there is no file.
const NO_FILE: &[&str] = &[
    "no",
    "nope",
    "nein",
    "i don't have it",
    "i dont have it",
    "i do not have it",
    "don't have",
    "dont have",
    "do not have",
    "not on this computer",
    "habe ich nicht",
    "hab ich nicht",
    "nicht auf diesem computer",
    "keine ahnung",
    "i don't know",
    "i dont know",
];

/// File types an upload of a letter or a pass takes.
const FILE_TYPES: &[&str] = &["pdf", "jpg", "jpeg", "png"];

/// Words of an answer that are no folder's own name ("in my letters folder" -> "letters").
const FILLER: &[&str] =
    &["in", "im", "my", "the", "a", "on", "it", "is", "its", "it's", "mein", "meinem", "meine", "der", "dem", "den"];

/// What the answer names. `home` is the user's home folder (Windows: %USERPROFILE%).
pub fn parse_answer(answer: &str, home: &Path) -> FileAnswer {
    let text = answer.trim().trim_matches(['"', '\'', '„', '“', '”']).trim().trim_end_matches(['.', '!']).trim();
    if text.is_empty() {
        return FileAnswer::None;
    }
    let low = text.to_lowercase();
    if NO_FILE.iter().any(|n| low == *n || (n.contains(' ') && low.contains(n))) {
        return FileAnswer::None;
    }
    let pathlike = text.contains(['/', '\\']) || is_drive(text);
    if has_file_type(text) && (pathlike || !text.contains(' ')) {
        return FileAnswer::Path(PathBuf::from(text));
    }
    if let Some(rest) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
        return FileAnswer::Folder(home.join(rest));
    }
    if text.starts_with('/') || text.starts_with('\\') || is_drive(text) {
        return FileAnswer::Folder(PathBuf::from(text));
    }
    let words: Vec<&str> = low.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    if let Some((_, name)) = words.iter().find_map(|w| FOLDER_WORDS.iter().find(|(k, _)| k == w)) {
        return FileAnswer::Folder(home.join(name));
    }
    // "in my letters folder" / "im Ordner Briefe": a folder of that name right in the home or in Documents.
    let named = words.iter().enumerate().find_map(|(i, w)| match *w {
        "folder" => words[..i].iter().rev().find(|w| !FILLER.contains(w)).copied(),
        "ordner" => words[i + 1..].iter().find(|w| !FILLER.contains(w)).copied(),
        _ => None,
    });
    if let Some(name) = named {
        for base in [home.to_path_buf(), home.join("Documents")] {
            if let Some(dir) = child_dir(&base, name) {
                return FileAnswer::Folder(dir);
            }
        }
    }
    FileAnswer::None
}

/// "C:\…" or "C:/…".
fn is_drive(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'/' | b'\\')
}

/// The text ends in a file name with a short extension ("pass.pdf"), not a sentence's full stop.
fn has_file_type(text: &str) -> bool {
    let name = text.rsplit(['/', '\\']).next().unwrap_or(text);
    name.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty() && (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric())
    })
}

/// A direct subfolder of `base` named `name` (any case). Never follows a link.
fn child_dir(base: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(base).ok()?.flatten().find_map(|e| {
        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
        (is_dir && e.file_name().to_string_lossy().eq_ignore_ascii_case(name)).then(|| e.path())
    })
}

/// How many of `words` (lowercase) a file's name holds: a short word as a whole part of the name
/// ("pass" in "boarding_pass", not in "passport"), a long one (6+ letters: "boarding", "bx2041") anywhere in it.
pub fn name_hits(path: &Path, words: &[String]) -> usize {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let parts: Vec<&str> = stem.split(|c: char| !c.is_alphanumeric()).filter(|p| !p.is_empty()).collect();
    let compact: String = stem.chars().filter(|c| c.is_alphanumeric()).collect();
    words
        .iter()
        .filter(|w| !w.is_empty())
        .filter(|w| parts.contains(&w.as_str()) || (w.chars().count() >= 6 && compact.contains(w.as_str())))
        .count()
}

/// The pdf/jpg/png files in `folder` and its direct subfolders whose name holds at least one of `words`: most
/// hits first, then the newest. At most 5. Links are never followed, so the search stays in the folder.
pub fn find_in(folder: &Path, words: &[String]) -> Vec<PathBuf> {
    let mut found: Vec<(usize, SystemTime, PathBuf)> = Vec::new();
    let mut look = |dir: &Path, subdirs: &mut Vec<PathBuf>| {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let Ok(kind) = e.file_type() else { continue };
            let path = e.path();
            if kind.is_dir() {
                subdirs.push(path);
                continue;
            }
            let wanted = path
                .extension()
                .is_some_and(|x| FILE_TYPES.iter().any(|t| x.to_string_lossy().eq_ignore_ascii_case(t)));
            if !kind.is_file() || !wanted {
                continue;
            }
            let hits = name_hits(&path, words);
            if hits > 0 {
                let modified = e.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                found.push((hits, modified, path));
            }
        }
    };
    let mut subdirs = Vec::new();
    look(folder, &mut subdirs);
    for dir in subdirs {
        look(&dir, &mut Vec::new());
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    found.into_iter().take(5).map(|(_, _, p)| p).collect()
}

/// Codes in a text that a file name may carry: a flight number ("BX 2041" -> "bx2041") and references that mix
/// letters and digits ("BX7Q2K" -> "bx7q2k").
pub fn code_words(text: &str) -> Vec<String> {
    let tokens: Vec<String> =
        text.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).map(str::to_lowercase).collect();
    let mut out: Vec<String> = Vec::new();
    let mut push = |w: String| {
        if !out.contains(&w) {
            out.push(w);
        }
    };
    for (i, t) in tokens.iter().enumerate() {
        let letters = t.chars().all(|c| c.is_ascii_alphabetic());
        if letters
            && t.len() == 2
            && let Some(n) = tokens.get(i + 1)
            && (1..=4).contains(&n.len())
            && n.chars().all(|c| c.is_ascii_digit())
        {
            push(format!("{t}{n}"));
        }
        let mixed = t.chars().any(|c| c.is_ascii_digit()) && t.chars().any(|c| c.is_ascii_alphabetic());
        if mixed && t.len() >= 5 {
            push(t.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pip-find-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn answers_name_a_file_a_folder_or_nothing() {
        let home = Path::new("/home/erika");
        let folder = |n: &str| FileAnswer::Folder(home.join(n));
        let table: &[(&str, FileAnswer)] = &[
            (
                "C:\\Users\\Erika\\Downloads\\boarding_pass.pdf",
                FileAnswer::Path("C:\\Users\\Erika\\Downloads\\boarding_pass.pdf".into()),
            ),
            ("/home/erika/pass.jpg", FileAnswer::Path("/home/erika/pass.jpg".into())),
            ("boarding_pass_bx2041.pdf", FileAnswer::Path("boarding_pass_bx2041.pdf".into())),
            ("in my Downloads", folder("Downloads")),
            ("In my Downloads.", folder("Downloads")),
            ("downloads", folder("Downloads")),
            ("on the desktop", folder("Desktop")),
            ("Documents", folder("Documents")),
            ("Downloads", folder("Downloads")),
            ("auf dem Schreibtisch", folder("Desktop")),
            ("in Dokumente", folder("Documents")),
            ("C:\\Users\\Erika\\Downloads", FileAnswer::Folder("C:\\Users\\Erika\\Downloads".into())),
            ("/home/erika/letters", FileAnswer::Folder("/home/erika/letters".into())),
            ("~/letters", folder("letters")),
            ("I don't have it", FileAnswer::None),
            ("no", FileAnswer::None),
            ("Nein", FileAnswer::None),
            ("It's not on this computer", FileAnswer::None),
            ("", FileAnswer::None),
            ("somewhere", FileAnswer::None),
        ];
        for (answer, want) in table {
            assert_eq!(&parse_answer(answer, home), want, "{answer}");
        }
    }

    #[test]
    fn a_named_folder_is_found_in_the_home() {
        let home = temp("home");
        std::fs::create_dir_all(home.join("Letters")).unwrap();
        std::fs::create_dir_all(home.join("Documents").join("Briefe")).unwrap();
        assert_eq!(parse_answer("in my letters folder", &home), FileAnswer::Folder(home.join("Letters")));
        assert_eq!(parse_answer("im Ordner Briefe", &home), FileAnswer::Folder(home.join("Documents").join("Briefe")));
        assert_eq!(parse_answer("in my travel folder", &home), FileAnswer::None);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_boarding_pass_is_found_first() {
        let dir = temp("downloads");
        for f in ["boarding_pass_bx2041.pdf", "au_erika_erst.pdf", "holiday.jpg", "passport.jpg", "notes.txt"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        std::fs::create_dir_all(dir.join("trip").join("deeper")).unwrap();
        std::fs::write(dir.join("trip").join("bx2041_ticket.png"), b"x").unwrap();
        std::fs::write(dir.join("trip").join("deeper").join("boarding_pass_old.pdf"), b"x").unwrap();
        let words: Vec<String> = ["boarding", "pass", "bx2041", "bx7q2k"].iter().map(|w| w.to_string()).collect();
        let found = find_in(&dir, &words);
        assert_eq!(found[0], dir.join("boarding_pass_bx2041.pdf"));
        // One level down counts, two levels down and other file types do not; "pass" is no part of "passport".
        assert_eq!(found, [dir.join("boarding_pass_bx2041.pdf"), dir.join("trip").join("bx2041_ticket.png")]);
        assert!(find_in(&dir, &["invoice".to_string()]).is_empty());
        assert!(find_in(&dir.join("missing"), &words).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn codes_of_a_task() {
        assert_eq!(code_words("Claim EUR 400 for flight BX 2041, booking BX7Q2K"), ["bx2041", "bx7q2k"]);
        assert_eq!(code_words("flight BX2041"), ["bx2041"]);
        assert!(code_words("Send my claim").is_empty());
    }
}
