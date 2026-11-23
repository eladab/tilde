//! The welcome: a few pages that say what Tilde is and how it is driven.
//! It is shown on the first launch, in place of a page, and again whenever
//! it is asked for (`W`, or Help → Welcome to Tilde).

use crate::app::App;
use crate::protocol::{Mode, Request};
use crate::ui::{fit, put, Palette};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthStr;

pub struct Page {
    pub title: &'static str,
    /// Said before the keys.
    pub intro: &'static [&'static str],
    /// A key, what it does, and the ⌘ shortcut that does the same from the page.
    pub keys: &'static [(&'static str, &'static str, &'static str)],
    /// Said after them.
    pub outro: &'static [&'static str],
}

pub const PAGES: &[Page] = &[
    Page {
        title: "Welcome to Tilde",
        intro: &[
            "A web browser driven from the keyboard. Everything around the",
            "page is drawn in a terminal; the page itself is the real thing.",
            "",
            "The keyboard is in one of two places, and the bottom left",
            "corner always says which:",
        ],
        keys: &[
            ("NAV", "the keys drive the browser: tabs, addresses, commands", ""),
            ("PAGE", "the keys go to the web page, as in any browser", ""),
            ("", "", ""),
            ("Enter", "from NAV, hand the keys to the page", ""),
            ("⌘.", "from the page, take them back", ""),
        ],
        outro: &["Shortcuts with ⌘ work in both. A click works anywhere."],
    },
    Page {
        title: "Going somewhere",
        intro: &["In NAV mode, plain keys are commands:"],
        keys: &[
            ("o", "type an address or a search in this tab", "⌘L"),
            ("t", "open a new tab", "⌘T"),
            ("j  k", "the next tab, the previous tab", ""),
            ("x", "close the tab", "⌘W"),
            ("u", "reopen the tab you just closed", "⇧⌘T"),
            ("H  L", "back, forward", "⌘[ ⌘]"),
            ("r", "reload", "⌘R"),
        ],
        outro: &["An address you have been to before is finished for you as", "you type: → or Tab takes it."],
    },
    Page {
        title: "The page, without the mouse",
        intro: &["You can read and use a page while staying in NAV mode:"],
        keys: &[
            ("f", "put a label on every link and button, then type one", ""),
            ("F", "the same, opening the link in a tab behind this one", ""),
            ("Space", "scroll down a screenful", ""),
            ("^D  ^U", "scroll down, up, by half a screenful", ""),
            ("g  G", "the top of the page, the bottom", ""),
            ("/", "find in the page", "⌘F"),
        ],
        outro: &["A field's label puts the cursor in it and hands the page the", "keys. From the page, ⌘J shows the labels."],
    },
    Page {
        title: "Finding anything",
        intro: &["One field finds a tab in any space, a page you visited, a tab", "you closed or a command by name:"],
        keys: &[
            (":", "the command palette", "⇧⌘P"),
            (">", "the palette, commands only", ""),
            ("", "", ""),
            ("y", "everything you visited; / filters it", "⌘Y"),
            ("d", "downloads", "⌥⌘L"),
            ("I", "developer tools: network and storage", "⌥⌘I"),
        ],
        outro: &[],
    },
    Page {
        title: "Keeping order",
        intro: &[],
        keys: &[
            ("p", "pin the tab: it stays at the top", "⌘D"),
            ("n", "give the tab a name of your own", ""),
            ("N", "a new space: its own tabs, colours and logins", ""),
            ("h  l", "the previous space, the next", ""),
            ("m", "mute a tab that is making noise", ""),
            ("b", "hide or show the sidebar", "⌘B"),
        ],
        outro: &[
            "Tabs you have not looked at for half an hour go to sleep and",
            "give their memory back; they wake as they were. Requests to",
            "advertising and tracking networks are blocked.",
            "",
            "?  lists every key.   W  brings this welcome back.",
        ],
    },
];

impl App {
    /// Shows the welcome from its first page.
    pub fn open_welcome(&mut self) {
        if self.welcome.is_some() {
            return;
        }
        self.close_overlays();
        self.cancel_edit();
        self.stop_hints();
        self.message = None;
        self.welcome_return = Some(self.mode).filter(|m| *m == Mode::Page);
        self.welcome = Some(0);
        self.request(Request::FocusSet { mode: Mode::Nav });
        self.ensure_active();
    }

    pub fn close_welcome(&mut self) {
        if self.welcome.take().is_none() {
            return;
        }
        self.ensure_active();
        if self.welcome_return.take() == Some(Mode::Page) {
            self.focus_page();
        } else if self.current().is_some_and(|t| t.is_blank()) && self.edit.is_none() {
            // Nothing is open yet: the next thing to do is to go somewhere.
            self.start_edit();
        }
    }

    /// The next page; past the last one the welcome closes.
    pub(crate) fn welcome_next(&mut self) {
        match self.welcome {
            Some(page) if page + 1 < PAGES.len() => self.welcome = Some(page + 1),
            Some(_) => self.close_welcome(),
            None => {}
        }
    }

    pub(crate) fn handle_welcome_key(&mut self, key: KeyEvent) {
        let Some(page) = self.welcome else { return };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.close_welcome(),
            KeyCode::Char('c') if ctrl => self.close_welcome(),
            KeyCode::Right | KeyCode::Down | KeyCode::Enter | KeyCode::Tab | KeyCode::PageDown => self.welcome_next(),
            KeyCode::Char('l' | 'j' | 'n' | ' ') => self.welcome_next(),
            KeyCode::Left | KeyCode::Up | KeyCode::Backspace | KeyCode::BackTab | KeyCode::PageUp => self.welcome = Some(page.saturating_sub(1)),
            KeyCode::Char('h' | 'k' | 'p') => self.welcome = Some(page.saturating_sub(1)),
            KeyCode::Home => self.welcome = Some(0),
            KeyCode::End => self.welcome = Some(PAGES.len() - 1),
            KeyCode::Char(c @ '1'..='9') => {
                let wanted = c as usize - '1' as usize;
                if wanted < PAGES.len() {
                    self.welcome = Some(wanted);
                }
            }
            KeyCode::Char('?') => self.open_help(),
            _ => {}
        }
    }
}

/// Draws a page of the welcome in the page area.
pub fn draw(buf: &mut Buffer, view: Rect, index: usize, p: &Palette) {
    let page = &PAGES[index.min(PAGES.len() - 1)];
    let width = 64.min(view.width.saturating_sub(4));
    if width < 30 || view.height < 8 {
        put(buf, view.x + 1, view.y, view.width.saturating_sub(1), "Make the window larger to read the welcome", Style::default().fg(p.dim).bg(p.bg));
        return;
    }
    let text = Style::default().fg(p.text).bg(p.bg);
    let dim = Style::default().fg(p.dim).bg(p.bg);
    let key_style = Style::default().fg(p.blue).bg(p.bg).add_modifier(Modifier::BOLD);
    let key_w = page.keys.iter().map(|(k, _, _)| k.width()).max().unwrap_or(0) as u16;
    // Every page is as tall as the tallest, so the pages do not jump about.
    let height_of = |page: &Page| 2 + page.intro.len() + (!page.intro.is_empty() && !page.keys.is_empty()) as usize + page.keys.len()
        + (!page.outro.is_empty()) as usize + page.outro.len();
    let tallest = PAGES.iter().map(height_of).max().unwrap_or(0) as u16;
    let x = view.x + (view.width - width) / 2;
    let bottom = view.y + view.height;
    let mut y = view.y + view.height.saturating_sub(tallest + 3) / 2;

    put(buf, x, y, width, page.title, Style::default().fg(p.accent).bg(p.bg).add_modifier(Modifier::BOLD));
    let step = format!("{} of {}", index + 1, PAGES.len());
    put(buf, x + width - step.width() as u16, y, step.width() as u16, &step, dim);
    y += 2;
    let line = |buf: &mut Buffer, y: &mut u16, draw: &dyn Fn(&mut Buffer, u16)| {
        if *y + 2 < bottom {
            draw(buf, *y);
        }
        *y += 1;
    };
    for row in page.intro {
        line(buf, &mut y, &|buf, y| put(buf, x, y, width, row, text));
    }
    if !page.intro.is_empty() && !page.keys.is_empty() {
        y += 1;
    }
    for (key, what, shortcut) in page.keys {
        line(buf, &mut y, &|buf, y| {
            let short_w = shortcut.width() as u16;
            put(buf, x + 2, y, key_w, key, key_style);
            put(buf, x + 2 + key_w + 3, y, width.saturating_sub(key_w + 5 + if short_w > 0 { short_w + 2 } else { 0 }), what, text);
            put(buf, x + width - short_w, y, short_w, shortcut, dim);
        });
    }
    if !page.outro.is_empty() {
        y += 1;
    }
    for row in page.outro {
        line(buf, &mut y, &|buf, y| put(buf, x, y, width, row, dim));
    }

    // One mark per page, and the keys that turn them.
    let foot = bottom.saturating_sub(2).min(view.y + view.height.saturating_sub(tallest + 3) / 2 + tallest + 1);
    for mark in 0..PAGES.len() {
        let (dot, color) = if mark == index { ("●", p.accent) } else { ("•", p.faint) };
        put(buf, x + 2 * mark as u16, foot, 1, dot, Style::default().fg(color).bg(p.bg));
    }
    let keys = if index + 1 == PAGES.len() { "←  back     ⏎  start browsing" } else { "←  →  turn the page     esc  close" };
    let shown = fit(keys, width.saturating_sub(2 * PAGES.len() as u16 + 2) as usize);
    put(buf, x + width - shown.width() as u16, foot, shown.width() as u16, &shown, dim);
}
