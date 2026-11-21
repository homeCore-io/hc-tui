//! Alerts: the sub-tabs, the lists and the selected alert's detail.

use super::lists::age;
use super::widgets::{heading, highlight};
use crate::app::alerts::{AlertsSub, action_label, describe_rule, kind_words};
use crate::app::{App, Load};
use crate::model::Alert;
use crate::text::take_chars;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap},
};

/// `Alerts  Open (2) · Acknowledged · Rules · Ignored`, the chosen one
/// bold.
fn title(app: &App) -> Line<'static> {
    let mut spans = vec![Span::raw(" Alerts  ")];
    for (i, s) in AlertsSub::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(Color::DarkGray)));
        }
        let mut text = s.title().to_string();
        if *s == AlertsSub::Open {
            text = format!("Open ({})", app.open_alerts());
        }
        let style = if *s == app.alerts_tab.sub {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::Gray)
        };
        spans.push(Span::styled(text, style));
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

pub fn draw_alerts(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(title(app));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    match app.alerts_tab.sub {
        AlertsSub::Open | AlertsSub::Acknowledged => draw_alert_list(frame, app, inner),
        AlertsSub::Rules => draw_rules(frame, app, inner),
        AlertsSub::Ignored => draw_ignores(frame, app, inner),
        AlertsSub::Kinds => draw_kinds(frame, app, inner),
    }
}

fn severity_color(a: &Alert) -> Color {
    if !a.is_open() {
        return Color::DarkGray;
    }
    match a.severity.as_str() {
        "critical" | "error" => Color::Red,
        "info" => Color::Cyan,
        _ => Color::Yellow,
    }
}

fn empty(frame: &mut Frame<'_>, area: Rect, text: &str) {
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn draw_alert_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let open = app.alerts_tab.sub == AlertsSub::Open;
    let selected = app.alerts_sel.id().map(str::to_string);
    let mut sel_row = None;
    let rows: Vec<Row> = app
        .visible_alerts()
        .enumerate()
        .map(|(i, a)| {
            let is_sel = selected.as_deref() == Some(a.id.as_str());
            if is_sel {
                sel_row = Some(i);
            }
            let dot = if a.is_open() { "●" } else { "○" };
            let device = app
                .device_by_id(&a.device_id)
                .map(|d| d.display_name().to_string())
                .unwrap_or_else(|| {
                    // A plugin's alert about itself names the plugin.
                    if a.device_id.starts_with("plugin.") {
                        super::widgets::clean_plugin_id(&a.device_id)
                    } else {
                        a.device_id.clone()
                    }
                });
            Row::new(vec![
                Cell::from(format!("{} {dot}", if is_sel { "▸" } else { " " }))
                    .style(Style::default().fg(severity_color(a))),
                Cell::from(a.message.clone()),
                Cell::from(device).style(Style::default().fg(Color::Gray)),
                Cell::from(age(&a.raised_at)).style(Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();
    if rows.is_empty() {
        empty(
            frame,
            area,
            if open {
                "Nothing needs looking at."
            } else {
                "No acknowledged alerts."
            },
        );
        return;
    }
    let detail_h = if area.height >= 14 { 6 } else { 0 };
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(2), Constraint::Length(detail_h)])
        .split(area);
    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Min(30),
            Constraint::Length(20),
            Constraint::Length(9),
        ],
    )
    .row_highlight_style(highlight());
    let mut state = TableState::default();
    state.select(sel_row);
    frame.render_stateful_widget(table, parts[0], &mut state);
    if detail_h > 0
        && let Some(a) = app.selected_alert()
    {
        draw_detail(frame, app, a, parts[1]);
    }
}

/// Where an alert came from, in words.
pub fn source_line(app: &App, a: &Alert) -> String {
    match a.kind.as_str() {
        "rule" => {
            let rule = a.rule_id.as_deref().and_then(|id| app.alert_rule(id));
            match rule {
                Some(r) => {
                    let told = match r.notify.as_deref() {
                        Some("all") => "notified: every channel".to_string(),
                        Some(c) => format!("notified: {c}"),
                        None => "shown, not sent".to_string(),
                    };
                    format!(
                        "From alert rule \"{}\" ({}) · {told}",
                        r.name,
                        describe_rule(r)
                    )
                }
                None => match app.alerts_tab.rules {
                    Load::Ready(_) => "From an alert rule since deleted".into(),
                    _ => "From an alert rule".into(),
                },
            }
        }
        "plugin" => {
            let p = a
                .plugin_id
                .as_deref()
                .unwrap_or("a plugin")
                .trim_start_matches("plugin.")
                .to_string();
            match a.topic.as_deref() {
                Some(t) if !t.is_empty() => format!("From {p}: {}", t.replace('_', " ")),
                _ => format!("From {p}"),
            }
        }
        k => format!("homeCore's own: {}", kind_words(k)),
    }
}

fn draw_detail(frame: &mut Frame<'_>, app: &App, a: &Alert, area: Rect) {
    let dim = Style::default().fg(Color::DarkGray);
    let mut lines = vec![
        Line::from(Span::styled(
            a.message.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(format!("  {}", source_line(app, a)), dim)),
    ];
    if let Some(ack) = &a.acknowledged {
        let by = ack.get("by").and_then(|v| v.as_str()).unwrap_or("someone");
        let at = ack
            .get("at")
            .and_then(|v| v.as_str())
            .map(age)
            .unwrap_or_default();
        lines.push(Line::from(Span::styled(
            format!("  Acknowledged by {by} {at}"),
            dim,
        )));
    }
    let mut keys: Vec<Span> = vec![Span::styled("  Actions: ", dim)];
    if a.is_open() && app.can("devices:command") {
        keys.push(Span::raw(" a acknowledge  "));
    }
    if let Some(label) = action_label(a)
        && app.can("devices:command")
    {
        keys.push(Span::raw(format!(
            " p press \"{}\"  ",
            take_chars(&label, 24)
        )));
    }
    if app.can("automations:write") {
        keys.push(Span::raw(" i ignore this device"));
    }
    if keys.len() > 1 {
        lines.push(Line::from(keys));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::TOP)),
        area,
    );
}

fn load_text<T>(l: &Load<T>) -> Option<String> {
    match l {
        Load::Idle | Load::Loading => Some("Loading...".into()),
        Load::Failed(e) => Some(format!("Not available: {e}")),
        Load::Ready(_) => None,
    }
}

fn draw_rules(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if !app.can("automations:read") {
        return empty(
            frame,
            area,
            "Alert rules are for people who may read automations.",
        );
    }
    if let Some(t) = load_text(&app.alerts_tab.rules) {
        return empty(frame, area, &t);
    }
    let selected = app.alerts_tab.rules_sel.id().map(str::to_string);
    let mut sel_row = None;
    let rows: Vec<Row> = app
        .visible_alert_rules()
        .enumerate()
        .map(|(i, r)| {
            let is_sel = selected.as_deref() == Some(r.id.as_str());
            if is_sel {
                sel_row = Some(i);
            }
            let (dot, c) = if r.enabled {
                ("●", Color::Green)
            } else {
                ("○", Color::DarkGray)
            };
            let devices: Vec<String> = r
                .device_ids
                .iter()
                .map(|d| {
                    app.device_by_id(d)
                        .map(|x| x.display_name().to_string())
                        .unwrap_or_else(|| d.clone())
                })
                .collect();
            Row::new(vec![
                Cell::from(format!("{} {dot}", if is_sel { "▸" } else { " " }))
                    .style(Style::default().fg(c)),
                Cell::from(r.name.clone()),
                Cell::from(describe_rule(r)).style(Style::default().fg(Color::Gray)),
                Cell::from(devices.join(", ")).style(Style::default().fg(Color::DarkGray)),
                Cell::from(r.severity.clone()).style(Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    if rows.is_empty() {
        empty(frame, parts[0], "No alert rules.");
    } else {
        let header = Row::new(
            ["", "Name", "When", "Devices", "How bad"]
                .iter()
                .map(|h| Cell::from(*h).style(heading())),
        );
        let table = Table::new(
            rows,
            [
                Constraint::Length(3),
                Constraint::Min(22),
                Constraint::Min(24),
                Constraint::Length(24),
                Constraint::Length(9),
            ],
        )
        .header(header)
        .row_highlight_style(highlight());
        let mut state = TableState::default();
        state.select(sel_row);
        frame.render_stateful_widget(table, parts[0], &mut state);
    }
    frame.render_widget(
        Paragraph::new(" New alert rules are made in the app: Settings, Alerts.")
            .style(Style::default().fg(Color::DarkGray)),
        parts[1],
    );
}

fn draw_kinds(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if let Some(t) = load_text(&app.alerts_tab.kinds) {
        return empty(frame, area, &t);
    }
    let list = app.alerts_tab.kinds.ready().cloned().unwrap_or_default();
    if list.is_empty() {
        return empty(frame, area, "No plugin declares kinds of alert.");
    }
    let selected = app.alerts_tab.kinds_sel.id().map(str::to_string);
    // The table, and what the selected kind is about beneath it.
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(2)])
        .split(area);
    let about = list
        .iter()
        .find(|k| selected.as_deref() == Some(k.key().as_str()))
        .and_then(|k| k.description.clone())
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(format!(" {about}"))
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: true }),
        parts[1],
    );
    let area = parts[0];
    let mut sel_row = None;
    let rows: Vec<Row> = list
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let is_sel = selected.as_deref() == Some(k.key().as_str());
            if is_sel {
                sel_row = Some(i);
            }
            let state = match (k.enabled, k.enabled == k.default_on) {
                (true, true) => "on",
                (false, true) => "off",
                (true, false) => "on (turned on)",
                (false, false) => "off (turned off)",
            };
            let dim = Style::default().fg(Color::DarkGray);
            Row::new(vec![
                Cell::from(if is_sel { " ▸" } else { "  " }),
                Cell::from(k.plugin_id.trim_start_matches("plugin.").to_string()).style(dim),
                Cell::from(k.label.clone()).style(if k.enabled { Style::default() } else { dim }),
                Cell::from(k.severity.clone()).style(dim),
                Cell::from(state).style(if k.enabled {
                    Style::default().fg(Color::Green)
                } else {
                    dim
                }),
                Cell::from(k.notify.clone().unwrap_or_else(|| "default channel".into())).style(dim),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(10),
            Constraint::Min(28),
            Constraint::Length(8),
            Constraint::Length(17),
            Constraint::Length(16),
        ],
    )
    .row_highlight_style(highlight());
    let mut state = TableState::default();
    state.select(sel_row);
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_ignores(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if !app.can("automations:read") {
        return empty(
            frame,
            area,
            "The ignore list is for people who may read automations.",
        );
    }
    if let Some(t) = load_text(&app.alerts_tab.ignores) {
        return empty(frame, area, &t);
    }
    let list = app.alerts_tab.ignores.ready().cloned().unwrap_or_default();
    if list.is_empty() {
        return empty(
            frame,
            area,
            "Nothing is ignored. i on an alert ignores its device.",
        );
    }
    let selected = app.alerts_tab.ignores_sel.id().map(str::to_string);
    let mut sel_row = None;
    let now = chrono::Utc::now();
    let rows: Vec<Row> = list
        .iter()
        .enumerate()
        .map(|(i, ig)| {
            let is_sel = selected.as_deref() == Some(ig.id.as_str());
            if is_sel {
                sel_row = Some(i);
            }
            let ran_out = ig
                .until
                .as_deref()
                .and_then(|u| chrono::DateTime::parse_from_rfc3339(u).ok())
                .is_some_and(|u| u <= now);
            let who = match (&ig.by, &ig.created_at) {
                (Some(b), Some(c)) => format!("{b}, {}", age(c)),
                (Some(b), None) => b.clone(),
                (None, Some(c)) => age(c),
                _ => String::new(),
            };
            Row::new(vec![
                Cell::from(if is_sel { " ▸" } else { "  " }),
                Cell::from(app.ignore_text(ig)).style(if ran_out {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default()
                }),
                Cell::from(if ran_out { "ran out" } else { "" })
                    .style(Style::default().fg(Color::DarkGray)),
                Cell::from(who).style(Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Min(30),
            Constraint::Length(8),
            Constraint::Length(22),
        ],
    )
    .row_highlight_style(highlight());
    let mut state = TableState::default();
    state.select(sel_row);
    frame.render_stateful_widget(table, area, &mut state);
}
