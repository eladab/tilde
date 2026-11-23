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

/// What the address bar offers to finish a typed address with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// The text that follows what was typed.
    pub rest: String,
    /// The address the completed text stands for.
    pub url: String,
}

/// An address the way it is typed: without its scheme and `www.`.
fn bare(url: &str) -> Option<&str> {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    let after = if lower.starts_with("https://") {
        &url[8..]
    } else if lower.starts_with("http://") {
        &url[7..]
    } else {
        return None;
    };
    let www = after.get(..4).is_some_and(|w| w.eq_ignore_ascii_case("www."));
    Some(if www { &after[4..] } else { after })
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

/// "14 Nov 2026"
pub fn full_date(time: u64) -> String {
    let (year, month, day) = civil(day_of(time));
    format!("{day} {} {year}", &MONTHS[month as usize - 1][..3])
}

/// "14:05:09"
pub fn clock_seconds(time: u64) -> String {
    let secs = local(time).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", secs / 3600, secs % 3600 / 60, secs % 60)
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

    /// Finishes a typed address from the pages visited before. Until a path
    /// is typed the offer is a site; after that it is a page of that site.
    /// Of several, the one visited most wins, then the one visited last.
    pub fn complete(&self, typed: &str) -> Option<Completion> {
        if typed.contains(char::is_whitespace) {
            return None;
        }
        let typed = bare(typed).unwrap_or_else(|| bare(&format!("https://{typed}")).map_or(typed, |b| &typed[typed.len() - b.len()..]));
        if typed.is_empty() {
            return None;
        }
        let whole_page = typed.contains(['/', '?', '#']);
        // Per candidate: how often it was visited, and the latest such visit.
        let mut found: std::collections::HashMap<String, (usize, usize)> = std::collections::HashMap::new();
        for (index, visit) in self.visits.iter().enumerate() {
            let Some(address) = bare(&visit.url) else { continue };
            if !address.get(..typed.len()).is_some_and(|start| start.eq_ignore_ascii_case(typed)) {
                continue;
            }
            let end = if whole_page { address.len() } else { address.find(['/', '?', '#']).unwrap_or(address.len()) };
            if end <= typed.len() {
                continue;
            }
            let entry = found.entry(address[..end].to_ascii_lowercase()).or_insert((0, index));
            *entry = (entry.0 + 1, index);
        }
        let (_, &(_, latest)) = found.iter().max_by_key(|(_, score)| **score)?;
        let url = &self.visits[latest].url;
        let address = bare(url)?;
        let end = if whole_page { address.len() } else { address.find(['/', '?', '#']).unwrap_or(address.len()) };
        let url = if whole_page { url.clone() } else { format!("{}/", &url[..url.len() - address.len() + end]) };
        Some(Completion { rest: address[typed.len()..end].to_string(), url })
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

    /// Every visit, newest first.
    pub fn newest_first(&self) -> impl Iterator<Item = &Visit> {
        self.visits.iter().rev()
    }

    /// The lines of the history view: newest first, a heading before each
    /// group. With a `filter`, only the visits whose title or address holds
    /// every word of it.
    pub fn rows(&self, now: u64, filter: &str) -> Vec<Row> {
        let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
        let mut rows = Vec::with_capacity(if words.is_empty() { self.visits.len() + 8 } else { 64 });
        let mut last = None;
        for (index, visit) in self.visits.iter().rev().enumerate() {
            if !words.is_empty() {
                let text = format!("{} {}", visit.title, visit.url).to_lowercase();
                if !words.iter().all(|word| text.contains(word.as_str())) {
                    continue;
                }
            }
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
    fn completes_typed_addresses() {
        let mut history = History::default();
        for (i, url) in [
            "https://github.com/",
            "https://www.github.com/rust-lang/rust",
            "https://github.com/rust-lang/cargo",
            "https://github.com/rust-lang/rust",
            "https://gitlab.com/",
            "http://plain.example/page?x=1",
            "file:///tmp/gift.html",
        ]
        .iter()
        .enumerate()
        {
            history.visit(url, "", "Home", NOW + i as u64 * DAY);
        }
        let complete = |typed: &str| history.complete(typed).map(|c| (c.rest, c.url));
        // The site visited most, though another was visited later.
        assert_eq!(complete("gi"), Some(("thub.com".into(), "https://github.com/".into())));
        assert_eq!(complete("GitL"), Some(("ab.com".into(), "https://gitlab.com/".into())));
        assert_eq!(complete("www.gith"), Some(("ub.com".into(), "https://github.com/".into())));
        assert_eq!(complete("https://gith"), Some(("ub.com".into(), "https://github.com/".into())));
        // With a path typed, a page: here the one seen twice.
        assert_eq!(complete("github.com/r"), Some(("ust-lang/rust".into(), "https://github.com/rust-lang/rust".into())));
        // A site keeps the scheme it was visited with.
        assert_eq!(complete("pl"), Some(("ain.example".into(), "http://plain.example/".into())));
        assert_eq!(complete("plain.example/"), Some(("page?x=1".into(), "http://plain.example/page?x=1".into())));
        // Nothing to add, nothing known, or not an address at all.
        assert_eq!(complete("gitlab.com"), None);
        assert_eq!(complete("zzz"), None);
        assert_eq!(complete("git hub"), None);
        assert_eq!(complete("gif"), None);
        assert_eq!(complete(""), None);
        assert_eq!(complete("https://"), None);
    }

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
            history.rows(NOW, ""),
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
