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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    /// Where "return to pinned page" goes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_url: Option<String>,
    /// The page title the tab was pinned under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_title: Option<String>,
    /// A name the user gave the tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A space: its own tabs, colour and profile.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionSpace {
    pub name: String,
    /// Index into the colour palette.
    #[serde(default)]
    pub color: usize,
    /// Index into the background tints; 0 is the plain one.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub background: usize,
    /// Pixel art: 0 for none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub art: usize,
    /// Cookie jar shared by the space's tabs; "default" is the original one.
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default)]
    pub active: usize,
    #[serde(default)]
    pub tabs: Vec<SessionTab>,
}

fn default_profile() -> String {
    "default".to_string()
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// Identifies the app process that the `tab_id`s belong to.
    #[serde(default)]
    pub instance: Option<u64>,
    /// Index of the space that was showing.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub current: usize,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sidebar_hidden: bool,
    /// Sessions written while spaces were called looms use that key.
    #[serde(default, alias = "looms")]
    pub spaces: Vec<SessionSpace>,
    /// Sessions written before spaces existed keep their tabs here; they are
    /// read as a single space and never written again.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub active: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<SessionTab>,
}

impl Session {
    /// The spaces of this session, converting the pre-space format if needed.
    pub fn into_spaces(self) -> Vec<SessionSpace> {
        if !self.spaces.is_empty() || self.tabs.is_empty() {
            return self.spaces;
        }
        vec![SessionSpace { name: "Home".into(), profile: default_profile(), active: self.active, tabs: self.tabs, ..Default::default() }]
    }
}

pub fn default_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("TILDE_SESSION") {
        return Some(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/tilde/session.json"))
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
        let dir = std::env::temp_dir().join(format!("tilde-session-test-{}", std::process::id()));
        let path = dir.join("nested/session.json");
        let session = Session {
            instance: Some(42),
            current: 1,
            sidebar_hidden: true,
            spaces: vec![
                SessionSpace { name: "Home".into(), profile: "default".into(), ..Default::default() },
                SessionSpace {
                    name: "Work".into(),
                    color: 3,
                    background: 2,
                    art: 4,
                    profile: "work".into(),
                    active: 1,
                    tabs: vec![
                SessionTab {
                    url: "https://a.example/now".into(),
                    title: "A".into(),
                    tab_id: Some(3),
                    pinned: true,
                    pin_url: Some("https://a.example".into()),
                    pin_title: Some("A Mail".into()),
                    name: Some("Mail".into()),
                },
                        SessionTab { url: "https://b.example".into(), ..Default::default() },
                    ],
                },
            ],
            ..Default::default()
        };
        save(&path, &session).unwrap();
        assert_eq!(load(&path), Some(session));
        assert!(!path.with_extension("json.tmp").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_sessions_written_before_pinning_existed() {
        let old = r#"{"instance":1,"active":0,"tabs":[{"url":"https://a.example","title":"A"}]}"#;
        let session: Session = serde_json::from_str(old).unwrap();
        assert!(!session.sidebar_hidden);
        assert!(!session.tabs[0].pinned);
        assert_eq!(session.tabs[0].name, None);
        // A session from before spaces becomes one space on the default profile.
        let spaces = session.into_spaces();
        assert_eq!(spaces.len(), 1);
        assert_eq!((spaces[0].name.as_str(), spaces[0].profile.as_str(), spaces[0].tabs.len()), ("Home", "default", 1));
    }

    #[test]
    fn reads_sessions_written_when_spaces_were_called_looms() {
        let old = r#"{"instance":1,"current":1,"looms":[
            {"name":"Home","color":0,"profile":"default","active":0,"tabs":[]},
            {"name":"Work","color":3,"background":2,"art":4,"profile":"work","active":0,"tabs":[{"url":"https://a.example","title":"A","pinned":true}]}]}"#;
        let session: Session = serde_json::from_str(old).unwrap();
        assert_eq!(session.current, 1);
        assert_eq!(session.spaces.len(), 2);
        assert_eq!((session.spaces[1].name.as_str(), session.spaces[1].profile.as_str(), session.spaces[1].art), ("Work", "work", 4));
        assert!(session.spaces[1].tabs[0].pinned);
        // And it is written back under the new key only.
        let text = serde_json::to_string(&session).unwrap();
        assert!(text.contains("\"spaces\"") && !text.contains("looms"));
    }

    #[test]
    fn missing_or_corrupt_is_none() {
        let path = std::env::temp_dir().join(format!("tilde-missing-{}.json", std::process::id()));
        assert_eq!(load(&path), None);
        fs::write(&path, "{not json").unwrap();
        assert_eq!(load(&path), None);
        fs::remove_file(path).unwrap();
    }
}
