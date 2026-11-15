//! The session file: which tabs exist, in what order, and which is selected.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionTab {
    pub url: String,
    #[serde(default)]
    pub title: String,
    /// The app-side tab, meaningful only while `Session::instance` is still running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// Identifies the app process that the `tab_id`s belong to.
    #[serde(default)]
    pub instance: Option<u64>,
    #[serde(default)]
    pub active: usize,
    #[serde(default)]
    pub tabs: Vec<SessionTab>,
}

pub fn default_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("WEFT_SESSION") {
        return Some(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/weft/session.json"))
}

pub fn load(path: &Path) -> Option<Session> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Writes to a temporary file and renames it, so a crash never leaves half a session.
pub fn save(path: &Path, session: &Session) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(session)?)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("weft-session-test-{}", std::process::id()));
        let path = dir.join("nested/session.json");
        let session = Session {
            instance: Some(42),
            active: 1,
            tabs: vec![
                SessionTab { url: "https://a.example".into(), title: "A".into(), tab_id: Some(3) },
                SessionTab { url: "https://b.example".into(), title: "".into(), tab_id: None },
            ],
        };
        save(&path, &session).unwrap();
        assert_eq!(load(&path), Some(session));
        assert!(!path.with_extension("json.tmp").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_or_corrupt_is_none() {
        let path = std::env::temp_dir().join(format!("weft-missing-{}.json", std::process::id()));
        assert_eq!(load(&path), None);
        fs::write(&path, "{not json").unwrap();
        assert_eq!(load(&path), None);
        fs::remove_file(path).unwrap();
    }
}
