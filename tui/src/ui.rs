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

/// The surfaces of the interface, all derived from the space's background tint.
#[derive(Clone, Copy)]
struct Palette {
    /// The page area and anything drawn in it.
    bg: Color,
    /// Sidebar and status line.
    panel: Color,
    /// Toolbar.
    bar: Color,
    /// A focused text field.
    field: Color,
    /// The selected tab's row.
    selected: Color,
}

impl Palette {
    fn new(background: usize) -> Self {
        let (_, (r, g, b)) = BACKGROUNDS[background % BACKGROUNDS.len()];
        // Each surface is the tint lifted by a fixed amount, a little more in
        // blue, which is what the original palette did.
        let lift = |n: u8| Color::Rgb(r.saturating_add(n), g.saturating_add(n), b.saturating_add(n + n / 4));
        Self { bg: Color::Rgb(r, g, b), panel: lift(7), bar: lift(15), field: lift(29), selected: lift(36) }
    }

    fn of(app: &App) -> Self {
        Self::new(app.theme().1)
    }
}

const TEXT: Color = Color::Rgb(220, 222, 228);
const DIM: Color = Color::Rgb(120, 123, 137);
const FAINT: Color = Color::Rgb(70, 72, 84);
const ACCENT: Color = Color::Rgb(122, 162, 247);
const GREEN: Color = Color::Rgb(158, 206, 106);
const YELLOW: Color = Color::Rgb(224, 175, 104);
const RED: Color = Color::Rgb(247, 118, 142);

fn space_color(index: usize) -> Color {
    let (_, (r, g, b)) = COLORS[index % COLORS.len()];
    Color::Rgb(r, g, b)
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
    let p = Palette::of(app);
    fill(buf, area, Style::default().bg(p.bg).fg(TEXT));

    draw_sidebar(buf, app);
    let cursor = draw_toolbar(buf, app);
    draw_viewport(buf, app);
    draw_status(buf, app);
    let _ = layout;
    cursor
}

fn draw_sidebar(buf: &mut Buffer, app: &App) {
    let p = Palette::of(app);
    let side = app.layout.sidebar;
    if side.width < 4 || side.height == 0 {
        return;
    }
    let inner_w = side.width - 1;
    fill(buf, Rect::new(side.x, side.y, inner_w, side.height), Style::default().bg(p.panel).fg(TEXT));
    for y in side.y..side.y + side.height {
        buf[(side.x + inner_w, y)].set_symbol("│").set_style(Style::default().fg(FAINT).bg(p.bg));
    }
    // The space: its name in its colour, then one dot per space.
    let (color, _, art_index) = app.theme();
    let accent = space_color(color);
    let space = app.current_space();
    // While a space is being made or edited, the header previews its name.
    let name = match &app.form {
        Some(form) if !form.name.trim().is_empty() => form.name.trim().to_string(),
        Some(form) if !form.editing => "New space".to_string(),
        _ => space.name.clone(),
    };
    // Shown the way a shell shows a folder in your home directory.
    let name = format!("~/{name}");
    put(buf, side.x + 1, side.y, inner_w - 1, &name, Style::default().fg(accent).bg(p.panel).add_modifier(Modifier::BOLD));
    if side.height > 1 {
        for (index, other) in app.spaces.iter().enumerate() {
            let x = side.x + 1 + 2 * index as u16;
            if x + 1 >= side.x + inner_w {
                break;
            }
            let dot = if index == app.space { "●" } else { "○" };
            let dot_color = if index == app.space { accent } else { space_color(other.color) };
            put(buf, x, side.y + 1, 1, dot, Style::default().fg(dot_color).bg(p.panel));
        }
    }

    let list = app.layout.tab_list;
    let rows = app.side_rows();
    for row in 0..list.height as usize {
        let Some(side_row) = rows.get(app.scroll + row).copied() else { break };
        let y = list.y + row as u16;
        let index = match side_row {
            SideRow::Tab(index) => index,
            SideRow::PinPlaceholder => {
                put(buf, list.x + 1, y, list.width.saturating_sub(2), "No pinned tabs · p to pin", Style::default().fg(FAINT).bg(p.panel));
                continue;
            }
            SideRow::Separator => {
                let rule = "─".repeat(list.width.saturating_sub(2) as usize);
                put(buf, list.x + 1, y, list.width.saturating_sub(2), &rule, Style::default().fg(FAINT).bg(p.panel));
                continue;
            }
        };
        let Some(tab) = app.tabs.get(index) else { break };
        let selected = index == app.selected;
        let bg = if selected { p.selected } else { p.panel };
        let mut style = Style::default().bg(bg).fg(if selected { TEXT } else { DIM });
        if selected {
            style = style.add_modifier(Modifier::BOLD);
        }
        fill(buf, Rect::new(list.x, y, list.width, 1), style);
        if selected {
            buf[(list.x, y)].set_symbol("▎").set_style(Style::default().fg(accent).bg(bg));
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
        put(buf, list.x + list.width - 1, list.y, 1, "↑", Style::default().fg(DIM).bg(p.panel));
    }
    if app.scroll + (list.height as usize) < rows.len() && list.height > 0 {
        put(buf, list.x + list.width - 1, list.y + list.height - 1, 1, "↓", Style::default().fg(DIM).bg(p.panel));
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
        put(buf, new_tab.x + 1, new_tab.y, new_tab.width.saturating_sub(1), "+ New tab", Style::default().fg(DIM).bg(p.panel));
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
    fill(buf, bar, Style::default().bg(p.bar).fg(TEXT));
    let tab = app.current();
    let on = Style::default().fg(TEXT).bg(p.bar);
    let off = Style::default().fg(FAINT).bg(p.bar);
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
            fill(buf, inner, Style::default().bg(p.field).fg(TEXT));
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
                put(buf, inner.x + 1, inner.y, text_w as u16, hint, Style::default().fg(DIM).bg(p.field));
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
                Style::default().bg(p.field).fg(TEXT)
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
                put(buf, inner.x + 1, inner.y, text_w as u16, url, Style::default().fg(DIM).bg(p.bar));
            }
            None
        }
    }
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
            (format!("Delete space \"{}\"?", space.name), Style::default().fg(TEXT).bg(p.bg).add_modifier(Modifier::BOLD)),
            (String::new(), Style::default()),
            (format!("Its {count} tab{} will be closed. Logins in its profile are kept.", if count == 1 { "" } else { "s" }), Style::default().fg(DIM).bg(p.bg)),
            (String::new(), Style::default()),
            ("y  delete        any other key  keep".to_string(), Style::default().fg(YELLOW).bg(p.bg)),
        ];
        draw_centered(buf, view, &lines);
        return;
    }
    if app.help {
        draw_help(buf, view, &p);
        return;
    }
    let lines: Vec<(String, Style)> = match app.current() {
        None => vec![("Connecting…".into(), Style::default().fg(DIM).bg(p.bg))],
        Some(_) if !app.ready => vec![("Connecting…".into(), Style::default().fg(DIM).bg(p.bg))],
        Some(tab) if tab.pending || tab.loading => vec![("Opening…".into(), Style::default().fg(DIM).bg(p.bg))],
        Some(_) => vec![
            ("New tab".into(), Style::default().fg(TEXT).bg(p.bg).add_modifier(Modifier::BOLD)),
            (String::new(), Style::default()),
            ("Type a URL or search terms, then press Enter".into(), Style::default().fg(DIM).bg(p.bg)),
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
        put(buf, view.x + 1, view.y, view.width.saturating_sub(1), "Make the window larger to edit the space", Style::default().fg(DIM).bg(p.bg));
        return;
    }
    let x = view.x + (view.width - width) / 2;
    let mut y = view.y + (view.height - 18) / 2;
    let accent = space_color(form.color);
    let label = Style::default().fg(DIM).bg(p.bg);
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
        fill(buf, Rect::new(fx, y, field_w, 1), Style::default().bg(bg).fg(TEXT));
        if value.is_empty() {
            put(buf, fx + 1, y, field_w - 2, hint, Style::default().fg(DIM).bg(bg));
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
        buf.set_stringn(fx + 1, y, &shown, room, Style::default().fg(TEXT).bg(bg));
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
        let color = Color::Rgb(*r, *g, *b);
        if index == form.color {
            put(buf, cx, y, 3, "[●]", Style::default().fg(color).bg(p.bg).add_modifier(Modifier::BOLD));
        } else {
            put(buf, cx + 1, y, 1, "●", Style::default().fg(color).bg(p.bg));
        }
    }
    put(buf, fx, y + 1, field_w, &format!("{}{}", COLORS[form.color % COLORS.len()].0, arrows(on)), label);
    y += 3;

    // Background: swatches of each tint. They are dark by design, so each is
    // shown a few steps lighter than it really is, with the chosen one marked.
    let on = form.field == FormField::Background;
    put(buf, x, y, indent - 1, "Background", name_style(on));
    for (index, (_, (r, g, b))) in BACKGROUNDS.iter().enumerate() {
        let cx = fx + 4 * index as u16;
        if cx + 3 > x + width {
            break;
        }
        let swatch = Color::Rgb(r.saturating_add(34), g.saturating_add(34), b.saturating_add(40));
        let chosen = index == form.background;
        let (left, right) = if chosen { ("[", "]") } else { (" ", " ") };
        put(buf, cx, y, 1, left, Style::default().fg(TEXT).bg(p.bg).add_modifier(Modifier::BOLD));
        put(buf, cx + 1, y, 1, " ", Style::default().bg(swatch));
        put(buf, cx + 2, y, 1, right, Style::default().fg(TEXT).bg(p.bg).add_modifier(Modifier::BOLD));
    }
    put(buf, fx, y + 1, field_w, &format!("{}{}", BACKGROUNDS[form.background % BACKGROUNDS.len()].0, arrows(on)), label);
    y += 3;

    // Art: the name of the piece; the sidebar shows the piece itself.
    let on = form.field == FormField::Art;
    put(buf, x, y, indent - 1, "Art", name_style(on));
    let choice = format!("‹ {} ›", art::art_name(form.art));
    put(buf, fx, y, field_w, &choice, if on { Style::default().fg(TEXT).bg(p.bg).add_modifier(Modifier::BOLD) } else { Style::default().fg(TEXT).bg(p.bg) });
    put(buf, fx, y + 1, field_w, &format!("shown under the tabs{}", arrows(on)), label);
    y += 3;

    let own = form.resolved_profile();
    let hint = if own.is_empty() { "its own, named after the space".to_string() } else { format!("its own: {own}") };
    text_field(buf, y, "Profile", &form.profile, &hint, form.field == FormField::Profile);
    put(buf, fx, y + 1, field_w, "Spaces with the same profile share logins.", label);
    put(buf, fx, y + 2, field_w, "\"default\" is the profile of the first space.", label);
    y += 4;

    if let Some(error) = &form.error {
        put(buf, x, y, width, error, Style::default().fg(RED).bg(p.bg));
    } else {
        put(buf, x, y, width, "tab next field    ⏎ save    esc cancel", Style::default().fg(FAINT).bg(p.bg));
    }
}

/// The key reference. It takes the place of the page, which cannot be drawn over.
fn draw_help(buf: &mut Buffer, view: Rect, p: &Palette) {
    let key_style = Style::default().fg(ACCENT).bg(p.bg).add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(TEXT).bg(p.bg);
    let head_style = Style::default().fg(YELLOW).bg(p.bg).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(DIM).bg(p.bg);

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
    put(buf, list.x, y, list.width, name, Style::default().fg(TEXT).bg(p.bg).add_modifier(Modifier::BOLD));
    let x = list.x + name.width() as u16 + 2;
    if x < list.x + list.width {
        put(buf, x, y, list.x + list.width - x, detail, Style::default().fg(DIM).bg(p.bg));
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
        put(buf, x, y, end - x, key, Style::default().fg(ACCENT).bg(p.bg).add_modifier(Modifier::BOLD));
        put(buf, x + key.width() as u16 + 1, y, end - x, what, Style::default().fg(DIM).bg(p.bg));
        x += width + 3;
    }
}

fn draw_view_empty(buf: &mut Buffer, app: &App, p: &Palette, text: &str) {
    let list = app.view_list();
    if list.height > 0 {
        put(buf, list.x, list.y, list.width, text, Style::default().fg(DIM).bg(p.bg));
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
            Style::default().fg(YELLOW).bg(p.bg));
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
            Row::Header(title) => put(buf, list.x, y, list.width, title, Style::default().fg(YELLOW).bg(p.bg).add_modifier(Modifier::BOLD)),
            Row::Item(index) => {
                let Some(visit) = app.history.get(*index) else { continue };
                let selected = *index == app.view_selected;
                let bg = if selected { p.selected } else { p.bg };
                if selected {
                    fill(buf, Rect::new(list.x, y, list.width, 1), Style::default().bg(bg));
                }
                let mut x = list.x + 1;
                put(buf, x, y, 5, &history::clock(visit.time), Style::default().fg(DIM).bg(bg));
                x += 7;
                let title = if visit.title.is_empty() { visit.url.as_str() } else { visit.title.as_str() };
                put(buf, x, y, title_w as u16, title, Style::default().fg(TEXT).bg(bg));
                x += title_w as u16 + 2;
                if host_w > 0 {
                    put(buf, x, y, host_w as u16, &display_host(&visit.url), Style::default().fg(DIM).bg(bg));
                    x += host_w as u16 + 2;
                }
                // The space is shown in its colour while it still exists.
                let color = app.spaces.iter().find(|s| s.name == visit.space).map(|s| space_color(s.color)).unwrap_or(FAINT);
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
                (text, ACCENT)
            }
            "active" => (format!("{} so far", views::size(download.received)), ACCENT),
            "done" => (format!("✓ {}", views::size(download.total.max(download.received))), GREEN),
            "cancelled" => ("Cancelled".to_string(), DIM),
            _ => (format!("Failed: {}", download.error.as_deref().unwrap_or("unknown error")), RED),
        };
        let gone = !download.is_active() && download.state != "done";
        let mut x = list.x + 1;
        put(buf, x, y, name_w as u16, &download.filename, Style::default().fg(if gone { DIM } else { TEXT }).bg(bg));
        x += name_w as u16 + 2;
        put(buf, x, y, status_w as u16, &status, Style::default().fg(color).bg(bg));
        x += status_w as u16 + 2;
        if host_w > 0 {
            put(buf, x, y, host_w as u16, &display_host(&download.url), Style::default().fg(DIM).bg(bg));
            x += host_w as u16 + 2;
        }
        let when = if history::is_today(download.started, now) { history::clock(download.started) } else { history::short_date(download.started) };
        put(buf, x, y, (list.x + list.width).saturating_sub(x), &when, Style::default().fg(DIM).bg(bg));
    }
}

fn draw_status(buf: &mut Buffer, app: &App) {
    let p = Palette::of(app);
    let bar = app.layout.status;
    if bar.width == 0 || bar.height == 0 {
        return;
    }
    fill(buf, bar, Style::default().bg(p.panel).fg(DIM));
    let (label, color) = match app.mode {
        Mode::Nav => (" NAV ", space_color(app.theme().0)),
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
    if app.sidebar_visible {
        segment(buf, &format!(" {count} {} ", if count == 1 { "tab" } else { "tabs" }), Style::default().bg(p.panel).fg(DIM));
    } else if let Some(tab) = app.current() {
        // Without the sidebar this is the only place that says which tab is showing.
        let pin = if tab.pinned { "pinned · " } else { "" };
        let label = fit(&tab.label(), 32);
        let space = app.current_space();
        segment(buf, &format!(" {} ", fit(&space.name, 16)), Style::default().bg(p.panel).fg(space_color(space.color)).add_modifier(Modifier::BOLD));
        segment(buf, &format!("{}/{count} {pin}{label} ", app.selected + 1), Style::default().bg(p.panel).fg(TEXT));
    }
    if let Some((notice, failed)) = app.download_notice().filter(|_| app.view != Some(View::Downloads)) {
        segment(buf, &format!(" {} ", fit(&notice, 40)), Style::default().bg(p.panel).fg(if failed { RED } else { ACCENT }));
    }
    if let Some(message) = &app.message {
        segment(buf, &format!(" {message} "), Style::default().bg(p.panel).fg(YELLOW));
    } else if let Some(tab) = app.current().filter(|t| t.loading) {
        let percent = (tab.progress * 100.0).round().clamp(0.0, 100.0) as u32;
        segment(buf, &format!(" Loading… {percent}% "), Style::default().bg(p.panel).fg(YELLOW));
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
    } else {
        match app.mode {
            Mode::Nav => "j/k tabs   o url   t new   x close   y history   d downloads   ? keys",
            Mode::Page => "⌘. tabs   ⌘L url   ⌘T new   ⌘/ keys",
        }
    };
    let hint_w = hint.width() as u16 + 1;
    if x + hint_w + 1 < end {
        put(buf, end - hint_w, bar.y, hint_w, hint, Style::default().bg(p.panel).fg(FAINT));
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
