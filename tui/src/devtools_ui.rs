//! Drawing of the dev tools pane. Reads `App`, never changes it.

use crate::app::App;
use crate::devtools::{self, Dock, ItemRef, NetFilter, NetTab, StorageKind, Tone, Tool};
use crate::keys;
use crate::protocol::{Mode, NetRequest};
use crate::ui::{fill, fit, put, Palette};
use crate::views::size;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

fn tone_color(tone: Tone, p: &Palette) -> Color {
    match tone {
        Tone::Text => p.text,
        Tone::Dim => p.dim,
        Tone::Key => p.blue,
        Tone::Head => p.yellow,
        Tone::Good => p.green,
        Tone::Warn => p.yellow,
        Tone::Bad => p.red,
        Tone::Accent => p.purple,
    }
}

/// What the pane is drawn with: its surfaces, and whether it has the keys.
struct Look {
    bg: Color,
    bar: Color,
    field: Color,
    selected: Color,
    accent: Color,
    focused: bool,
    /// The colours of text.
    p: Palette,
}

/// Draws the pane, if it is open, and returns where the text cursor
/// belongs when its filter field is being typed in.
pub fn draw(buf: &mut Buffer, app: &App, p: &Palette) -> Option<Position> {
    let pane = app.layout.devtools;
    if !app.dev.open || pane.width == 0 || pane.height == 0 {
        return None;
    }
    let look = Look {
        bg: p.panel,
        bar: p.bar,
        field: p.field,
        selected: p.selected,
        accent: p.accent,
        p: *p,
        focused: app.dev_has_keys() && app.mode == Mode::Nav && !app.overlay_open() && app.edit.is_none(),
    };
    let inner = app.dev_inner();
    fill(buf, inner, Style::default().bg(look.bg).fg(p.text));
    if app.dev.dock == Dock::Right {
        for y in pane.y..pane.y + pane.height {
            buf[(pane.x, y)].set_symbol("│").set_style(Style::default().fg(p.line).bg(p.bg));
        }
    }
    draw_header(buf, app, &look);
    let cursor = if app.current().is_none_or(|tab| tab.tab_id.is_none()) {
        let body = app.dev_body();
        lines(buf, body, look.bg, &look.p, &["This tab has no page yet.", "", "Open one to see its requests and what it stores."]);
        None
    } else {
        match app.dev.tool {
            Tool::Network => draw_network(buf, app, &look),
            Tool::Storage => draw_storage(buf, app, &look),
        }
    };
    draw_keys(buf, app, &look);
    cursor
}

/// A few dim lines of explanation, a little in from the corner.
fn lines(buf: &mut Buffer, area: Rect, bg: Color, p: &Palette, text: &[&str]) {
    for (i, line) in text.iter().enumerate() {
        let y = area.y + 1 + i as u16;
        if y >= area.y + area.height || area.width < 4 {
            break;
        }
        let style = if i == 0 { Style::default().fg(p.text).bg(bg) } else { Style::default().fg(p.dim).bg(bg) };
        put(buf, area.x + 2, y, area.width - 3, line, style);
    }
}

/// The pane's top line: its tools, and what the showing one holds.
fn draw_header(buf: &mut Buffer, app: &App, look: &Look) {
    let inner = app.dev_inner();
    let bar = Rect::new(inner.x, inner.y, inner.width, inner.height.min(1));
    fill(buf, bar, Style::default().bg(look.bar).fg(look.p.dim));
    let mut end = inner.x;
    for (tool, rect) in app.dev_tool_tabs() {
        let style = if tool != app.dev.tool {
            Style::default().fg(look.p.dim).bg(look.bar)
        } else if look.focused {
            Style::default().fg(look.accent).bg(look.selected).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(look.p.text).bg(look.field)
        };
        fill(buf, rect, style);
        put(buf, rect.x + 1, rect.y, rect.width - 1, tool.label(), style);
        end = rect.x + rect.width;
    }
    let summary = match app.dev.tool {
        Tool::Network => {
            let net = &app.dev.net;
            let shown = net.visible();
            let total = net.requests.len();
            let bytes: u64 = shown.iter().filter_map(|&i| net.requests[i].size).sum();
            let mut text = if shown.len() == total {
                format!("{total} request{}", if total == 1 { "" } else { "s" })
            } else {
                format!("{} of {total} requests", shown.len())
            };
            if bytes > 0 {
                text.push_str(&format!(" · {}", size(bytes)));
            }
            if net.preserve {
                text.push_str(" · kept");
            }
            if net.follow {
                text.push_str(" · following");
            }
            text
        }
        Tool::Storage if app.dev.storage.loading => "reading…".to_string(),
        Tool::Storage => {
            let sites = app.dev.storage.sites.len();
            format!("{sites} site{} in profile {}", if sites == 1 { "" } else { "s" }, app.current_space().profile)
        }
    };
    let room = (inner.x + inner.width).saturating_sub(end + 3);
    let shown = fit(&summary, room as usize);
    let width = shown.width() as u16;
    if width > 0 && room >= 8 {
        put(buf, inner.x + inner.width - width - 1, bar.y, width, &shown, Style::default().fg(look.p.dim).bg(look.bar));
    }
}

/// The names of the keys, along the bottom of the pane.
fn draw_keys(buf: &mut Buffer, app: &App, look: &Look) {
    let rect = app.dev_keys_rect();
    if rect.height == 0 || rect.width < 8 {
        return;
    }
    let bg = look.bg;
    if let Some(question) = app.dev_question() {
        put(buf, rect.x + 1, rect.y, rect.width - 2, &question, Style::default().fg(look.p.yellow).bg(bg).add_modifier(Modifier::BOLD));
        let answer = "y deletes   any other key keeps it";
        if rect.height > 1 {
            put(buf, rect.x + 1, rect.y + 1, rect.width - 2, answer, Style::default().fg(look.p.yellow).bg(bg));
        } else {
            let x = rect.x + 1 + question.width() as u16 + 3;
            if x < rect.x + rect.width {
                put(buf, x, rect.y, rect.x + rect.width - x - 1, answer, Style::default().fg(look.p.yellow).bg(bg));
            }
        }
        return;
    }
    if !look.focused {
        let hint = match app.mode {
            Mode::Page => "⌥⌘I gives the keys to dev tools",
            Mode::Nav => "I gives the keys to dev tools",
        };
        put(buf, rect.x + 1, rect.y + rect.height - 1, rect.width - 2, hint, Style::default().fg(look.p.dim).bg(bg));
        return;
    }
    let keys: &[(&str, &str)] = if app.dev.search.is_some() {
        keys::DEV_SEARCH
    } else {
        match app.dev.tool {
            Tool::Network if app.dev.net.detail_open => keys::NET_DETAIL,
            Tool::Network => keys::NET_LIST,
            Tool::Storage if app.dev.storage.picking => keys::STORAGE_SITES,
            Tool::Storage if app.dev.storage.detail_open => keys::STORAGE_DETAIL,
            Tool::Storage => keys::STORAGE_LIST,
        }
    };
    let end = rect.x + rect.width - 1;
    let (mut x, mut y) = (rect.x + 1, rect.y);
    for (key, what) in keys {
        let width = (key.width() + 1 + what.width()) as u16;
        if x + width > end {
            if y + 1 >= rect.y + rect.height {
                break;
            }
            x = rect.x + 1;
            y += 1;
        }
        put(buf, x, y, end - x, key, Style::default().fg(look.p.blue).bg(bg).add_modifier(Modifier::BOLD));
        put(buf, x + key.width() as u16 + 1, y, end.saturating_sub(x + key.width() as u16 + 1), what, Style::default().fg(look.p.dim).bg(bg));
        x += width + 3;
    }
}

/// The filter field while it is typed in. Returns where its cursor is.
fn draw_search(buf: &mut Buffer, app: &App, look: &Look, row: Rect) -> Option<Position> {
    let edit = app.dev.search.as_ref()?;
    if row.width < 6 || row.height == 0 {
        return None;
    }
    let field = Rect::new(row.x + 1, row.y, row.width - 2, 1);
    fill(buf, field, Style::default().bg(look.field).fg(look.p.text));
    put(buf, field.x, field.y, 2, "/", Style::default().fg(look.p.yellow).bg(look.field).add_modifier(Modifier::BOLD));
    let room = (field.width - 3) as usize;
    // Show the end of a long text, where typing happens.
    let chars: Vec<char> = edit.text.chars().collect();
    let cursor = edit.cursor.min(chars.len());
    let mut start = cursor;
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
    if shown.is_empty() {
        put(buf, field.x + 2, field.y, room as u16, "type to filter", Style::default().fg(look.p.dim).bg(look.field));
    } else {
        buf.set_stringn(field.x + 2, field.y, &shown, room, Style::default().fg(look.p.text).bg(look.field));
    }
    Some(Position::new(field.x + 2 + used as u16, field.y))
}

/// A filter that is in force, named at the right end of a line.
fn draw_search_note(buf: &mut Buffer, row: Rect, from: u16, text: &str, bg: Color, p: &Palette) {
    if text.is_empty() {
        return;
    }
    let note = format!("/{text}");
    let room = (row.x + row.width).saturating_sub(from + 2);
    let shown = fit(&note, room as usize);
    let width = shown.width() as u16;
    if width >= 2 {
        put(buf, row.x + row.width - width - 1, row.y, width, &shown, Style::default().fg(p.yellow).bg(bg));
    }
}

/// A row of names, one of them current.
fn draw_chips<T: PartialEq + Copy>(buf: &mut Buffer, y: u16, chips: &[(T, u16, u16, String)], current: T, look: &Look, strong: bool) -> u16 {
    let mut end = 0;
    for (item, x, width, label) in chips {
        let style = if *item != current {
            Style::default().fg(look.p.dim).bg(look.bg)
        } else if strong {
            Style::default().fg(look.p.text).bg(look.selected).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(look.p.text).bg(look.field)
        };
        fill(buf, Rect::new(*x, y, *width, 1), style);
        put(buf, x + 1, y, width - 1, label, style);
        end = x + width;
    }
    end
}

/// The background of a list line, and its marker when it is the selected one.
fn list_line(buf: &mut Buffer, list: Rect, y: u16, selected: bool, active: bool, look: &Look) -> Color {
    if !selected {
        return look.bg;
    }
    let bg = if active { look.selected } else { look.field };
    fill(buf, Rect::new(list.x, y, list.width, 1), Style::default().bg(bg));
    if active {
        buf[(list.x, y)].set_symbol("▎").set_style(Style::default().fg(look.accent).bg(bg));
    }
    bg
}

fn scroll_marks(buf: &mut Buffer, area: Rect, scroll: usize, len: usize, bg: Color, p: &Palette) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let x = area.x + area.width - 1;
    if scroll > 0 {
        put(buf, x, area.y, 1, "↑", Style::default().fg(p.dim).bg(bg));
    }
    if scroll + (area.height as usize) < len {
        put(buf, x, area.y + area.height - 1, 1, "↓", Style::default().fg(p.dim).bg(bg));
    }
}

/// The rule between the list and the details of a wide pane.
fn draw_rule(buf: &mut Buffer, app: &App, look: &Look) {
    let (column, detail) = app.dev_columns();
    if !app.dev_wide() || detail.is_none() {
        return;
    }
    for y in column.y..column.y + column.height {
        buf[(column.x + column.width, y)].set_symbol("│").set_style(Style::default().fg(look.p.line).bg(look.bg));
    }
}

/// The laid-out text of the details, from the line it is scrolled to.
fn draw_rows(buf: &mut Buffer, app: &App, look: &Look) {
    let text = app.dev_detail_text();
    if text.width == 0 {
        return;
    }
    let rows = &app.dev.rows;
    for (line, row) in rows.iter().skip(app.dev.detail_scroll).take(text.height as usize).enumerate() {
        let y = text.y + line as u16;
        let mut x = text.x;
        let end = text.x + text.width;
        for (piece, tone) in row {
            if x >= end {
                break;
            }
            let mut style = Style::default().fg(tone_color(*tone, &look.p)).bg(look.bg);
            if *tone == Tone::Head {
                style = style.add_modifier(Modifier::BOLD);
            }
            buf.set_stringn(x, y, piece, (end - x) as usize, style);
            x += piece.width() as u16;
        }
    }
    scroll_marks(buf, Rect::new(text.x, text.y, text.width + 1, text.height), app.dev.detail_scroll, rows.len(), look.bg, &look.p);
}

// ----- network -----

fn request_color(request: &NetRequest, p: &Palette) -> Color {
    tone_color(devtools::status_tone(request), p)
}

fn draw_network(buf: &mut Buffer, app: &App, look: &Look) -> Option<Position> {
    let (column, detail) = app.dev_columns();
    let mut cursor = None;
    if column.width > 0 {
        cursor = draw_net_list(buf, app, look, column);
    }
    if let Some(area) = detail {
        draw_rule(buf, app, look);
        draw_net_detail(buf, app, look, area);
    }
    cursor
}

fn draw_net_list(buf: &mut Buffer, app: &App, look: &Look, column: Rect) -> Option<Position> {
    let net = &app.dev.net;
    let bg = look.bg;
    let active = look.focused && !net.detail_open;
    let mut cursor = None;

    // The filters, or the search field while it is typed in.
    if column.height > 0 {
        let row = Rect::new(column.x, column.y, column.width, 1);
        if app.dev.search.is_some() {
            cursor = draw_search(buf, app, look, row);
        } else {
            let chips = app.net_chips();
            let end = if chips.is_empty() {
                put(buf, row.x + 1, row.y, 7, "Filter", Style::default().fg(look.p.dim).bg(bg));
                let label = format!("‹ {} ›", net.filter.label());
                put(buf, row.x + 8, row.y, row.width.saturating_sub(9), &label, Style::default().fg(look.p.text).bg(bg).add_modifier(Modifier::BOLD));
                row.x + 8 + label.width() as u16
            } else {
                let chips: Vec<_> = chips.into_iter().map(|(f, x, w): (NetFilter, u16, u16)| (f, x, w, f.label().to_string())).collect();
                draw_chips(buf, row.y, &chips, net.filter, look, active)
            };
            draw_search_note(buf, row, end, &net.search, bg, &look.p);
        }
    }

    // Which columns there is room for.
    let list = app.dev_list();
    let width = list.width;
    let method_w: u16 = if width >= 58 { 7 } else { 4 };
    let type_w: u16 = if width >= 64 { 5 } else { 0 };
    let size_w: u16 = if width >= 50 { 8 } else { 0 };
    let time_w: u16 = 8;
    let gap = |w: u16| if w > 0 { w + 1 } else { 0 };
    // The last two columns stay clear, for the mark that says there is more below.
    let fixed = 1 + 3 + 1 + method_w + 1 + gap(type_w) + gap(size_w) + time_w + 3;
    let name_w = width.saturating_sub(fixed);
    let x_status = list.x + 1;
    let x_method = x_status + 4;
    let x_name = x_method + method_w + 1;
    let x_type = x_name + name_w + 1;
    let x_size = x_type + gap(type_w);
    let x_time = x_size + gap(size_w);

    // A page that was there before the pane says what that costs, and the way out.
    let notice = u16::from(net.late);
    if net.late && column.height > 2 {
        let y = column.y + 1;
        let text = if width >= 62 { "This page loaded before dev tools opened, so some requests lack detail." } else { "Loaded before dev tools opened." };
        put(buf, column.x + 1, y, 1, "!", Style::default().fg(look.p.yellow).bg(bg).add_modifier(Modifier::BOLD));
        put(buf, column.x + 3, y, width.saturating_sub(4), text, Style::default().fg(look.p.yellow).bg(bg));
        let x = column.x + 3 + text.width() as u16 + 2;
        let end = column.x + width - 1;
        if x + 10 <= end {
            put(buf, x, y, 1, "r", Style::default().fg(look.p.blue).bg(bg).add_modifier(Modifier::BOLD));
            put(buf, x + 2, y, end - x - 2, "reloads it", Style::default().fg(look.p.dim).bg(bg));
        }
    }
    if column.height > 1 + notice && name_w >= 4 {
        let y = column.y + 1 + notice;
        let head = Style::default().fg(look.p.dim).bg(bg);
        put(buf, x_status, y, 3, "St", head);
        if method_w >= 6 {
            put(buf, x_method, y, method_w, "Method", head);
        }
        put(buf, x_name, y, name_w, "Name", head);
        if type_w > 0 {
            put(buf, x_type, y, type_w, "Type", head);
        }
        if size_w > 0 {
            put(buf, x_size, y, size_w, &format!("{:>8}", "Size"), head);
        }
        put(buf, x_time, y, time_w, &format!("{:>8}", "Time"), head);
    }

    let visible = net.visible();
    if visible.is_empty() {
        if net.requests.is_empty() {
            lines(buf, list, bg, &look.p, &["No requests yet.", "", "They are recorded while dev tools are open.", "Press r to reload the page and see them all."]);
        } else {
            lines(buf, list, bg, &look.p, &["No request matches.", "", "esc clears the search, then the filter."]);
        }
        return cursor;
    }
    if name_w < 4 {
        return cursor;
    }
    let page = app.current().map(|tab| tab.url.as_str()).unwrap_or("");
    for (line, &index) in visible.iter().skip(net.scroll).take(list.height as usize).enumerate() {
        let request = &net.requests[index];
        let y = list.y + line as u16;
        let bg = list_line(buf, list, y, net.selected == Some(request.id), active, look);
        let failed = request.state == "failed";
        let pending = request.state == "pending";
        put(buf, x_status, y, 3, &devtools::status_label(request), Style::default().fg(request_color(request, &look.p)).bg(bg).add_modifier(Modifier::BOLD));
        let method_color = if request.method == "GET" { look.p.dim } else { look.p.text };
        put(buf, x_method, y, method_w, &request.method, Style::default().fg(method_color).bg(bg));
        let name = devtools::short_name(&request.url);
        let name_color = if failed { look.p.red } else if pending { look.p.dim } else { look.p.text };
        put(buf, x_name, y, name_w, &name, Style::default().fg(name_color).bg(bg));
        // A request to another site says which, when there is room.
        if let Some(host) = devtools::foreign_host(&request.url, page) {
            let used = name.width() as u16 + 2;
            if name_w > used + 6 {
                put(buf, x_name + used, y, name_w - used, &host, Style::default().fg(look.p.dim).bg(bg));
            }
        }
        if type_w > 0 {
            put(buf, x_type, y, type_w, &request.kind, Style::default().fg(look.p.dim).bg(bg));
        }
        if size_w > 0 {
            let text = match request.size {
                Some(bytes) => size(bytes),
                None if request.cached => "cache".to_string(),
                None => "—".to_string(),
            };
            put(buf, x_size, y, size_w, &format!("{text:>8}"), Style::default().fg(look.p.dim).bg(bg));
        }
        let (time, color) = match (request.duration, failed, pending) {
            (_, true, _) => ("failed".to_string(), look.p.red),
            (Some(duration), _, _) => (devtools::millis(duration), look.p.dim),
            (None, _, true) => ("pending".to_string(), look.p.yellow),
            (None, _, false) => ("—".to_string(), look.p.dim),
        };
        put(buf, x_time, y, time_w, &format!("{time:>8}"), Style::default().fg(color).bg(bg));
    }
    scroll_marks(buf, list, net.scroll, visible.len(), look.bg, &look.p);
    cursor
}

fn draw_net_detail(buf: &mut Buffer, app: &App, look: &Look, area: Rect) {
    let net = &app.dev.net;
    let bg = look.bg;
    let Some(request) = net.selected_request() else {
        lines(buf, area, bg, &look.p, &["No request selected."]);
        return;
    };
    if area.width < 10 || area.height == 0 {
        return;
    }
    // What it is: status, method, name.
    let mut x = area.x + 1;
    let end = area.x + area.width - 1;
    let status = devtools::status_label(request);
    put(buf, x, area.y, 3, &status, Style::default().fg(request_color(request, &look.p)).bg(bg).add_modifier(Modifier::BOLD));
    x += 4;
    put(buf, x, area.y, end.saturating_sub(x), &request.method, Style::default().fg(look.p.dim).bg(bg));
    x += request.method.width() as u16 + 1;
    if x < end {
        put(buf, x, area.y, end - x, &devtools::short_name(&request.url), Style::default().fg(look.p.text).bg(bg).add_modifier(Modifier::BOLD));
    }
    if area.height > 1 {
        let strong = look.focused && net.detail_open;
        let tabs: Vec<_> = app.net_tabs().into_iter().map(|(t, x, w): (NetTab, u16, u16)| (t, x, w, t.label().to_string())).collect();
        draw_chips(buf, area.y + 1, &tabs, net.tab, look, strong);
    }
    draw_rows(buf, app, look);
}

// ----- storage -----

fn draw_storage(buf: &mut Buffer, app: &App, look: &Look) -> Option<Position> {
    let (column, detail) = app.dev_columns();
    let mut cursor = None;
    if column.width > 0 {
        cursor = if app.dev.storage.picking { draw_sites(buf, app, look, column) } else { draw_storage_list(buf, app, look, column) };
    }
    if let Some(area) = detail {
        draw_rule(buf, app, look);
        draw_storage_detail(buf, app, look, area);
    }
    cursor
}

fn draw_storage_list(buf: &mut Buffer, app: &App, look: &Look, column: Rect) -> Option<Position> {
    let storage = &app.dev.storage;
    let bg = look.bg;
    let active = look.focused && !storage.detail_open;
    let mut cursor = None;
    let site = storage.site_name();

    // The site whose storage this is, or the search field.
    if column.height > 0 {
        let row = Rect::new(column.x, column.y, column.width, 1);
        if app.dev.search.is_some() {
            cursor = draw_search(buf, app, look, row);
        } else {
            let mut x = row.x + 1;
            let end = row.x + row.width - 1;
            put(buf, x, row.y, 5, "Site", Style::default().fg(look.p.dim).bg(bg));
            x += 5;
            let name = if site.is_empty() { "none".to_string() } else { site.clone() };
            put(buf, x, row.y, end.saturating_sub(x), &name, Style::default().fg(look.accent).bg(bg).add_modifier(Modifier::BOLD));
            x += (name.width() as u16).min(end.saturating_sub(x)) + 1;
            if x < end {
                put(buf, x, row.y, end - x, "▾", Style::default().fg(look.p.dim).bg(bg));
                x += 3;
            }
            let note = if storage.on_page_site() { "the page in this tab" } else if storage.data.is_some() && !site.is_empty() { "not the page in this tab" } else { "" };
            if x < end && end - x >= note.width() as u16 {
                put(buf, x, row.y, end - x, note, Style::default().fg(look.p.dim).bg(bg));
                x += note.width() as u16;
            }
            draw_search_note(buf, row, x, &storage.search, bg, &look.p);
        }
    }
    if column.height > 1 {
        draw_chips(buf, column.y + 1, &app.storage_chips(), storage.kind, look, active);
    }

    let list = app.dev_list();
    let Some(data) = &storage.data else {
        let text: &[&str] = if storage.loading { &["Reading what this profile stores…"] } else { &["Nothing has been read yet.", "", "Press r to read the storage."] };
        lines(buf, list, bg, &look.p, text);
        return cursor;
    };
    let items = storage.items(app.now());
    if items.is_empty() {
        let kind = storage.kind;
        if !storage.search.is_empty() {
            lines(buf, list, bg, &look.p, &["Nothing matches.", "", "esc clears the search."]);
        } else if kind != StorageKind::Cookies && !storage.on_page_site() {
            let first = format!("The {} of {site} can be read", kind.long());
            let stores = storage.sites.iter().find(|s| s.name == site).map(|s| s.kinds.join(", ")).unwrap_or_default();
            let also = if stores.is_empty() { String::new() } else { format!("It stores: {stores}.") };
            lines(buf, list, bg, &look.p, &[&first, "while a page of it is open in this tab.", "", &also]);
        } else if data.origin.is_empty() && kind != StorageKind::Cookies {
            lines(buf, list, bg, &look.p, &["This page has no storage of its own."]);
        } else {
            let first = format!("No {}.", kind.long());
            let origin = if kind == StorageKind::Cookies { format!("{site} has set none in this profile.") } else { format!("{} has none.", data.origin) };
            lines(buf, list, bg, &look.p, &[&first, "", &origin]);
        }
        return cursor;
    }

    // Name and value, then for cookies their domain and expiry when there is room.
    let width = list.width;
    let cookies = storage.kind == StorageKind::Cookies;
    let when_w: u16 = if cookies && width >= 60 { 12 } else { 0 };
    let extra_w: u16 = if cookies && width >= 92 { 24 } else { 0 };
    let gap = |w: u16| if w > 0 { w + 2 } else { 0 };
    let rest = width.saturating_sub(2 + gap(when_w) + gap(extra_w) + 2);
    let name_w = (rest * 2 / 5).clamp(8.min(rest), 34);
    let value_w = rest.saturating_sub(name_w);
    let x_name = list.x + 1;
    let x_value = x_name + name_w + 2;
    let x_extra = x_value + value_w + 2;
    let x_when = x_extra + gap(extra_w);
    if column.height > 2 {
        let y = column.y + 2;
        let head = Style::default().fg(look.p.dim).bg(bg);
        let (name, value) = match storage.kind {
            StorageKind::Cookies => ("Name", "Value"),
            StorageKind::Local | StorageKind::Session => ("Key", "Value"),
            StorageKind::IndexedDb => ("Database › store", "Holds"),
            StorageKind::Cache => ("Cache", "Holds"),
        };
        put(buf, x_name, y, name_w, name, head);
        put(buf, x_value, y, value_w, value, head);
        if extra_w > 0 {
            put(buf, x_extra, y, extra_w, "Domain", head);
        }
        if when_w > 0 {
            put(buf, x_when, y, when_w, "Expires", head);
        }
    }
    for (line, item) in items.iter().enumerate().skip(storage.scroll).take(list.height as usize) {
        let y = list.y + (line - storage.scroll) as u16;
        let bg = list_line(buf, list, y, line == storage.selected, active, look);
        put(buf, x_name, y, name_w, &item.name, Style::default().fg(look.p.text).bg(bg));
        let value: String = item.value.chars().take(400).map(|c| if c.is_control() { ' ' } else { c }).collect();
        let value_color = if matches!(item.source, ItemRef::Store(..) | ItemRef::Cache(_)) { look.p.dim } else { look.p.green };
        put(buf, x_value, y, value_w, &value, Style::default().fg(value_color).bg(bg));
        if extra_w > 0 {
            put(buf, x_extra, y, extra_w, &item.extra, Style::default().fg(look.p.dim).bg(bg));
        }
        if when_w > 0 {
            let color = if item.when == "Expired" { look.p.red } else { look.p.dim };
            put(buf, x_when, y, when_w, &item.when, Style::default().fg(color).bg(bg));
        }
    }
    scroll_marks(buf, list, storage.scroll, items.len(), look.bg, &look.p);
    cursor
}

/// The list of every site with data in the profile.
fn draw_sites(buf: &mut Buffer, app: &App, look: &Look, column: Rect) -> Option<Position> {
    let storage = &app.dev.storage;
    let bg = look.bg;
    let sites = storage.picker_sites();
    let mut cursor = None;
    if column.height > 0 {
        let row = Rect::new(column.x, column.y, column.width, 1);
        if app.dev.search.is_some() {
            cursor = draw_search(buf, app, look, row);
        } else {
            put(buf, row.x + 1, row.y, 6, "Sites", Style::default().fg(look.p.text).bg(bg).add_modifier(Modifier::BOLD));
            let detail = format!("{} with data in profile {}", storage.sites.len(), app.current_space().profile);
            put(buf, row.x + 8, row.y, row.width.saturating_sub(9), &detail, Style::default().fg(look.p.dim).bg(bg));
            draw_search_note(buf, row, row.x + 8 + detail.width() as u16, &storage.pick_search, bg, &look.p);
        }
    }
    let list = app.dev_list();
    if sites.is_empty() {
        lines(buf, list, bg, &look.p, &[if storage.pick_search.is_empty() { "No site has stored anything in this profile." } else { "No site matches." }]);
        return cursor;
    }
    let showing = storage.site_name();
    let width = list.width;
    let count_w: u16 = 12;
    let kinds_w: u16 = if width >= 76 { (width - 46).min(44) } else { 0 };
    let name_w = width.saturating_sub(1 + 2 + count_w + if kinds_w > 0 { kinds_w + 2 } else { 0 } + 1);
    for (line, site) in sites.iter().enumerate().skip(storage.pick_scroll).take(list.height as usize) {
        let y = list.y + (line - storage.pick_scroll) as u16;
        let bg = list_line(buf, list, y, line == storage.pick_selected, look.focused, look);
        let mut style = Style::default().fg(look.p.text).bg(bg);
        if site.name == showing {
            style = Style::default().fg(look.accent).bg(bg).add_modifier(Modifier::BOLD);
        }
        put(buf, list.x + 1, y, name_w, &site.name, style);
        if site.current {
            let used = site.name.width() as u16 + 2;
            if name_w > used + 9 {
                put(buf, list.x + 1 + used, y, name_w - used, "this page", Style::default().fg(look.p.green).bg(bg));
            }
        }
        let mut x = list.x + 1 + name_w + 2;
        let cookies = match site.cookies {
            0 => "no cookies".to_string(),
            1 => "1 cookie".to_string(),
            n => format!("{n} cookies"),
        };
        put(buf, x, y, count_w, &format!("{cookies:>12}"), Style::default().fg(look.p.dim).bg(bg));
        x += count_w + 2;
        if kinds_w > 0 {
            put(buf, x, y, kinds_w, &site.kinds.join(", "), Style::default().fg(look.p.dim).bg(bg));
        }
    }
    scroll_marks(buf, list, storage.pick_scroll, sites.len(), look.bg, &look.p);
    cursor
}

fn draw_storage_detail(buf: &mut Buffer, app: &App, look: &Look, area: Rect) {
    let bg = look.bg;
    let storage = &app.dev.storage;
    let Some(item) = app.storage_selected().filter(|_| !storage.picking || app.dev_wide()) else {
        if storage.data.is_some() {
            lines(buf, area, bg, &look.p, &["Nothing selected."]);
        }
        return;
    };
    if area.width < 10 || area.height == 0 {
        return;
    }
    let kind = match item.source {
        ItemRef::Cookie(_) => "Cookie",
        ItemRef::Local(_) => "Local storage",
        ItemRef::Session(_) => "Session storage",
        ItemRef::Store(..) => "IndexedDB",
        ItemRef::Cache(_) => "Cache storage",
    };
    let end = area.x + area.width - 1;
    let kind_w = kind.width() as u16;
    let name_w = area.width.saturating_sub(kind_w + 5);
    put(buf, area.x + 1, area.y, name_w, &item.name, Style::default().fg(look.p.text).bg(bg).add_modifier(Modifier::BOLD));
    if area.width > kind_w + 12 {
        put(buf, end - kind_w, area.y, kind_w, kind, Style::default().fg(look.p.dim).bg(bg));
    }
    draw_rows(buf, app, look);
}
