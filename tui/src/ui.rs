//! Drawing. Reads `App`, never changes it.

use crate::app::{App, EditKind, SideRow};
use crate::art::{self, Art, BACKGROUNDS};
use crate::history::{self, Row};
use crate::keys;
use crate::resolve::display_host;
use crate::views::{self, View};
use crate::spaces::{FormField, SpaceForm, COLORS};
use crate::protocol::Mode;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Everything the interface is drawn with, all derived from the space's
/// background. The background can be any colour, dark or light: the text
/// colours are chosen against it, and every other colour is moved towards
/// white or black until it can be read on the surfaces.
#[derive(Clone, Copy)]
pub(crate) struct Palette {
    /// The page area and anything drawn in it.
    pub(crate) bg: Color,
    /// Sidebar and status line.
    pub(crate) panel: Color,
    /// Toolbar.
    pub(crate) bar: Color,
    /// A focused text field.
    pub(crate) field: Color,
    /// The selected tab's row.
    pub(crate) selected: Color,
    pub(crate) text: Color,
    /// Secondary text.
    pub(crate) dim: Color,
    /// Placeholders and things that are switched off. Still readable.
    pub(crate) faint: Color,
    /// Rules and borders, which need not be read.
    pub(crate) line: Color,
    /// The space's colour.
    pub(crate) accent: Color,
    pub(crate) blue: Color,
    pub(crate) green: Color,
    pub(crate) yellow: Color,
    pub(crate) red: Color,
    pub(crate) purple: Color,
}

/// Contrast ratios (as WCAG defines them) that the palette guarantees
/// against every surface.
pub(crate) const TEXT_CONTRAST: f64 = 7.0;
pub(crate) const DIM_CONTRAST: f64 = 4.5;
pub(crate) const FAINT_CONTRAST: f64 = 2.4;
pub(crate) const COLOR_CONTRAST: f64 = 4.0;

fn channels(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    }
}

/// `a` with `t` of `b` mixed in.
pub(crate) fn mix(a: Color, b: Color, t: f64) -> Color {
    let (a, b) = (channels(a), channels(b));
    let blend = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t.clamp(0.0, 1.0)).round() as u8;
    Color::Rgb(blend(a.0, b.0), blend(a.1, b.1), blend(a.2, b.2))
}

fn luminance(color: Color) -> f64 {
    let (r, g, b) = channels(color);
    let linear = |c: u8| {
        let c = c as f64 / 255.0;
        if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

pub(crate) fn contrast(a: Color, b: Color) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `color`, moved towards white or black just far enough to stand out
/// from `on` by the ratio `min`.
fn legible(color: Color, on: Color, min: f64) -> Color {
    let (white, black) = (Color::Rgb(255, 255, 255), Color::Rgb(0, 0, 0));
    let toward = if contrast(white, on) >= contrast(black, on) { white } else { black };
    (0..=40).map(|step| mix(color, toward, step as f64 / 40.0)).find(|c| contrast(*c, on) >= min).unwrap_or(toward)
}

impl Palette {
    pub(crate) fn new(background: usize, color: usize) -> Self {
        let (_, (r, g, b)) = BACKGROUNDS[background % BACKGROUNDS.len()];
        let bg = Color::Rgb(r, g, b);
        let light = luminance(bg) > 0.3;
        // Each surface is the background stepped away from itself: lifted on
        // a dark one (a little more in blue, which is what the original
        // palette did), lowered on a light one.
        let surface = |n: u8| {
            if light {
                let n = n + n / 3;
                Color::Rgb(r.saturating_sub(n), g.saturating_sub(n), b.saturating_sub(n))
            } else {
                Color::Rgb(r.saturating_add(n), g.saturating_add(n), b.saturating_add(n + n / 4))
            }
        };
        let selected = surface(36);
        // The selected row is the surface furthest from the background, so
        // what can be read on it can be read everywhere.
        let ink = if light { Color::Rgb(24, 26, 32) } else { TEXT };
        let on = |c: Color| legible(c, selected, COLOR_CONTRAST);
        let mut palette = Self {
            bg,
            panel: surface(7),
            bar: surface(15),
            field: surface(29),
            selected,
            text: legible(ink, selected, TEXT_CONTRAST),
            dim: legible(mix(ink, bg, 0.30), selected, DIM_CONTRAST),
            faint: legible(mix(ink, bg, 0.55), selected, FAINT_CONTRAST),
            line: mix(bg, ink, 0.17),
            accent: bg,
            blue: on(ACCENT),
            green: on(GREEN),
            yellow: on(YELLOW),
            red: on(RED),
            purple: on(PURPLE),
        };
        palette.accent = palette.ink(space_color(color));
        palette
    }

    pub(crate) fn of(app: &App) -> Self {
        let (color, background, _) = app.theme();
        Self::new(background, color)
    }

    /// Any colour, made readable on this palette's surfaces.
    pub(crate) fn ink(&self, color: Color) -> Color {
        legible(color, self.selected, COLOR_CONTRAST)
    }
}

// The colours as designed, for a dark background. Text is drawn with the
// palette's versions of them; these are for filling a badge that holds
// black text.
pub(crate) const TEXT: Color = Color::Rgb(220, 222, 228);
pub(crate) const ACCENT: Color = Color::Rgb(122, 162, 247);
pub(crate) const GREEN: Color = Color::Rgb(158, 206, 106);
pub(crate) const YELLOW: Color = Color::Rgb(224, 175, 104);
pub(crate) const RED: Color = Color::Rgb(247, 118, 142);
pub(crate) const PURPLE: Color = Color::Rgb(187, 154, 247);

/// A space's colour as designed. Text in it goes through `Palette::ink`.
pub(crate) fn space_color(index: usize) -> Color {
    let (_, (r, g, b)) = COLORS[index % COLORS.len()];
    Color::Rgb(r, g, b)
}

/// The frames of the spinner beside something that is loading.
pub(crate) const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Tints the first `fraction` of a row, under whatever is drawn there.
fn wash(buf: &mut Buffer, row: Rect, fraction: f64, color: Color) {
    let cells = (row.width as f64 * fraction.clamp(0.0, 1.0)).round() as u16;
    for x in row.x..row.x + cells.min(row.width) {
        buf[(x, row.y)].set_bg(color);
    }
}

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

pub(crate) fn fill(buf: &mut Buffer, rect: Rect, style: Style) {
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            buf[(x, y)].set_symbol(" ").set_style(style);
        }
    }
}

pub(crate) fn put(buf: &mut Buffer, x: u16, y: u16, max_width: u16, text: &str, style: Style) {
    if max_width == 0 {
        return;
    }
    buf.set_stringn(x, y, fit(text, max_width as usize), max_width as usize, style);
}

/// Draws the whole interface and returns where the text cursor belongs, if anywhere.
pub fn draw(frame: &mut Frame, app: &App) -> Option<Position> {
    let area = frame.area();
    let buf = frame.buffer_mut();
    let p = Palette::of(app);
    fill(buf, area, Style::default().bg(p.bg).fg(p.text));

    draw_sidebar(buf, app);
    let cursor = draw_toolbar(buf, app);
    draw_viewport(buf, app);
    let cursor = crate::devtools_ui::draw(buf, app, &p).or(cursor);
    draw_status(buf, app);
    cursor
}

fn draw_sidebar(buf: &mut Buffer, app: &App) {
    let p = Palette::of(app);
    let side = app.layout.sidebar;
    if side.width < 4 || side.height == 0 {
        return;
    }
    let inner_w = side.width - 1;
    fill(buf, Rect::new(side.x, side.y, inner_w, side.height), Style::default().bg(p.panel).fg(p.text));
    for y in side.y..side.y + side.height {
        buf[(side.x + inner_w, y)].set_symbol("│").set_style(Style::default().fg(p.line).bg(p.bg));
    }
    // The space: its name in its colour, then one mark per space.
    let art_index = app.theme().2;
    let accent = p.accent;
    let space = app.current_space();
    // While a space is being made or edited, the header previews its name.
    let name = match &app.form {
        Some(form) if !form.name.trim().is_empty() => form.name.trim().to_string(),
        Some(form) if !form.editing => "New space".to_string(),
        _ => space.name.clone(),
    };
    // Shown the way a shell shows a folder in your home directory: the
    // path quietly, the name in the space's colour.
    put(buf, side.x + 1, side.y, 2, "~/", Style::default().fg(p.dim).bg(p.panel));
    put(buf, side.x + 3, side.y, inner_w.saturating_sub(3), &name, Style::default().fg(accent).bg(p.panel).add_modifier(Modifier::BOLD));
    // How many tabs it holds, at the far end of the line.
    let count = app.tabs.len().to_string();
    let count_w = count.width() as u16;
    if inner_w > 3 + name.width() as u16 + count_w + 2 {
        put(buf, side.x + inner_w - count_w - 1, side.y, count_w, &count, Style::default().fg(p.faint).bg(p.panel));
    }
    if side.height > 1 {
        for (index, other) in app.spaces.iter().enumerate() {
            let x = side.x + 1 + 2 * index as u16;
            if x + 1 >= side.x + inner_w {
                break;
            }
            // The current space is a full mark, the others small ones in
            // their own colours, held back so that they do not compete.
            let (dot, dot_color) = if index == app.space {
                ("●", accent)
            } else {
                ("•", mix(p.ink(space_color(other.color)), p.panel, 0.35))
            };
            put(buf, x, side.y + 1, 1, dot, Style::default().fg(dot_color).bg(p.panel));
        }
    }

    let list = app.layout.tab_list;
    let rows = app.side_rows();
    let rule_style = Style::default().fg(p.line).bg(p.panel);
    for row in 0..list.height as usize {
        let Some(side_row) = rows.get(app.scroll + row).copied() else { break };
        let y = list.y + row as u16;
        let index = match side_row {
            SideRow::Tab(index) => index,
            SideRow::PinPlaceholder => {
                put(buf, list.x + 1, y, list.width.saturating_sub(2), "No pinned tabs · p to pin", Style::default().fg(p.faint).bg(p.panel));
                continue;
            }
            SideRow::Separator => {
                // A light rule, inset from both edges.
                let rule = "╌".repeat(list.width.saturating_sub(4) as usize);
                put(buf, list.x + 2, y, list.width.saturating_sub(4), &rule, rule_style);
                continue;
            }
        };
        let Some(tab) = app.tabs.get(index) else { break };
        let selected = index == app.selected;
        let unloaded = tab.tab_id.is_none();
        let bg = if selected { p.selected } else { p.panel };
        // Three steps of emphasis: the tab showing, tabs with a page, and
        // tabs that have not been loaded.
        let mut style = Style::default().bg(bg).fg(if selected { p.text } else if unloaded { p.faint } else { p.dim });
        if selected {
            style = style.add_modifier(Modifier::BOLD);
        }
        let line = Rect::new(list.x, y, list.width, 1);
        fill(buf, line, style);
        let (icon, icon_color) = if tab.crashed {
            ("!", p.red)
        } else if tab.loading {
            (SPINNER[app.frame % SPINNER.len()], accent)
        } else if unloaded {
            ("·", p.faint)
        } else {
            (" ", p.dim)
        };
        put(buf, list.x + 1, y, 1, icon, Style::default().fg(icon_color).bg(bg));
        put(buf, list.x + 3, y, list.width.saturating_sub(4), &tab.label(), style);
        if tab.loading {
            // The row fills with a trace of the space's colour as the page arrives.
            wash(buf, line, tab.progress, mix(bg, accent, 0.16));
        }
        if selected {
            buf[(list.x, y)].set_symbol("▎").set_fg(accent);
        }
    }
    if app.scroll > 0 && list.height > 0 {
        put(buf, list.x + list.width - 1, list.y, 1, "↑", Style::default().fg(p.dim).bg(p.panel));
    }
    if app.scroll + (list.height as usize) < rows.len() && list.height > 0 {
        put(buf, list.x + list.width - 1, list.y + list.height - 1, 1, "↓", Style::default().fg(p.dim).bg(p.panel));
    }

    // The space's art sits at the bottom of the list, if the tabs leave room.
    if let Some(piece) = art::art(art_index) {
        let cells = (piece.height() / 2) as u16;
        let used = rows.len().saturating_sub(app.scroll) as u16;
        if list.height >= used + cells + 2 && list.width as usize >= piece.width() {
            let x = list.x + (list.width - piece.width() as u16).div_ceil(2);
            draw_art(buf, piece, x, list.y + list.height - cells - 1, p.panel);
        }
    }

    let new_tab = app.layout.new_tab;
    if new_tab.height > 0 && new_tab.y > side.y {
        put(buf, new_tab.x + 1, new_tab.y, 1, "+", Style::default().fg(accent).bg(p.panel).add_modifier(Modifier::BOLD));
        put(buf, new_tab.x + 3, new_tab.y, new_tab.width.saturating_sub(3), "New tab", Style::default().fg(p.dim).bg(p.panel));
        if new_tab.width >= 16 {
            put(buf, new_tab.x + new_tab.width - 3, new_tab.y, 2, "⌘T", Style::default().fg(p.faint).bg(p.panel));
        }
    }
}

/// Draws a piece with half blocks: the upper pixel is the glyph's foreground,
/// the lower one its background. `behind` shows through transparent pixels.
fn draw_art(buf: &mut Buffer, piece: &Art, x: u16, y: u16, behind: Color) {
    let color = |pixel: Option<(u8, u8, u8)>| pixel.map(|(r, g, b)| Color::Rgb(r, g, b));
    for row in 0..piece.height() / 2 {
        for col in 0..piece.width() {
            let top = color(piece.pixel(col, row * 2));
            let bottom = color(piece.pixel(col, row * 2 + 1));
            let (cx, cy) = (x + col as u16, y + row as u16);
            if cx >= buf.area.x + buf.area.width || cy >= buf.area.y + buf.area.height {
                continue;
            }
            match (top, bottom) {
                (None, None) => {}
                (Some(t), Some(b)) => {
                    buf[(cx, cy)].set_symbol("▀").set_style(Style::default().fg(t).bg(b));
                }
                (Some(t), None) => {
                    buf[(cx, cy)].set_symbol("▀").set_style(Style::default().fg(t).bg(behind));
                }
                (None, Some(b)) => {
                    buf[(cx, cy)].set_symbol("▄").set_style(Style::default().fg(b).bg(behind));
                }
            }
        }
    }
}

fn draw_toolbar(buf: &mut Buffer, app: &App) -> Option<Position> {
    let p = Palette::of(app);
    let bar = app.layout.toolbar;
    if bar.width == 0 || bar.height == 0 {
        return None;
    }
    fill(buf, bar, Style::default().bg(p.bar).fg(p.text));
    let tab = app.current();
    let on = Style::default().fg(p.text).bg(p.bar).add_modifier(Modifier::BOLD);
    let off = Style::default().fg(p.faint).bg(p.bar);
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
    // The field is a pill: half a cell of the toolbar shows at each end.
    let pill = |buf: &mut Buffer, color: Color| {
        fill(buf, inner, Style::default().bg(color).fg(p.text));
        buf[(inner.x, inner.y)].set_symbol("▐").set_style(Style::default().fg(color).bg(p.bar));
        buf[(inner.x + inner.width - 1, inner.y)].set_symbol("▌").set_style(Style::default().fg(color).bg(p.bar));
    };
    match &app.edit {
        Some(edit) => {
            pill(buf, p.field);
            // A rename is announced by a label, so it cannot be mistaken for the address.
            let mut inner = inner;
            let mut text_w = text_w;
            if edit.kind == EditKind::Rename {
                let label = " Name ";
                let label_w = (label.width() as u16).min(inner.width);
                put(buf, inner.x, inner.y, label_w, label, Style::default().bg(YELLOW).fg(Color::Black).add_modifier(Modifier::BOLD));
                inner = Rect::new(inner.x + label_w, inner.y, inner.width - label_w, 1);
                text_w = inner.width.saturating_sub(2) as usize;
                if inner.width < 3 {
                    return None;
                }
            }
            if edit.text.is_empty() {
                let hint = if edit.kind == EditKind::Rename { "Tab name (empty uses the page title)" } else { "Type a URL or search" };
                put(buf, inner.x + 1, inner.y, text_w as u16, hint, Style::default().fg(p.faint).bg(p.field));
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
                Style::default().bg(p.field).fg(p.text)
            };
            buf.set_stringn(inner.x + 1, inner.y, &visible, text_w, style);
            let cursor_x = inner.x + 1 + width_of(&chars[start..edit.cursor.min(chars.len())]) as u16;
            Some(Position::new(cursor_x.min(inner.x + inner.width - 1), inner.y))
        }
        None => {
            // At rest the field sits a little above the toolbar, and the
            // address is shown the way it is read: the site first.
            let well = mix(p.bar, p.field, 0.55);
            pill(buf, well);
            let url = tab.map(|t| t.url.as_str()).unwrap_or("");
            if url.is_empty() {
                put(buf, inner.x + 1, inner.y, text_w as u16, "Type a URL or search", Style::default().fg(p.faint).bg(well));
            } else {
                let (scheme, site, rest) = address_parts(url);
                let end = inner.x + 1 + text_w as u16;
                let mut x = inner.x + 1;
                for (part, color) in [(scheme, p.dim), (site, p.text), (rest, p.dim)] {
                    if x >= end || part.is_empty() {
                        continue;
                    }
                    put(buf, x, inner.y, end - x, part, Style::default().fg(color).bg(well));
                    x += (part.width() as u16).min(end - x);
                }
            }
            if let Some(tab) = tab.filter(|t| t.loading) {
                // The field fills with the space's colour as the page arrives.
                wash(buf, Rect::new(inner.x + 1, inner.y, inner.width - 2, 1), tab.progress, mix(well, p.accent, 0.24));
            }
            None
        }
    }
}

/// An address as the toolbar shows it: the scheme (nothing for https, which
/// is the ordinary case), the site, and the rest.
fn address_parts(url: &str) -> (&str, &str, &str) {
    let (scheme, after) = if let Some(after) = url.strip_prefix("https://") {
        ("", after)
    } else if url.starts_with("http://") {
        url.split_at("http://".len())
    } else {
        // about:blank, file:///…, and anything else is shown whole.
        return ("", url, "");
    };
    let site_end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (site, rest) = after.split_at(site_end);
    (scheme, site, rest)
}

fn draw_viewport(buf: &mut Buffer, app: &App) {
    let p = Palette::of(app);
    let view = app.layout.viewport;
    if view.width == 0 || view.height == 0 || app.page_visible() {
        // The page covers these cells; they stay blank.
        return;
    }
    match app.view {
        Some(View::History) => return draw_history(buf, app, &p),
        Some(View::Downloads) => return draw_downloads(buf, app, &p),
        None => {}
    }
    if let Some(form) = &app.form {
        draw_space_form(buf, view, form, &p);
        return;
    }
    if app.confirm_delete {
        let space = app.current_space();
        let count = app.tabs.iter().filter(|t| !t.url.is_empty()).count();
        let lines = [
            (format!("Delete space \"{}\"?", space.name), Style::default().fg(p.text).bg(p.bg).add_modifier(Modifier::BOLD)),
            (String::new(), Style::default()),
            (format!("Its {count} tab{} will be closed. Logins in its profile are kept.", if count == 1 { "" } else { "s" }), Style::default().fg(p.dim).bg(p.bg)),
            (String::new(), Style::default()),
            ("y  delete        any other key  keep".to_string(), Style::default().fg(p.yellow).bg(p.bg)),
        ];
        draw_centered(buf, view, &lines);
        return;
    }
    if app.help {
        draw_help(buf, view, &p);
        return;
    }
    let lines: Vec<(String, Style)> = match app.current() {
        None => vec![("Connecting…".into(), Style::default().fg(p.dim).bg(p.bg))],
        Some(_) if !app.ready => vec![("Connecting…".into(), Style::default().fg(p.dim).bg(p.bg))],
        Some(tab) if tab.pending || tab.loading => vec![("Opening…".into(), Style::default().fg(p.dim).bg(p.bg))],
        Some(_) => vec![
            ("New tab".into(), Style::default().fg(p.text).bg(p.bg).add_modifier(Modifier::BOLD)),
            (String::new(), Style::default()),
            ("Type a URL or search terms, then press Enter".into(), Style::default().fg(p.dim).bg(p.bg)),
        ],
    };
    // On a blank tab the space's art sits above the text, when there is room.
    if let Some(piece) = art::art(app.theme().2).filter(|_| lines.len() == 3) {
        let cells = (piece.height() / 2) as u16;
        if view.height >= cells + 9 && view.width as usize >= piece.width() + 4 {
            let top = view.y + (view.height - 3) / 2;
            draw_art(buf, piece, view.x + (view.width - piece.width() as u16) / 2, top - cells - 2, p.bg);
        }
    }
    draw_centered(buf, view, &lines);
}

fn draw_centered(buf: &mut Buffer, view: Rect, lines: &[(String, Style)]) {
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

/// The space editor. Like the key reference, it takes the place of the page.
/// The sidebar and the colours around it preview the choices as they are made.
fn draw_space_form(buf: &mut Buffer, view: Rect, form: &SpaceForm, p: &Palette) {
    let width = 62.min(view.width.saturating_sub(4));
    if width < 40 || view.height < 18 {
        put(buf, view.x + 1, view.y, view.width.saturating_sub(1), "Make the window larger to edit the space", Style::default().fg(p.dim).bg(p.bg));
        return;
    }
    let x = view.x + (view.width - width) / 2;
    let mut y = view.y + (view.height - 18) / 2;
    let accent = p.accent;
    let label = Style::default().fg(p.dim).bg(p.bg);
    let label_on = Style::default().fg(accent).bg(p.bg).add_modifier(Modifier::BOLD);
    let indent = 12; // width of the label column
    let field_w = width - indent;
    let fx = x + indent;
    let name_style = |on: bool| if on { label_on } else { label };
    let arrows = |on: bool| if on { "   ← → to change" } else { "" };

    let title = if form.editing { "Edit space" } else { "New space" };
    put(buf, x, y, width, title, Style::default().fg(accent).bg(p.bg).add_modifier(Modifier::BOLD));
    y += 2;

    let text_field = |buf: &mut Buffer, y: u16, name: &str, value: &str, hint: &str, focused: bool| {
        put(buf, x, y, indent - 1, name, name_style(focused));
        let bg = if focused { p.field } else { p.bar };
        fill(buf, Rect::new(fx, y, field_w, 1), Style::default().bg(bg).fg(p.text));
        if value.is_empty() {
            put(buf, fx + 1, y, field_w - 2, hint, Style::default().fg(p.dim).bg(bg));
            if focused {
                buf[(fx, y)].set_symbol("▕").set_style(Style::default().fg(accent).bg(bg));
            }
            return;
        }
        // Show the end of a long value, where typing happens.
        let chars: Vec<char> = value.chars().collect();
        let room = (field_w - 3) as usize;
        let mut start = chars.len();
        let mut used = 0;
        while start > 0 {
            let w = chars[start - 1].width().unwrap_or(0);
            if used + w > room {
                break;
            }
            used += w;
            start -= 1;
        }
        let shown: String = chars[start..].iter().collect();
        buf.set_stringn(fx + 1, y, &shown, room, Style::default().fg(p.text).bg(bg));
        if focused {
            buf[(fx + 1 + used as u16, y)].set_symbol("▏").set_style(Style::default().fg(accent).bg(bg));
        }
    };

    text_field(buf, y, "Name", &form.name, "Work, Personal, …", form.field == FormField::Name);
    y += 2;

    // Colour: a row of dots, the chosen one in brackets.
    let on = form.field == FormField::Color;
    put(buf, x, y, indent - 1, "Colour", name_style(on));
    for (index, (_, (r, g, b))) in COLORS.iter().enumerate() {
        let cx = fx + 4 * index as u16;
        if cx + 3 > x + width {
            break;
        }
        let color = p.ink(Color::Rgb(*r, *g, *b));
        if index == form.color {
            put(buf, cx, y, 3, "[●]", Style::default().fg(color).bg(p.bg).add_modifier(Modifier::BOLD));
        } else {
            put(buf, cx + 1, y, 1, "●", Style::default().fg(color).bg(p.bg));
        }
    }
    put(buf, fx, y + 1, field_w, &format!("{}{}", COLORS[form.color % COLORS.len()].0, arrows(on)), label);
    y += 3;

    // Background: a swatch of each. The dark ones are hard to tell apart
    // at their true values, so those are shown a few steps lighter than they
    // really are; the window around the editor shows the real thing.
    let on = form.field == FormField::Background;
    put(buf, x, y, indent - 1, "Background", name_style(on));
    let pitch = if BACKGROUNDS.len() as u16 * 3 <= field_w { 3 } else { 2 };
    for (index, (_, (r, g, b))) in BACKGROUNDS.iter().enumerate() {
        let cx = fx + pitch * index as u16;
        if cx + 3 > x + width {
            break;
        }
        let dark = (*r as u16 + *g as u16 + *b as u16) < 150;
        let swatch = if dark { Color::Rgb(r.saturating_add(34), g.saturating_add(34), b.saturating_add(40)) } else { Color::Rgb(*r, *g, *b) };
        put(buf, cx + 1, y, 1, " ", Style::default().bg(swatch));
    }
    // The brackets go on last, so that a neighbour's swatch cannot cover them.
    let cx = fx + pitch * (form.background % BACKGROUNDS.len()) as u16;
    if cx + 3 <= x + width {
        put(buf, cx, y, 1, "[", Style::default().fg(p.text).bg(p.bg).add_modifier(Modifier::BOLD));
        put(buf, cx + 2, y, 1, "]", Style::default().fg(p.text).bg(p.bg).add_modifier(Modifier::BOLD));
    }
    put(buf, fx, y + 1, field_w, &format!("{}{}", BACKGROUNDS[form.background % BACKGROUNDS.len()].0, arrows(on)), label);
    y += 3;

    // Art: the name of the piece; the sidebar shows the piece itself.
    let on = form.field == FormField::Art;
    put(buf, x, y, indent - 1, "Art", name_style(on));
    let choice = format!("‹ {} ›", art::art_name(form.art));
    put(buf, fx, y, field_w, &choice, if on { Style::default().fg(p.text).bg(p.bg).add_modifier(Modifier::BOLD) } else { Style::default().fg(p.text).bg(p.bg) });
    put(buf, fx, y + 1, field_w, &format!("shown under the tabs{}", arrows(on)), label);
    y += 3;

    let own = form.resolved_profile();
    let hint = if own.is_empty() { "its own, named after the space".to_string() } else { format!("its own: {own}") };
    text_field(buf, y, "Profile", &form.profile, &hint, form.field == FormField::Profile);
    put(buf, fx, y + 1, field_w, "Spaces with the same profile share logins.", label);
    put(buf, fx, y + 2, field_w, "\"default\" is the profile of the first space.", label);
    y += 4;

    if let Some(error) = &form.error {
        put(buf, x, y, width, error, Style::default().fg(p.red).bg(p.bg));
    } else {
        put(buf, x, y, width, "tab next field    ⏎ save    esc cancel", Style::default().fg(p.dim).bg(p.bg));
    }
}

/// The key reference. It takes the place of the page, which cannot be drawn over.
fn draw_help(buf: &mut Buffer, view: Rect, p: &Palette) {
    let key_style = Style::default().fg(p.blue).bg(p.bg).add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(p.text).bg(p.bg);
    let head_style = Style::default().fg(p.yellow).bg(p.bg).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(p.dim).bg(p.bg);

    let key_w = |rows: &[(&str, &str)]| rows.iter().map(|(k, _)| k.width()).max().unwrap_or(0) as u16;
    let col_w = |rows: &[(&str, &str)]| key_w(rows) + 3 + rows.iter().map(|(_, d)| d.width()).max().unwrap_or(0) as u16;
    let sections: [(&str, &[(&str, &str)]); 2] = [("NAV mode", keys::NAV), ("Anywhere", keys::ANYWHERE)];
    let gap = 6;
    let side_by_side = col_w(keys::NAV) + gap + col_w(keys::ANYWHERE) + 4 <= view.width;

    let draw_section = |buf: &mut Buffer, x: u16, mut y: u16, title: &str, rows: &[(&str, &str)]| -> u16 {
        let bottom = view.y + view.height;
        let room = (view.x + view.width).saturating_sub(x);
        if y < bottom {
            put(buf, x, y, room, title, head_style);
        }
        y += 2;
        let kw = key_w(rows);
        for (key, what) in rows {
            if y >= bottom {
                break;
            }
            put(buf, x, y, room, key, key_style);
            put(buf, x + kw + 3, y, room.saturating_sub(kw + 3), what, text_style);
            y += 1;
        }
        y
    };

    let top = view.y + 2.min(view.height.saturating_sub(1));
    let mut end = top;
    if side_by_side {
        let total = col_w(keys::NAV) + gap + col_w(keys::ANYWHERE);
        let x = view.x + (view.width - total) / 2;
        let a = draw_section(buf, x, top, sections[0].0, sections[0].1);
        let b = draw_section(buf, x + col_w(keys::NAV) + gap, top, sections[1].0, sections[1].1);
        end = end.max(a).max(b);
    } else {
        let x = view.x + 2.min(view.width.saturating_sub(1));
        let mut y = top;
        for (title, rows) in sections {
            y = draw_section(buf, x, y, title, rows) + 1;
        }
        end = y;
    }
    let footer = "Press any key to return to the page";
    let y = (end + 1).min(view.y + view.height - 1);
    let shown = fit(footer, view.width as usize);
    put(buf, view.x + (view.width.saturating_sub(shown.width() as u16)) / 2, y, view.width, &shown, dim);
}

/// The heading of a view: its name, how much it holds, and a rule of thumb
/// for what the keys do. Returns nothing; the list starts at `App::view_list`.
fn draw_view_heading(buf: &mut Buffer, app: &App, p: &Palette, name: &str, detail: &str) {
    let view = app.layout.viewport;
    let list = app.view_list();
    if view.height < 2 || list.width == 0 {
        return;
    }
    let y = view.y + 1;
    put(buf, list.x, y, list.width, name, Style::default().fg(p.text).bg(p.bg).add_modifier(Modifier::BOLD));
    let x = list.x + name.width() as u16 + 2;
    if x < list.x + list.width {
        put(buf, x, y, list.x + list.width - x, detail, Style::default().fg(p.dim).bg(p.bg));
    }
}

/// The view's own keys, along the bottom of the page area. They run on to
/// the last line when one line is too narrow for them.
fn draw_view_keys(buf: &mut Buffer, app: &App, p: &Palette, keys: &[(&str, &str)]) {
    let view = app.layout.viewport;
    let list = app.view_list();
    if view.height < 6 || list.width == 0 {
        return;
    }
    let mut y = view.y + view.height - 2;
    let end = list.x + list.width;
    let mut x = list.x;
    for (key, what) in keys {
        let width = (key.width() + 1 + what.width()) as u16;
        if x + width > end {
            if x == list.x || y + 1 >= view.y + view.height {
                break;
            }
            x = list.x;
            y += 1;
        }
        put(buf, x, y, end - x, key, Style::default().fg(p.blue).bg(p.bg).add_modifier(Modifier::BOLD));
        put(buf, x + key.width() as u16 + 1, y, end - x, what, Style::default().fg(p.dim).bg(p.bg));
        x += width + 3;
    }
}

fn draw_view_empty(buf: &mut Buffer, app: &App, p: &Palette, text: &str) {
    let list = app.view_list();
    if list.height > 0 {
        put(buf, list.x, list.y, list.width, text, Style::default().fg(p.dim).bg(p.bg));
    }
}

/// Every page visited, newest first, under headings by time.
fn draw_history(buf: &mut Buffer, app: &App, p: &Palette) {
    let count = app.history.len();
    if app.confirm_clear {
        let list = app.view_list();
        draw_view_heading(buf, app, p, "History", "");
        let x = list.x + "History".width() as u16 + 2;
        put(buf, x, app.layout.viewport.y + 1, (list.x + list.width).saturating_sub(x), "Clear all history?   y clears   any other key keeps it",
            Style::default().fg(p.yellow).bg(p.bg));
    } else {
        draw_view_heading(buf, app, p, "History", &format!("{count} page{}, in every space", if count == 1 { "" } else { "s" }));
    }
    draw_view_keys(buf, app, p, keys::HISTORY);
    if count == 0 {
        return draw_view_empty(buf, app, p, "No pages visited yet.");
    }
    let list = app.view_list();
    let width = list.width as usize;
    // Time, title, then (when there is room) the site, then the space.
    let space_w = 16.min(width / 4);
    let host_w = if width >= 70 { 24 } else { 0 };
    let title_w = width.saturating_sub(7 + space_w + host_w + if host_w > 0 { 2 } else { 0 } + 2);
    let rows = app.history_rows();
    for (line, row) in rows.iter().skip(app.view_scroll).take(list.height as usize).enumerate() {
        let y = list.y + line as u16;
        match row {
            Row::Header(title) => put(buf, list.x, y, list.width, title, Style::default().fg(p.yellow).bg(p.bg).add_modifier(Modifier::BOLD)),
            Row::Item(index) => {
                let Some(visit) = app.history.get(*index) else { continue };
                let selected = *index == app.view_selected;
                let bg = if selected { p.selected } else { p.bg };
                if selected {
                    fill(buf, Rect::new(list.x, y, list.width, 1), Style::default().bg(bg));
                }
                let mut x = list.x + 1;
                put(buf, x, y, 5, &history::clock(visit.time), Style::default().fg(p.dim).bg(bg));
                x += 7;
                let title = if visit.title.is_empty() { visit.url.as_str() } else { visit.title.as_str() };
                put(buf, x, y, title_w as u16, title, Style::default().fg(p.text).bg(bg));
                x += title_w as u16 + 2;
                if host_w > 0 {
                    put(buf, x, y, host_w as u16, &display_host(&visit.url), Style::default().fg(p.dim).bg(bg));
                    x += host_w as u16 + 2;
                }
                // The space is shown in its colour while it still exists.
                let color = app.spaces.iter().find(|s| s.name == visit.space).map(|s| p.ink(space_color(s.color))).unwrap_or(p.faint);
                let room = (list.x + list.width).saturating_sub(x + 1);
                if room > 2 && !visit.space.is_empty() {
                    put(buf, x, y, room, &format!("● {}", visit.space), Style::default().fg(color).bg(bg));
                }
            }
        }
    }
}

/// A bar of `width` cells filled to `fraction`.
fn progress_bar(fraction: f64, width: usize) -> String {
    let filled = ((fraction.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

/// Every download, running and past, newest first.
fn draw_downloads(buf: &mut Buffer, app: &App, p: &Palette) {
    let count = app.downloads.len();
    let running = app.downloads.iter().filter(|d| d.is_active()).count();
    let detail = match (count, running) {
        (0, _) => String::new(),
        (_, 0) => format!("{count} file{}, saved in Downloads", if count == 1 { "" } else { "s" }),
        _ => format!("{count} file{}, {running} in progress", if count == 1 { "" } else { "s" }),
    };
    draw_view_heading(buf, app, p, "Downloads", &detail);
    draw_view_keys(buf, app, p, keys::DOWNLOADS);
    if count == 0 {
        return draw_view_empty(buf, app, p, "Nothing has been downloaded.");
    }
    let list = app.view_list();
    let width = list.width as usize;
    let now = app.now();
    // File name, what became of it, then (when there is room) where it came from, then when.
    let status_w = 34.min(width / 2);
    let host_w = if width >= 84 { 20 } else { 0 };
    let name_w = width.saturating_sub(status_w + host_w + if host_w > 0 { 2 } else { 0 } + 6 + 5);
    for line in 0..list.height as usize {
        let index = app.view_scroll + line;
        let Some(download) = app.download_at(index) else { break };
        let y = list.y + line as u16;
        let selected = index == app.view_selected;
        let bg = if selected { p.selected } else { p.bg };
        if selected {
            fill(buf, Rect::new(list.x, y, list.width, 1), Style::default().bg(bg));
        }
        let (status, color) = match download.state.as_str() {
            "active" if download.total > 0 => {
                let fraction = download.received as f64 / download.total as f64;
                let text = format!("{} {:>3}%  {} of {}", progress_bar(fraction, 10), (fraction * 100.0).floor().min(100.0) as u32,
                    views::size(download.received), views::size(download.total));
                (text, p.blue)
            }
            "active" => (format!("{} so far", views::size(download.received)), p.blue),
            "done" => (format!("✓ {}", views::size(download.total.max(download.received))), p.green),
            "cancelled" => ("Cancelled".to_string(), p.dim),
            _ => (format!("Failed: {}", download.error.as_deref().unwrap_or("unknown error")), p.red),
        };
        let gone = !download.is_active() && download.state != "done";
        let mut x = list.x + 1;
        put(buf, x, y, name_w as u16, &download.filename, Style::default().fg(if gone { p.dim } else { p.text }).bg(bg));
        x += name_w as u16 + 2;
        put(buf, x, y, status_w as u16, &status, Style::default().fg(color).bg(bg));
        x += status_w as u16 + 2;
        if host_w > 0 {
            put(buf, x, y, host_w as u16, &display_host(&download.url), Style::default().fg(p.dim).bg(bg));
            x += host_w as u16 + 2;
        }
        let when = if history::is_today(download.started, now) { history::clock(download.started) } else { history::short_date(download.started) };
        put(buf, x, y, (list.x + list.width).saturating_sub(x), &when, Style::default().fg(p.dim).bg(bg));
    }
}

/// Writes one piece of the status line at `x` and moves `x` past it.
fn segment(buf: &mut Buffer, x: &mut u16, bar: Rect, text: &str, style: Style) {
    let end = bar.x + bar.width;
    if *x >= end {
        return;
    }
    let room = end - *x;
    put(buf, *x, bar.y, room, text, style);
    *x += (text.width() as u16).min(room);
}

/// The downloads in progress, as a chip in the status line: what is coming
/// down, a bar that fills, and how far along it is.
fn draw_download_chip(buf: &mut Buffer, app: &App, p: &Palette, x: &mut u16, bar: Rect) {
    const CELLS: usize = 12;
    let active: Vec<_> = app.downloads.iter().filter(|d| d.is_active()).collect();
    let (received, total) = active.iter().fold((0u64, 0u64), |(r, t), d| (r + d.received, t + d.total));
    let known = active.iter().all(|d| d.total > 0);
    let what = if active.len() == 1 { active[0].filename.clone() } else { format!("{} downloads", active.len()) };
    let chip = Style::default().bg(p.field);
    let frame = app.frame;

    segment(buf, x, bar, " ", Style::default().bg(p.panel));
    // The arrow beats, so the chip is seen to be alive even while the
    // numbers stand still.
    let arrow = if (frame / 4).is_multiple_of(2) { p.blue } else { mix(p.blue, p.field, 0.45) };
    segment(buf, x, bar, " ↓ ", chip.fg(arrow).add_modifier(Modifier::BOLD));
    segment(buf, x, bar, &format!("{} ", fit(&what, 28)), chip.fg(p.text).add_modifier(Modifier::BOLD));

    // The bar: a thin line that fills in blue. A glint runs along the
    // filled part; with no known size, a short piece runs back and forth
    // instead.
    let fraction = if known { received as f64 / total.max(1) as f64 } else { 0.0 };
    let filled = ((fraction.clamp(0.0, 1.0) * CELLS as f64).round() as usize).min(CELLS);
    let bounce = {
        let span = (CELLS - 3) * 2;
        let at = frame % span;
        if at > CELLS - 3 { span - at } else { at }
    };
    for cell in 0..CELLS {
        let color = if known {
            if cell >= filled {
                p.line
            } else if filled > 1 && cell == frame % (filled + 6) {
                p.text
            } else {
                p.blue
            }
        } else if (bounce..bounce + 3).contains(&cell) {
            p.blue
        } else {
            p.line
        };
        segment(buf, x, bar, "━", chip.fg(color));
    }
    let amount = if known { format!(" {}% ", (received * 100 / total.max(1)).min(100)) } else { format!(" {} ", views::size(received)) };
    segment(buf, x, bar, &amount, chip.fg(p.text));
}

fn draw_status(buf: &mut Buffer, app: &App) {
    let p = Palette::of(app);
    let bar = app.layout.status;
    if bar.width == 0 || bar.height == 0 {
        return;
    }
    fill(buf, bar, Style::default().bg(p.panel).fg(p.dim));
    let (label, color) = match app.mode {
        // The keys are the dev tools pane's.
        Mode::Nav if app.dev_has_keys() && !app.overlay_open() && app.edit.is_none() => (" DEV ", YELLOW),
        Mode::Nav => (" NAV ", space_color(app.theme().0)),
        Mode::Page => (" PAGE ", GREEN),
    };
    let mut x = bar.x;
    let end = bar.x + bar.width;
    let plain = Style::default().bg(p.panel);
    segment(buf, &mut x, bar, label, Style::default().bg(color).fg(Color::Black).add_modifier(Modifier::BOLD));
    let count = app.tabs.len();
    if app.sidebar_visible {
        segment(buf, &mut x, bar, &format!(" {count} {} ", if count == 1 { "tab" } else { "tabs" }), plain.fg(p.dim));
    } else if let Some(tab) = app.current() {
        // Without the sidebar this is the only place that says which tab is showing.
        let pin = if tab.pinned { "pinned · " } else { "" };
        let label = fit(&tab.label(), 32);
        let space = app.current_space();
        segment(buf, &mut x, bar, &format!(" {} ", fit(&space.name, 16)), plain.fg(p.accent).add_modifier(Modifier::BOLD));
        segment(buf, &mut x, bar, &format!("{}/{count} {pin}{label} ", app.selected + 1), plain.fg(p.text));
    }
    if app.view != Some(View::Downloads) {
        if app.downloads.iter().any(|d| d.is_active()) {
            draw_download_chip(buf, app, &p, &mut x, bar);
        } else if let Some((notice, failed)) = app.download_notice() {
            // What became of the last download, until it has been looked at.
            segment(buf, &mut x, bar, " ", plain);
            let color = if failed { p.red } else { p.green };
            segment(buf, &mut x, bar, &format!(" {} ", fit(&notice, 40)), Style::default().bg(p.field).fg(color).add_modifier(Modifier::BOLD));
        }
    }
    if let Some(message) = &app.message {
        segment(buf, &mut x, bar, &format!(" {message} "), plain.fg(p.yellow));
    } else if let Some(tab) = app.current().filter(|t| t.loading) {
        let percent = (tab.progress * 100.0).round().clamp(0.0, 100.0) as u32;
        segment(buf, &mut x, bar, &format!(" {} ", SPINNER[app.frame % SPINNER.len()]), plain.fg(p.accent));
        segment(buf, &mut x, bar, &format!("Loading… {percent}% "), plain.fg(p.text));
    }

    let hint = if app.view.is_some() {
        // The view lists its own keys along its bottom edge.
        ""
    } else if app.help {
        "any key closes"
    } else if app.form.is_some() || app.confirm_delete {
        ""
    } else if let Some(edit) = &app.edit {
        match edit.kind {
            EditKind::Url => "⏎ go   esc cancel",
            EditKind::Rename => "⏎ rename   esc cancel",
        }
    } else if app.dev_has_keys() && app.mode == Mode::Nav {
        // The pane lists its own keys along its bottom edge.
        ""
    } else if app.dev.open {
        match app.mode {
            Mode::Nav => "j/k tabs   o url   I dev tools   ⏎ page   ? keys",
            Mode::Page => "⌘. tabs   ⌥⌘I dev tools   ⌘L url   ⌘/ keys",
        }
    } else {
        match app.mode {
            Mode::Nav => "j/k tabs   o url   t new   x close   y history   d downloads   ? keys",
            Mode::Page => "⌘. tabs   ⌘L url   ⌘T new   ⌘/ keys",
        }
    };
    let hint_w = hint.width() as u16 + 1;
    if x + hint_w + 1 < end {
        // Each hint is a key and what it does: the key stands out.
        let mut hx = end - hint_w;
        for part in hint.split("   ") {
            let (key, what) = part.split_once(' ').unwrap_or((part, ""));
            put(buf, hx, bar.y, end.saturating_sub(hx), key, plain.fg(p.text));
            hx += key.width() as u16;
            if !what.is_empty() {
                put(buf, hx + 1, bar.y, end.saturating_sub(hx + 1), what, plain.fg(p.dim));
                hx += 1 + what.width() as u16;
            }
            hx += 3;
        }
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
