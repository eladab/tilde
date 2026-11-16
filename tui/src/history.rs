//! Browsing history: every page visited, in every space, newest last on disk
//! and newest first on screen.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The most visits kept; the oldest are dropped beyond this.
pub const LIMIT: usize = 10_000;
/// A visit this recent is listed under "Recent".
const RECENT_SECS: u64 = 3600;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Visit {
    pub url: String,
    #[serde(default)]
    pub title: String,
    /// Seconds since 1970.
    pub time: u64,
    /// Name of the space the page was visited in.
    #[serde(default)]
    pub space: String,
}

/// The heading a visit is listed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Recent,
    EarlierToday,
    Yesterday,
    /// Local days since 1970.
    Day(i64),
}

/// One line of the history view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Header(String),
    /// Index of the visit, counting from the newest.
    Item(usize),
}

#[derive(Debug, Default)]
pub struct History {
    /// Oldest first.
    visits: Vec<Visit>,
    pub dirty: bool,
}

/// Pages worth remembering: not blank pages, not `data:` payloads.
pub fn recordable(url: &str) -> bool {
    let scheme = url.split_once(':').map(|(s, _)| s.to_ascii_lowercase()).unwrap_or_default();
    matches!(scheme.as_str(), "http" | "https" | "file")
}

/// The time as local wall-clock seconds, for telling days and hours apart.
#[cfg(not(test))]
pub fn local(time: u64) -> i64 {
    let t = time as libc::time_t;
    // SAFETY: `tm` is plain data and `localtime_r` only writes into it.
    let offset = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm.tm_gmtoff as i64
    };
    time as i64 + offset
}

/// Tests run in UTC whatever the machine is set to.
#[cfg(test)]
pub fn local(time: u64) -> i64 {
    time as i64
}

fn day_of(time: u64) -> i64 {
    local(time).div_euclid(86_400)
}

/// Year, month (1-12) and day of a count of days since 1970.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// "14:05"
pub fn clock(time: u64) -> String {
    let secs = local(time).rem_euclid(86_400);
    format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

/// "14 Nov"
pub fn short_date(time: u64) -> String {
    let (_, month, day) = civil(day_of(time));
    format!("{day} {}", &MONTHS[month as usize - 1][..3])
}

pub fn is_today(time: u64, now: u64) -> bool {
    day_of(time) == day_of(now)
}

pub fn section(time: u64, now: u64) -> Section {
    let (day, today) = (day_of(time), day_of(now));
    if time + RECENT_SECS > now {
        Section::Recent
    } else if day == today {
        Section::EarlierToday
    } else if day == today - 1 {
        Section::Yesterday
    } else {
        Section::Day(day)
    }
}

pub fn section_title(section: Section, now: u64) -> String {
    match section {
        Section::Recent => "Recent".into(),
        Section::EarlierToday => "Earlier today".into(),
        Section::Yesterday => "Yesterday".into(),
        Section::Day(days) => {
            let (year, month, day) = civil(days);
            let weekday = WEEKDAYS[(days + 4).rem_euclid(7) as usize];
            let month = MONTHS[month as usize - 1];
            if year == civil(day_of(now)).0 {
                format!("{weekday}, {day} {month}")
            } else {
                format!("{weekday}, {day} {month} {year}")
            }
        }
    }
}

impl History {
    pub fn len(&self) -> usize {
        self.visits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.visits.is_empty()
    }

    /// The visit at `index`, counting from the newest.
    pub fn get(&self, index: usize) -> Option<&Visit> {
        self.visits.len().checked_sub(index + 1).and_then(|i| self.visits.get(i))
    }

    /// Records a page. A page seen again on the same day moves to the top
    /// rather than being listed twice.
    pub fn visit(&mut self, url: &str, title: &str, space: &str, now: u64) {
        if !recordable(url) {
            return;
        }
        let today = day_of(now);
        let earlier = self.visits.iter().rposition(|v| v.url == url).filter(|&i| day_of(self.visits[i].time) == today);
        let mut title = title.to_string();
        if let Some(index) = earlier {
            let old = self.visits.remove(index);
            if title.is_empty() {
                title = old.title;
            }
        }
        self.visits.push(Visit { url: url.to_string(), title, time: now, space: space.to_string() });
        if self.visits.len() > LIMIT {
            let extra = self.visits.len() - LIMIT;
            self.visits.drain(..extra);
        }
        self.dirty = true;
    }

    /// Gives the latest visit of `url` the title its page turned out to have.
    pub fn retitle(&mut self, url: &str, title: &str) {
        if title.is_empty() {
            return;
        }
        if let Some(visit) = self.visits.iter_mut().rev().take(50).find(|v| v.url == url) {
            if visit.title != title {
                visit.title = title.to_string();
                self.dirty = true;
            }
        }
    }

    /// The page at `old` turned out to live at `new` (it redirected, or
    /// rewrote its own address): the visit follows it rather than doubling.
    pub fn readdress(&mut self, old: &str, new: &str, space: &str, now: u64) {
        let Some(at) = self.visits.iter().rposition(|v| v.url == old).filter(|&i| self.visits.len() - i <= 50) else {
            // Nothing to move, for instance because `old` was not worth recording.
            return self.visit(new, "", space, now);
        };
        if !recordable(new) {
            self.visits.remove(at);
            self.dirty = true;
            return;
        }
        let day = day_of(self.visits[at].time);
        self.visits[at].url = new.to_string();
        // Keep the rule that a page is listed once a day.
        if let Some(twin) = (0..self.visits.len()).rev().find(|&i| i != at && self.visits[i].url == new && day_of(self.visits[i].time) == day) {
            self.visits.remove(twin);
        }
        self.dirty = true;
    }

    /// Forgets the visit at `index`, counting from the newest.
    pub fn remove(&mut self, index: usize) {
        if let Some(at) = self.visits.len().checked_sub(index + 1) {
            self.visits.remove(at);
            self.dirty = true;
        }
    }

    pub fn clear(&mut self) {
        if !self.visits.is_empty() {
            self.visits.clear();
            self.dirty = true;
        }
    }

    /// Keeps visits pointing at a space that was given a new name.
    pub fn rename_space(&mut self, old: &str, new: &str) {
        if old == new {
            return;
        }
        for visit in self.visits.iter_mut().filter(|v| v.space == old) {
            visit.space = new.to_string();
            self.dirty = true;
        }
    }

    /// The lines of the history view: newest first, a heading before each group.
    pub fn rows(&self, now: u64) -> Vec<Row> {
        let mut rows = Vec::with_capacity(self.visits.len() + 8);
        let mut last = None;
        for (index, visit) in self.visits.iter().rev().enumerate() {
            let group = section(visit.time, now);
            if last != Some(group) {
                rows.push(Row::Header(section_title(group, now)));
                last = Some(group);
            }
            rows.push(Row::Item(index));
        }
        rows
    }

    pub fn load(path: &Path) -> Self {
        let visits = fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        Self { visits, dirty: false }
    }

    pub fn save(&mut self, path: &Path) -> io::Result<()> {
        self.dirty = false;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec(&self.visits)?)?;
        fs::rename(&tmp, path)
    }
}

/// The history file that goes with a session file: `history.json` beside
/// `session.json`, and a name of its own beside any other session.
pub fn path_beside(session: &Path) -> PathBuf {
    let stem = session.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = if stem == "session" || stem.is_empty() { "history.json".to_string() } else { format!("{stem}.history.json") };
    session.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;
    /// Saturday 14 November 2026, 15:00 UTC.
    const NOW: u64 = 1_794_668_400;

    #[test]
    fn dates_and_clock() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(day_of(NOW)), (2026, 11, 14));
        assert_eq!(clock(NOW + 5 * 60), "15:05");
        assert_eq!(short_date(NOW), "14 Nov");
        assert_eq!(section_title(Section::Day(day_of(NOW - 3 * DAY)), NOW), "Wednesday, 11 November");
        assert_eq!(section_title(Section::Day(day_of(NOW - 400 * DAY)), NOW), "Friday, 10 October 2025");
    }

    #[test]
    fn visits_are_grouped_newest_first() {
        let mut history = History::default();
        history.visit("https://old.example/", "Old", "Home", NOW - 3 * DAY);
        history.visit("https://yesterday.example/", "Y", "Home", NOW - DAY);
        history.visit("https://morning.example/", "M", "Work", NOW - 5 * 3600);
        history.visit("https://now.example/", "N", "Work", NOW - 60);
        assert_eq!(
            history.rows(NOW),
            vec![
                Row::Header("Recent".into()),
                Row::Item(0),
                Row::Header("Earlier today".into()),
                Row::Item(1),
                Row::Header("Yesterday".into()),
                Row::Item(2),
                Row::Header("Wednesday, 11 November".into()),
                Row::Item(3),
            ]
        );
        assert_eq!(history.get(0).unwrap().url, "https://now.example/");
        assert_eq!(history.get(3).unwrap().url, "https://old.example/");
        assert_eq!(history.get(4), None);
    }

    #[test]
    fn a_page_seen_again_the_same_day_is_listed_once() {
        let mut history = History::default();
        history.visit("https://a.example/", "A", "Home", NOW - DAY);
        history.visit("https://a.example/", "A", "Home", NOW - 7200);
        history.visit("https://b.example/", "B", "Home", NOW - 3600);
        history.visit("https://a.example/", "", "Work", NOW);
        assert_eq!(history.len(), 3, "yesterday's visit stays, today's moves up");
        let top = history.get(0).unwrap();
        assert_eq!((top.url.as_str(), top.title.as_str(), top.space.as_str(), top.time), ("https://a.example/", "A", "Work", NOW));
        history.visit("about:blank", "", "Home", NOW);
        history.visit("data:text/html,x", "", "Home", NOW);
        assert_eq!(history.len(), 3, "blank and data pages are not history");
    }

    #[test]
    fn a_readdressed_page_is_still_one_visit() {
        let mut history = History::default();
        history.visit("https://a.example/", "A", "Home", NOW - 100);
        history.visit("https://search.example/?q=x", "x", "Home", NOW - 10);
        history.readdress("https://search.example/?q=x", "https://search.example/?q=x&ia=web", "Home", NOW);
        assert_eq!(history.len(), 2);
        let top = history.get(0).unwrap();
        assert_eq!((top.url.as_str(), top.title.as_str(), top.time), ("https://search.example/?q=x&ia=web", "x", NOW - 10));
        // Landing on a page already seen today leaves one line for it.
        history.readdress("https://search.example/?q=x&ia=web", "https://a.example/", "Home", NOW);
        assert_eq!(history.len(), 1);
        assert_eq!(history.get(0).unwrap().url, "https://a.example/");
    }

    #[test]
    fn retitle_remove_rename_and_clear() {
        let mut history = History::default();
        history.visit("https://a.example/", "", "Home", NOW - 10);
        history.visit("https://b.example/", "B", "Home", NOW);
        history.dirty = false;
        history.retitle("https://a.example/", "A");
        assert_eq!(history.get(1).unwrap().title, "A");
        assert!(history.dirty);
        history.rename_space("Home", "Life");
        assert_eq!(history.get(0).unwrap().space, "Life");
        history.remove(0);
        assert_eq!(history.get(0).unwrap().url, "https://a.example/");
        history.clear();
        assert!(history.is_empty());
    }

    #[test]
    fn history_round_trips_and_sits_beside_the_session() {
        assert_eq!(path_beside(Path::new("/x/session.json")), PathBuf::from("/x/history.json"));
        assert_eq!(path_beside(Path::new("/x/pins.json")), PathBuf::from("/x/pins.history.json"));
        let path = std::env::temp_dir().join(format!("tilde-history-{}.json", std::process::id()));
        let mut history = History::default();
        history.visit("https://a.example/", "A", "Home", NOW);
        history.save(&path).unwrap();
        let loaded = History::load(&path);
        assert_eq!(loaded.get(0), history.get(0));
        let _ = fs::remove_file(&path);
    }
}
