//! Drawing. Reads `App`, never changes it.

use crate::app::App;
use crate::protocol::Mode;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const BG: Color = Color::Rgb(23, 23, 28);
const PANEL: Color = Color::Rgb(30, 30, 37);
const BAR: Color = Color::Rgb(38, 38, 47);
const FIELD: Color = Color::Rgb(52, 53, 66);
const SELECTED: Color = Color::Rgb(58, 60, 76);
const TEXT: Color = Color::Rgb(220, 222, 228);
const DIM: Color = Color::Rgb(120, 123, 137);
const FAINT: Color = Color::Rgb(70, 72, 84);
const ACCENT: Color = Color::Rgb(122, 162, 247);
const GREEN: Color = Color::Rgb(158, 206, 106);
const YELLOW: Color = Color::Rgb(224, 175, 104);
const RED: Color = Color::Rgb(247, 118, 142);

/// Truncates to a display width, ending in an ellipsis when cut.
pub fn fit(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width - 1 {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

fn fill(buf: &mut Buffer, rect: Rect, style: Style) {
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            buf[(x, y)].set_symbol(" ").set_style(style);
        }
    }
}

fn put(buf: &mut Buffer, x: u16, y: u16, max_width: u16, text: &str, style: Style) {
    if max_width == 0 {
        return;
    }
    buf.set_stringn(x, y, fit(text, max_width as usize), max_width as usize, style);
}

/// Draws the whole interface and returns where the text cursor belongs, if anywhere.
pub fn draw(frame: &mut Frame, app: &App) -> Option<Position> {
    let area = frame.area();
    let layout = app.layout;
    let buf = frame.buffer_mut();
    fill(buf, area, Style::default().bg(BG).fg(TEXT));

    draw_sidebar(buf, app);
    let cursor = draw_toolbar(buf, app);
    draw_viewport(buf, app);
    draw_status(buf, app);
    let _ = layout;
    cursor
}

fn draw_sidebar(buf: &mut Buffer, app: &App) {
    let side = app.layout.sidebar;
    if side.width < 4 || side.height == 0 {
        return;
    }
    let inner_w = side.width - 1;
    fill(buf, Rect::new(side.x, side.y, inner_w, side.height), Style::default().bg(PANEL).fg(TEXT));
    for y in side.y..side.y + side.height {
        buf[(side.x + inner_w, y)].set_symbol("│").set_style(Style::default().fg(FAINT).bg(BG));
    }
    put(buf, side.x + 1, side.y, inner_w - 1, "weft", Style::default().fg(ACCENT).bg(PANEL).add_modifier(Modifier::BOLD));

    let list = app.layout.tab_list;
    for row in 0..list.height as usize {
        let index = app.scroll + row;
        let Some(tab) = app.tabs.get(index) else { break };
        let y = list.y + row as u16;
        let selected = index == app.selected;
        let bg = if selected { SELECTED } else { PANEL };
        let mut style = Style::default().bg(bg).fg(if selected { TEXT } else { DIM });
        if selected {
            style = style.add_modifier(Modifier::BOLD);
        }
        fill(buf, Rect::new(list.x, y, list.width, 1), style);
        if selected {
            buf[(list.x, y)].set_symbol("▎").set_style(Style::default().fg(ACCENT).bg(bg));
        }
        let (icon, icon_color) = if tab.crashed {
            ("!", RED)
        } else if tab.loading {
            ("○", YELLOW)
        } else if tab.tab_id.is_none() {
            ("·", FAINT)
        } else {
            (" ", DIM)
        };
        put(buf, list.x + 1, y, 1, icon, Style::default().fg(icon_color).bg(bg));
        put(buf, list.x + 3, y, list.width.saturating_sub(4), &tab.label(), style);
    }
    if app.scroll > 0 && list.height > 0 {
        put(buf, list.x + list.width - 1, list.y, 1, "↑", Style::default().fg(DIM).bg(PANEL));
    }
    if app.scroll + (list.height as usize) < app.tabs.len() && list.height > 0 {
        put(buf, list.x + list.width - 1, list.y + list.height - 1, 1, "↓", Style::default().fg(DIM).bg(PANEL));
    }

    let new_tab = app.layout.new_tab;
    if new_tab.height > 0 && new_tab.y > side.y {
        put(buf, new_tab.x + 1, new_tab.y, new_tab.width.saturating_sub(1), "+ New tab", Style::default().fg(DIM).bg(PANEL));
    }
}

fn draw_toolbar(buf: &mut Buffer, app: &App) -> Option<Position> {
    let bar = app.layout.toolbar;
    if bar.width == 0 || bar.height == 0 {
        return None;
    }
    fill(buf, bar, Style::default().bg(BAR).fg(TEXT));
    let tab = app.current();
    let on = Style::default().fg(TEXT).bg(BAR);
    let off = Style::default().fg(FAINT).bg(BAR);
    let button = |buf: &mut Buffer, rect: Rect, glyph: &str, enabled: bool| {
        if rect.width >= 2 {
            put(buf, rect.x + 1, rect.y, 1, glyph, if enabled { on } else { off });
        }
    };
    button(buf, app.layout.back, "←", tab.is_some_and(|t| t.can_go_back));
    button(buf, app.layout.forward, "→", tab.is_some_and(|t| t.can_go_forward));
    let loading = tab.is_some_and(|t| t.loading);
    button(buf, app.layout.reload, if loading { "×" } else { "↻" }, tab.is_some_and(|t| t.tab_id.is_some()));

    let field = app.layout.url;
    if field.width < 3 {
        return None;
    }
    let inner = Rect::new(field.x, field.y, field.width - 1, 1);
    let text_w = (inner.width - 2) as usize;
    match &app.edit {
        Some(edit) => {
            fill(buf, inner, Style::default().bg(FIELD).fg(TEXT));
            if edit.text.is_empty() {
                put(buf, inner.x + 1, inner.y, text_w as u16, "Type a URL or search", Style::default().fg(DIM).bg(FIELD));
                return Some(Position::new(inner.x + 1, inner.y));
            }
            // Scroll the text so the cursor stays inside the field.
            let chars: Vec<char> = edit.text.chars().collect();
            let width_of = |slice: &[char]| slice.iter().map(|c| c.width().unwrap_or(0)).sum::<usize>();
            let mut start = 0;
            while width_of(&chars[start..edit.cursor.min(chars.len())]) >= text_w.max(1) {
                start += 1;
            }
            let mut visible = String::new();
            let mut used = 0;
            for c in &chars[start..] {
                let w = c.width().unwrap_or(0);
                if used + w > text_w {
                    break;
                }
                visible.push(*c);
                used += w;
            }
            let style = if edit.replace_on_type {
                Style::default().bg(ACCENT).fg(Color::Black)
            } else {
                Style::default().bg(FIELD).fg(TEXT)
            };
            buf.set_stringn(inner.x + 1, inner.y, &visible, text_w, style);
            let cursor_x = inner.x + 1 + width_of(&chars[start..edit.cursor.min(chars.len())]) as u16;
            Some(Position::new(cursor_x.min(inner.x + inner.width - 1), inner.y))
        }
        None => {
            let url = tab.map(|t| t.url.as_str()).unwrap_or("");
            if url.is_empty() {
                put(buf, inner.x + 1, inner.y, text_w as u16, "Type a URL or search", off);
            } else {
                put(buf, inner.x + 1, inner.y, text_w as u16, url, Style::default().fg(DIM).bg(BAR));
            }
            None
        }
    }
}

fn draw_viewport(buf: &mut Buffer, app: &App) {
    let view = app.layout.viewport;
    if view.width == 0 || view.height == 0 || app.page_visible() {
        // The page covers these cells; they stay blank.
        return;
    }
    let lines: Vec<(String, Style)> = match app.current() {
        None => vec![("Connecting…".into(), Style::default().fg(DIM).bg(BG))],
        Some(_) if !app.ready => vec![("Connecting…".into(), Style::default().fg(DIM).bg(BG))],
        Some(tab) if tab.pending || tab.loading => vec![("Opening…".into(), Style::default().fg(DIM).bg(BG))],
        Some(_) => vec![
            ("New tab".into(), Style::default().fg(TEXT).bg(BG).add_modifier(Modifier::BOLD)),
            (String::new(), Style::default()),
            ("Type a URL or search terms, then press Enter".into(), Style::default().fg(DIM).bg(BG)),
        ],
    };
    let top = view.y + view.height.saturating_sub(lines.len() as u16) / 2;
    for (i, (text, style)) in lines.iter().enumerate() {
        let y = top + i as u16;
        if y >= view.y + view.height {
            break;
        }
        let shown = fit(text, view.width as usize);
        let x = view.x + (view.width.saturating_sub(shown.width() as u16)) / 2;
        buf.set_string(x, y, shown, *style);
    }
}

fn draw_status(buf: &mut Buffer, app: &App) {
    let bar = app.layout.status;
    if bar.width == 0 || bar.height == 0 {
        return;
    }
    fill(buf, bar, Style::default().bg(PANEL).fg(DIM));
    let (label, color) = match app.mode {
        Mode::Nav => (" NAV ", ACCENT),
        Mode::Page => (" PAGE ", GREEN),
    };
    let mut x = bar.x;
    let end = bar.x + bar.width;
    let mut segment = |buf: &mut Buffer, text: &str, style: Style| {
        if x >= end {
            return;
        }
        let room = end - x;
        put(buf, x, bar.y, room, text, style);
        x += (text.width() as u16).min(room);
    };
    segment(buf, label, Style::default().bg(color).fg(Color::Black).add_modifier(Modifier::BOLD));
    let count = app.tabs.len();
    segment(buf, &format!(" {count} {} ", if count == 1 { "tab" } else { "tabs" }), Style::default().bg(PANEL).fg(DIM));
    if let Some(message) = &app.message {
        segment(buf, &format!(" {message} "), Style::default().bg(PANEL).fg(YELLOW));
    } else if let Some(tab) = app.current().filter(|t| t.loading) {
        let percent = (tab.progress * 100.0).round().clamp(0.0, 100.0) as u32;
        segment(buf, &format!(" Loading… {percent}% "), Style::default().bg(PANEL).fg(YELLOW));
    }

    let hint = if app.edit.is_some() {
        "⏎ go   esc cancel"
    } else {
        match app.mode {
            Mode::Nav => "j/k tabs   o url   t new   x close   ⏎ page   q quit",
            Mode::Page => "⌘. tabs   ⌘L url   ⌘T new   ⌘W close",
        }
    };
    let hint_w = hint.width() as u16 + 1;
    if x + hint_w + 1 < end {
        put(buf, end - hint_w, bar.y, hint_w, hint, Style::default().bg(PANEL).fg(FAINT));
    }
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn fit_truncates_by_display_width() {
        assert_eq!(fit("hello", 10), "hello");
        assert_eq!(fit("hello world", 6), "hello…");
        assert_eq!(fit("日本語のタイトル", 7), "日本語…");
        assert_eq!(fit("abc", 0), "");
    }
}
