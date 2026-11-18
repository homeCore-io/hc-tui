//! The frame: a header line, the sidebar, the screen, the search bar
//! when in use, the status line and the footer of keys.

use crate::app::App;
use crate::screens::{self, ScreenId};
use crate::text::{char_len, truncate};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

const SIDEBAR: u16 = 20;

pub fn draw_main(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let hints = footer_lines(app, area.width as usize);
    let sync = usize::from(!app.sync_errors.is_empty());
    let search = usize::from(search_visible(app));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(search as u16),
            Constraint::Length(1 + sync as u16),
            Constraint::Length(hints.len() as u16),
        ])
        .split(area);
    draw_header(frame, app, rows[0]);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(SIDEBAR), Constraint::Min(10)])
        .split(rows[1]);
    draw_sidebar(frame, app, cols[0]);
    screens::screen(app.screen).draw(frame, app, cols[1]);
    if search > 0 {
        draw_search(frame, app, rows[2]);
    }
    draw_status(frame, app, rows[3]);
    let lines: Vec<Line> = hints
        .into_iter()
        .map(|l| Line::from(Span::styled(l, Style::default().fg(Color::Gray))))
        .collect();
    frame.render_widget(Paragraph::new(lines), rows[4]);
}

// ── Header ──────────────────────────────────────────────────────────────────

/// `homeCore v0.1.116 · Day · 21°F out`: the version, the modes that are
/// on, and the outside temperature when a weather device reports one.
pub fn header_left(app: &App) -> String {
    let mut parts = vec![
        match app.system_status.as_ref().map(|s| s.version.as_str()) {
            Some(v) if !v.is_empty() => format!("homeCore v{v}"),
            _ => "homeCore".to_string(),
        },
    ];
    let on: Vec<&str> = app
        .modes
        .iter()
        .filter(|m| app.mode_on(&m.config.id))
        .map(|m| m.config.name.as_str())
        .collect();
    if !on.is_empty() {
        parts.push(on.join(", "));
    }
    if let Some(t) = app
        .devices
        .iter()
        .find(|d| d.device_type() == "weather")
        .and_then(crate::app::devices::device_temperature)
    {
        parts.push(format!(
            "{:.0}{} out",
            app.temp_unit.convert_celsius(t),
            app.temp_unit.symbol()
        ));
    }
    parts.join(" · ")
}

/// `household member · live ●`.
pub fn header_right(app: &App) -> String {
    let role = app
        .current_user
        .as_ref()
        .map(|u| u.role.label().to_lowercase())
        .unwrap_or_default();
    let live = if app.ws_connected {
        "live ●"
    } else {
        "offline ○"
    };
    format!("{role} · {live}")
}

fn draw_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let right = header_right(app);
    let width = area.width as usize;
    let rlen = char_len(&right);
    let left = truncate(&header_left(app), width.saturating_sub(rlen + 2));
    let gap = width.saturating_sub(char_len(&left) + rlen);
    let live = if app.ws_connected {
        Color::Green
    } else {
        Color::Red
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(left, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" ".repeat(gap)),
            Span::styled(right, Style::default().fg(live)),
        ])),
        area,
    );
}

// ── Sidebar ─────────────────────────────────────────────────────────────────

/// The sidebar's lines: `(text, screen)`; `None` for the divider.
pub fn sidebar_rows(app: &App) -> Vec<(String, Option<ScreenId>)> {
    let mut out = Vec::new();
    let mut divided = false;
    for id in app.screens() {
        if id.is_settings() && !divided {
            out.push((format!("{:─^w$}", " Settings ", w = 18), None));
            divided = true;
        }
        let num = id
            .hotkey()
            .map(|c| format!("{c} "))
            .unwrap_or_else(|| "  ".into());
        let badge = match screens::screen(id).badge(app) {
            Some(n) if n > 0 => format!(" ({n})"),
            _ => String::new(),
        };
        out.push((format!("{num}{}{badge}", id.title()), Some(id)));
    }
    out
}

fn draw_sidebar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let w = area.width.saturating_sub(2) as usize;
    let lines: Vec<Line> = sidebar_rows(app)
        .into_iter()
        .map(|(text, id)| match id {
            None => Line::from(Span::styled(
                truncate(&text, w),
                Style::default().fg(Color::DarkGray),
            )),
            Some(id) if id == app.screen => Line::from(Span::styled(
                format!("{:<w$}", truncate(&text, w)),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Some(_) => Line::from(Span::styled(
                truncate(&text, w),
                Style::default().fg(Color::Gray),
            )),
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

// ── Search, status and footer ───────────────────────────────────────────────

fn search_visible(app: &App) -> bool {
    app.search.open || !app.query(app.screen).is_empty()
}

fn draw_search(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let q = app.query(app.screen);
    let (text, style) = if app.search.open {
        (format!(" /{q}_"), Style::default().fg(Color::Yellow))
    } else {
        (
            format!(" /{q}   (Esc clears)"),
            Style::default().fg(Color::DarkGray),
        )
    };
    frame.render_widget(Paragraph::new(text).style(style), area);
}

fn draw_status(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let width = area.width as usize;
    let busy = if app.busy() {
        format!("{} ", app.spinner())
    } else {
        String::new()
    };
    let (text, style) = match &app.error {
        Some(err) => (format!("{busy}{err}"), Style::default().fg(Color::Red)),
        None => (
            format!("{busy}{}", app.status),
            Style::default().fg(Color::DarkGray),
        ),
    };
    let mut lines = vec![Line::from(Span::styled(
        format!(" {}", truncate(&text, width.saturating_sub(1))),
        style,
    ))];
    if let Some(line) = sync_error_line(app, width.saturating_sub(1)) {
        lines.push(Line::from(Span::styled(
            format!(" {line}"),
            Style::default().fg(Color::Red),
        )));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// `"sync failed: rules (500: ...), users (403: ...)"` when the last sync
/// had failures.
fn sync_error_line(app: &App, width: usize) -> Option<String> {
    if app.sync_errors.is_empty() {
        return None;
    }
    let parts: Vec<String> = app
        .sync_errors
        .iter()
        .map(|(section, err)| format!("{section} ({})", truncate(err, 60)))
        .collect();
    Some(truncate(
        &format!("sync failed: {}", parts.join(", ")),
        width,
    ))
}

/// The keys of an open dialog or input. `None` when none is open and
/// the screen's own table applies.
pub fn modal_hints(app: &App) -> Option<Vec<&'static str>> {
    if app.confirm.is_some() {
        return Some(vec!["y yes", "n or Esc no"]);
    }
    if app.help_open {
        return Some(vec!["Esc or ? close"]);
    }
    if let Some(m) = &app.manage.modal {
        return Some(super::settings::modal_hints(m));
    }
    if let Some(s) = &app.plugins_tab.streaming {
        if s.stage.is_terminal() {
            return Some(vec!["Esc close"]);
        }
        if s.pending_prompt.is_some() {
            return Some(vec![
                "type a reply",
                "Enter send",
                "Ctrl+X stop the run",
                "Esc close",
            ]);
        }
        return Some(vec!["c stop the run", "Esc close"]);
    }
    if let Some(h) = super::plugins::modal_hints(app) {
        return Some(h);
    }
    if let Some(h) = super::scenes::modal_hints(app) {
        return Some(h);
    }
    if app.devices_tab.editor.is_some() {
        return Some(vec![
            "Tab field",
            "←/→ room or Security",
            "Enter save",
            "Esc cancel",
        ]);
    }
    if app.rooms.editor.is_some() {
        return Some(vec!["Enter save", "Esc cancel"]);
    }
    if app.manage.user_editor.is_some() {
        return Some(vec![
            "Tab field",
            "Space next role",
            "Enter save",
            "Esc cancel",
        ]);
    }
    if let Some(h) = super::helpers::modal_hints(app).or_else(|| super::rules::modal_hints(app)) {
        return Some(h);
    }
    if app.manage.mode_editor.is_some() {
        return Some(vec!["Tab field", "←/→ change", "Enter save", "Esc cancel"]);
    }
    if app.manage.matter.editor.is_some() {
        return Some(vec!["Tab field", "Enter commission", "Esc cancel"]);
    }
    if app.home.picker.is_some() {
        return Some(vec![
            "type to search",
            "Tab kind",
            "↑/↓ move",
            "←/→ heading",
            "Enter add",
            "Esc done",
        ]);
    }
    if app.home.heading.is_some() {
        return Some(vec![
            "type a name",
            "Tab rename or new",
            "Enter save",
            "Esc cancel",
        ]);
    }
    if app.search.open {
        return Some(vec!["type to search", "Enter keep", "Esc clear"]);
    }
    if app.rules_tab.filter_bar.is_some() {
        return Some(vec![
            "type filter",
            "Tab field",
            "Enter apply",
            "Esc cancel",
        ]);
    }
    None
}

/// The footer, wrapped to `width`.
pub fn footer_lines(app: &App, width: usize) -> Vec<String> {
    let hints: Vec<String> = match modal_hints(app) {
        Some(h) => h.into_iter().map(str::to_string).collect(),
        None => screens::footer_hints(app),
    };
    wrap_hints(&hints, width)
}

/// Join hints with two spaces, wrapping by characters.
pub fn wrap_hints(hints: &[String], width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::from(" ");
    for h in hints {
        let add = if char_len(&cur) > 1 { 2 } else { 0 } + char_len(h);
        if char_len(&cur) + add > width && char_len(&cur) > 1 {
            lines.push(std::mem::replace(&mut cur, String::from(" ")));
        }
        if char_len(&cur) > 1 {
            cur.push_str("  ");
        }
        cur.push_str(&truncate(h, width.saturating_sub(1)));
    }
    lines.push(cur);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::model::Role;

    #[test]
    fn hints_wrap_by_characters() {
        let h: Vec<String> = ["←/→ panel", "a", "b"].map(String::from).to_vec();
        let lines = wrap_hints(&h, 12);
        assert!(lines.iter().all(|l| char_len(l) <= 12), "{lines:?}");
        assert_eq!(wrap_hints(&[], 10), vec![" ".to_string()]);
    }

    #[test]
    fn the_sidebar_has_numbers_a_divider_and_the_alert_badge() {
        let mut a = signed_in(Role::Admin);
        a.alerts = vec![serde_json::from_value(serde_json::json!({"id": "1"})).unwrap()];
        let rows: Vec<String> = sidebar_rows(&a).into_iter().map(|(t, _)| t).collect();
        assert_eq!(rows[0], "1 Home");
        assert!(rows.contains(&"8 Alerts (1)".to_string()), "{rows:?}");
        let div = rows.iter().position(|r| r.contains("Settings")).unwrap();
        assert_eq!(rows[div + 1], "  People");
    }

    #[test]
    fn the_header_names_version_modes_and_outside() {
        let mut a = signed_in(Role::HouseholdMember);
        a.system_status = Some(crate::model::SystemStatus {
            version: "0.1.116".into(),
            ..Default::default()
        });
        let mut w = device("w", "Weather");
        w.device_type = Some("weather".into());
        w.attributes = serde_json::json!({"temperature": -6.0})
            .as_object()
            .cloned()
            .unwrap();
        let mut day = device("mode_day", "Day");
        day.attributes = serde_json::json!({"on": true})
            .as_object()
            .cloned()
            .unwrap();
        a.devices = vec![w, day];
        a.modes = vec![
            serde_json::from_value(
                serde_json::json!({"config": {"id": "mode_day", "name": "Day", "kind": "solar"}}),
            )
            .unwrap(),
        ];
        a.temp_unit = crate::model::TempUnit::Fahrenheit;
        assert_eq!(header_left(&a), "homeCore v0.1.116 · Day · 21°F out");
        assert_eq!(header_right(&a), "household member · offline ○");
    }
}
