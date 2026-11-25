//! The command palette: one field that finds a tab in any space, a page from
//! the history, a closed tab or a command, or opens what was typed.
//!
//! It is drawn over the page. Nothing can really be drawn over a page, so
//! the app is told which cells the palette takes (`overlay.set`) and shows a
//! still of the page with those cells cut out of it.

use crate::app::{contains, App, UrlEdit};
use crate::protocol::{Mode, Request};
use crate::resolve::{display_host, resolve, SEARCH_BASE};
use crate::ui::{fill, mix, put, Palette as Colors};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use std::collections::HashMap;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The most lines the palette lists.
const LIMIT: usize = 40;

/// Commands that can be run by name: the name the app's menu sends, what
/// the palette calls it, and its key.
pub const COMMANDS: &[(&str, &str, &str)] = &[
    ("new-tab", "New tab", "⌘T"),
    ("close-tab", "Close tab", "⌘W"),
    ("reopen-tab", "Reopen closed tab", "⇧⌘T"),
    ("recent-tab", "Switch to the tab used before", "⌃`"),
    ("reload", "Reload page", "⌘R"),
    ("back", "Back", "⌘["),
    ("forward", "Forward", "⌘]"),
    ("find", "Find in page", "⌘F"),
    ("hints", "Follow a link by keyboard", "⌘J"),
    ("hints-new", "Follow a link in a new tab", "⇧⌘J"),
    ("toggle-pin", "Pin or unpin tab", "⌘D"),
    ("pin-home", "Return to pinned page", "⇧⌘H"),
    ("rename-tab", "Rename tab", "n"),
    ("toggle-mute", "Mute or unmute tab", "⌥⌘M"),
    ("toggle-blocking", "Content blocking on or off for this site", ""),
    ("sleep-others", "Put the other tabs to sleep", ""),
    ("history", "History", "⌘Y"),
    ("downloads", "Downloads", "⌥⌘L"),
    ("devtools", "Developer tools", "⌥⌘I"),
    ("toggle-sidebar", "Show or hide sidebar", "⌘B"),
    ("new-space", "New space", "⇧⌘N"),
    ("edit-space", "Edit space", "e"),
    ("delete-space", "Delete space", "D"),
    ("next-space", "Next space", "⌥⌘→"),
    ("prev-space", "Previous space", "⌥⌘←"),
    ("help", "Keyboard shortcuts", "⌘/"),
    ("welcome", "Welcome: a short tour of Tilde", "⇧⌘/"),
    ("quit", "Quit Tilde", "⌘Q"),
];

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Show a tab: the index of its space and the tab's key.
    Tab { space: usize, key: u64 },
    /// Reopen the closed tab at this index of `App::closed`.
    Reopen(usize),
    /// Open a page from the history in a new tab.
    Visit { url: String, title: String },
    Command(&'static str),
    /// Open what was typed, as an address or a search.
    Go(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub action: Action,
    pub title: String,
    /// The site, the space or the key: said after the title, quietly.
    pub detail: String,
    /// What kind of thing it is, at the end of the line.
    pub kind: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandPalette {
    pub edit: UrlEdit,
    pub items: Vec<Item>,
    pub selected: usize,
    pub scroll: usize,
    /// The page had the keys when the palette opened, and gets them back.
    return_to_page: bool,
}

/// How well `words` (lower-case) are found in a title and an address, or
/// `None` when one of them is in neither. Every word must be found; a word
/// at the start of a word counts for more than one inside it, the title for
/// more than the address, and letters merely in order for least.
pub fn score(words: &[String], title: &str, address: &str) -> Option<i64> {
    let title = title.to_lowercase();
    let address = address.to_lowercase();
    let mut total = 0;
    for word in words {
        let found = |text: &str, base: i64| {
            text.find(word.as_str()).map(|at| {
                let boundary = at == 0 || !text[..at].chars().next_back().is_some_and(char::is_alphanumeric);
                base + if boundary { 30 } else { 0 } - (at.min(40) as i64) / 2
            })
        };
        let in_order = |text: &str| {
            let mut letters = word.chars();
            let mut next = letters.next();
            for c in text.chars() {
                if Some(c) == next {
                    next = letters.next();
                }
            }
            next.is_none() && word.chars().count() >= 2
        };
        total += found(&title, 100).max(found(&address, 60)).or_else(|| in_order(&title).then_some(12))?;
    }
    Some(total)
}

impl App {
    pub fn toggle_palette(&mut self) {
        if self.palette.is_some() {
            self.close_palette();
        } else {
            self.open_palette("");
        }
    }

    pub fn open_palette(&mut self, text: &str) {
        self.close_overlays();
        self.cancel_edit();
        self.stop_hints();
        self.message = None;
        let return_to_page = self.mode == Mode::Page;
        self.request(Request::FocusSet { mode: Mode::Nav });
        self.palette = Some(CommandPalette { edit: UrlEdit::holding(text), items: Vec::new(), selected: 0, scroll: 0, return_to_page });
        self.palette_changed();
        self.sync_overlay();
    }

    pub fn close_palette(&mut self) {
        let Some(palette) = self.palette.take() else { return };
        // The page comes back before it is given the keys.
        self.sync_overlay();
        if palette.return_to_page && !self.overlay_open() {
            self.focus_page();
        }
    }

    /// Closes the palette for a command that is about to be carried out,
    /// and says whether the page had the keys before it opened. The keys
    /// are not handed back here: the command may want them.
    pub(crate) fn dismiss_palette(&mut self) -> bool {
        let Some(palette) = self.palette.take() else { return false };
        self.sync_overlay();
        palette.return_to_page
    }

    /// The cells the palette takes: the upper middle of the page area.
    pub fn palette_rect(&self) -> Rect {
        let view = self.layout.viewport;
        let width = 78.min(view.width.saturating_sub(4)).max(view.width.min(24));
        let height = 18.min(view.height.saturating_sub(2)).max(view.height.min(4));
        let top = if view.height >= height + 6 { 2 } else { (view.height - height) / 2 };
        Rect::new(view.x + (view.width - width) / 2, view.y + top, width, height)
    }

    fn palette_list_height(&self) -> usize {
        self.palette_rect().height.saturating_sub(3) as usize
    }

    /// Tells the app which cells are drawn over the page, when that changes.
    pub(crate) fn sync_overlay(&mut self) {
        let wanted = (self.palette.is_some() && self.page_visible()).then(|| self.palette_rect()).filter(|r| r.width > 0 && r.height > 0);
        if wanted == self.overlay_sent {
            return;
        }
        self.overlay_sent = wanted;
        match wanted {
            Some(rect) => self.request(Request::OverlaySet { col: rect.x, row: rect.y, cols: rect.width, rows: rect.height }),
            None => self.request(Request::OverlayClear),
        }
    }

    /// Everything the typed text finds, best first.
    fn palette_items(&self, text: &str) -> Vec<Item> {
        let text = text.trim();
        let commands_only = text.starts_with('>');
        let query = text.trim_start_matches('>').trim();
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut found: Vec<(i64, Item)> = Vec::new();
        let current_key = self.current().map(|t| t.key);

        if !commands_only {
            // Tabs, of this space and of the others.
            for (space, home) in self.spaces.iter().enumerate() {
                let tabs = if space == self.space { &self.tabs } else { &home.tabs };
                for (index, tab) in tabs.iter().enumerate() {
                    if tab.url.is_empty() || Some(tab.key) == current_key {
                        continue;
                    }
                    let Some(points) = score(&words, &tab.label(), &tab.url) else { continue };
                    // With nothing typed the tabs are listed as the sidebar has them.
                    let order = if words.is_empty() { -((space != self.space) as i64 * 1000 + index as i64) } else { 0 };
                    let detail = if space == self.space { display_host(&tab.url) } else { format!("{} · ~/{}", display_host(&tab.url), home.name) };
                    found.push((points + 1000 + order, Item { action: Action::Tab { space, key: tab.key }, title: tab.label(), detail, kind: "tab" }));
                }
            }
            for (index, closed) in self.closed.iter().enumerate().rev() {
                let title = if closed.title.is_empty() { display_host(&closed.url) } else { closed.title.clone() };
                let Some(points) = score(&words, &title, &closed.url) else { continue };
                found.push((points + 300 + index as i64, Item { action: Action::Reopen(index), title, detail: display_host(&closed.url), kind: "closed" }));
            }
            // The history, once something is typed: each page once, the
            // pages gone to most often first.
            if !words.is_empty() {
                let open: Vec<&str> = self.tabs.iter().map(|t| t.url.as_str()).collect();
                let mut pages: HashMap<&str, (i64, usize)> = HashMap::new();
                let mut order: Vec<(&str, &str, i64)> = Vec::new();
                for (age, visit) in self.history.newest_first().enumerate() {
                    if let Some((_, visits)) = pages.get_mut(visit.url.as_str()) {
                        *visits += 1;
                        continue;
                    }
                    if open.contains(&visit.url.as_str()) {
                        continue;
                    }
                    let Some(points) = score(&words, &visit.title, &visit.url) else { continue };
                    // A page seen lately comes before one seen long ago.
                    pages.insert(visit.url.as_str(), (order.len() as i64, 1));
                    order.push((visit.url.as_str(), visit.title.as_str(), points - (age.min(2000) / 100) as i64));
                }
                for (url, title, points) in order {
                    let visits = pages.get(url).map_or(1, |(_, n)| *n).min(20) as i64;
                    let shown = if title.is_empty() { url.to_string() } else { title.to_string() };
                    found.push((
                        points + visits * 4,
                        Item { action: Action::Visit { url: url.to_string(), title: title.to_string() }, title: shown, detail: display_host(url), kind: "history" },
                    ));
                }
            }
        }
        for (index, (name, title, keys)) in COMMANDS.iter().enumerate() {
            let Some(points) = score(&words, title, "") else { continue };
            let rank = if commands_only { 2000 } else { 0 };
            found.push((points + rank - index as i64, Item { action: Action::Command(name), title: title.to_string(), detail: keys.to_string(), kind: "command" }));
        }
        found.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        let mut items: Vec<Item> = found.into_iter().take(LIMIT).map(|(_, item)| item).collect();

        if !commands_only && !query.is_empty() {
            if let Some(url) = resolve(query) {
                let search = url.starts_with(SEARCH_BASE);
                let go = Item {
                    action: Action::Go(query.to_string()),
                    title: if search { format!("Search for “{query}”") } else { format!("Open {query}") },
                    detail: String::new(),
                    kind: if search { "search" } else { "open" },
                };
                // An address is what was meant; a search is what is left
                // when nothing else fits.
                if search {
                    items.push(go);
                } else {
                    items.insert(0, go);
                }
            }
        }
        items
    }

    fn palette_changed(&mut self) {
        let Some(text) = self.palette.as_ref().map(|p| p.edit.text.clone()) else { return };
        let items = self.palette_items(&text);
        if let Some(palette) = self.palette.as_mut() {
            palette.items = items;
            palette.selected = 0;
            palette.scroll = 0;
        }
    }

    fn palette_move(&mut self, delta: i64) {
        let height = self.palette_list_height().max(1);
        let Some(palette) = self.palette.as_mut() else { return };
        let len = palette.items.len() as i64;
        if len == 0 {
            return;
        }
        palette.selected = (palette.selected as i64 + delta).rem_euclid(len) as usize;
        if palette.selected < palette.scroll {
            palette.scroll = palette.selected;
        } else if palette.selected >= palette.scroll + height {
            palette.scroll = palette.selected + 1 - height;
        }
    }

    /// Does what the selected line says.
    fn palette_accept(&mut self) {
        let Some(palette) = self.palette.as_mut() else { return };
        let Some(action) = palette.items.get(palette.selected).map(|item| item.action.clone()) else { return };
        // What follows decides where the keys go, not the closing.
        let came_from_page = std::mem::take(&mut palette.return_to_page);
        let acts_on_page = matches!(action, Action::Command("reload" | "back" | "forward" | "toggle-mute" | "toggle-blocking" | "toggle-pin" | "toggle-sidebar" | "sleep-others"));
        self.close_palette();
        match action {
            Action::Tab { space, key } => {
                self.switch_space(space);
                if let Some(index) = self.index_of_key(key) {
                    self.select(index);
                }
                if came_from_page {
                    // A tab that is asleep takes the keys once it is awake.
                    self.focus_on_create = Some(key);
                    self.focus_page();
                }
            }
            Action::Reopen(index) => {
                self.reopen_closed(index);
                self.focus_on_create = self.current().map(|t| t.key);
            }
            Action::Visit { url, title } => self.open_in_new_tab(url, title),
            Action::Go(input) => {
                let url = resolve(&input).unwrap_or(input);
                self.open_in_new_tab(url, String::new());
            }
            Action::Command(name) => {
                self.on_command(name, None, None);
                if acts_on_page && came_from_page {
                    self.focus_page();
                }
            }
        }
    }

    pub(crate) fn handle_palette_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let page = self.palette_list_height().max(1) as i64;
        match key.code {
            KeyCode::Esc => return self.close_palette(),
            KeyCode::Char('c') if ctrl => return self.close_palette(),
            KeyCode::Enter => return self.palette_accept(),
            KeyCode::Down | KeyCode::Tab => return self.palette_move(1),
            KeyCode::Up | KeyCode::BackTab => return self.palette_move(-1),
            KeyCode::Char('n') if ctrl => return self.palette_move(1),
            KeyCode::Char('p') if ctrl => return self.palette_move(-1),
            KeyCode::PageDown => return self.palette_move(page),
            KeyCode::PageUp => return self.palette_move(-page),
            _ => {}
        }
        let Some(palette) = self.palette.as_mut() else { return };
        let before = palette.edit.text.clone();
        palette.edit.key(key);
        if palette.edit.text != before {
            self.palette_changed();
        }
    }

    pub(crate) fn palette_paste(&mut self, text: &str) {
        let Some(palette) = self.palette.as_mut() else { return };
        palette.edit.insert(text);
        self.palette_changed();
    }

    /// A click on a line does what it says; a click outside closes the palette.
    pub(crate) fn palette_click(&mut self, col: u16, row: u16) {
        let rect = self.palette_rect();
        if !contains(rect, col, row) {
            return self.close_palette();
        }
        let Some(palette) = self.palette.as_mut() else { return };
        if row >= rect.y + 2 {
            let index = palette.scroll + (row - rect.y - 2) as usize;
            if index < palette.items.len() {
                palette.selected = index;
                self.palette_accept();
            }
        }
    }
}

/// Draws the palette, if it is open, and says where its cursor is.
pub fn draw(buf: &mut Buffer, app: &App, p: &Colors) -> Option<Position> {
    let palette = app.palette.as_ref()?;
    let rect = app.palette_rect();
    if rect.width < 12 || rect.height < 3 {
        return None;
    }
    let surface = p.bar;
    fill(buf, rect, Style::default().bg(surface).fg(p.text));

    // The field: a mark in the space's colour, then what is typed.
    let field = Rect::new(rect.x, rect.y, rect.width, 1);
    fill(buf, field, Style::default().bg(p.field).fg(p.text));
    put(buf, field.x + 1, field.y, 1, "›", Style::default().fg(p.accent).bg(p.field).add_modifier(Modifier::BOLD));
    let text_x = field.x + 3;
    let text_w = field.width.saturating_sub(4) as usize;
    let chars: Vec<char> = palette.edit.text.chars().collect();
    let width_of = |slice: &[char]| slice.iter().map(|c| c.width().unwrap_or(0)).sum::<usize>();
    let at = palette.edit.cursor.min(chars.len());
    let mut start = 0;
    while width_of(&chars[start..at]) >= text_w.max(1) {
        start += 1;
    }
    if chars.is_empty() {
        put(buf, text_x, field.y, text_w as u16, "Find a tab, a page or a command   > commands only", Style::default().fg(p.faint).bg(p.field));
    } else {
        crate::ui::put_edit(buf, text_x, field.y, text_w, &chars, start, palette.edit.selection(), Style::default().fg(p.text).bg(p.field));
    }
    let cursor = Position::new(text_x + width_of(&chars[start..at]) as u16, field.y);

    let rule = "─".repeat(rect.width as usize);
    put(buf, rect.x, rect.y + 1, rect.width, &rule, Style::default().fg(p.line).bg(surface));

    let list = Rect::new(rect.x, rect.y + 2, rect.width, rect.height.saturating_sub(3));
    if palette.items.is_empty() {
        put(buf, list.x + 3, list.y, list.width.saturating_sub(4), "Nothing found", Style::default().fg(p.dim).bg(surface));
    }
    for (line, item) in palette.items.iter().skip(palette.scroll).take(list.height as usize).enumerate() {
        let y = list.y + line as u16;
        let selected = palette.scroll + line == palette.selected;
        let bg = if selected { p.selected } else { surface };
        if selected {
            fill(buf, Rect::new(list.x, y, list.width, 1), Style::default().bg(bg));
            buf[(list.x, y)].set_symbol("▎").set_fg(p.accent);
        }
        let kind_w = item.kind.width() as u16;
        let room = list.width.saturating_sub(4 + kind_w + 2);
        let title_style = Style::default().fg(p.text).bg(bg).add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() });
        put(buf, list.x + 3, y, room, &item.title, title_style);
        let used = (item.title.width() as u16).min(room);
        if !item.detail.is_empty() && room > used + 4 {
            put(buf, list.x + 3 + used + 2, y, room - used - 2, &item.detail, Style::default().fg(p.dim).bg(bg));
        }
        let kind_color = match item.kind {
            "tab" => p.accent,
            "command" => p.yellow,
            "open" | "search" => p.green,
            _ => p.faint,
        };
        put(buf, list.x + list.width - kind_w - 2, y, kind_w, item.kind, Style::default().fg(kind_color).bg(bg));
    }

    // The keys, on the last line, a step away from the list.
    let foot = Rect::new(rect.x, rect.y + rect.height - 1, rect.width, 1);
    let foot_bg = mix(surface, p.bg, 0.5);
    fill(buf, foot, Style::default().bg(foot_bg));
    let count = palette.items.len();
    let keys = "↑↓ move   ⏎ go   esc close";
    put(buf, foot.x + 3, foot.y, foot.width.saturating_sub(4), keys, Style::default().fg(p.dim).bg(foot_bg));
    let total = if count >= LIMIT { format!("{LIMIT}+") } else { count.to_string() };
    let total_w = total.width() as u16;
    if foot.width > keys.width() as u16 + total_w + 8 {
        put(buf, foot.x + foot.width - total_w - 2, foot.y, total_w, &total, Style::default().fg(p.faint).bg(foot_bg));
    }
    Some(cursor)
}

#[cfg(test)]
mod tests {
    use super::score;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_lowercase).collect()
    }

    #[test]
    fn every_word_must_be_found() {
        assert!(score(&words("git pull"), "Pull requests · GitHub", "https://github.com/pulls").is_some());
        assert!(score(&words("git zebra"), "Pull requests · GitHub", "https://github.com/pulls").is_none());
        assert!(score(&[], "anything", "").is_some());
    }

    #[test]
    fn better_matches_score_higher() {
        let start = score(&words("mail"), "Mail – Inbox", "https://mail.example/").unwrap();
        let inside = score(&words("mail"), "Gmail", "https://g.example/").unwrap();
        let address = score(&words("mail"), "Inbox", "https://mail.example/").unwrap();
        let in_order = score(&words("ml"), "Mail", "").unwrap();
        assert!(start > inside && inside > address && address > in_order);
        assert!(score(&words("x"), "Mail", "").is_none(), "a single letter is not matched loosely");
    }
}
