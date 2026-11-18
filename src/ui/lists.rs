//! Helpers and Alerts: plain lists until their own passes.

use super::widgets::{heading, highlight};
use crate::app::App;
use crate::app::helpers::helper_action_hint;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Row, Table, TableState},
};

pub fn draw_helpers(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let selected = app.helpers_sel.id().map(str::to_string);
    let mut sel_row = None;
    let rows: Vec<Row> = app
        .visible_helpers()
        .enumerate()
        .map(|(i, d)| {
            if selected.as_deref() == Some(d.device_id.as_str()) {
                sel_row = Some(i);
            }
            let (state, on) = app.helper_state(d);
            let color = match on {
                Some(true) => Color::Green,
                Some(false) => Color::Gray,
                None => Color::White,
            };
            Row::new(vec![
                Cell::from(format!("  {}", d.display_name())),
                Cell::from(d.device_type().to_string()).style(Style::default().fg(Color::DarkGray)),
                Cell::from(state).style(Style::default().fg(color)),
                Cell::from(helper_action_hint(d.device_type()))
                    .style(Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();
    let header = Row::new(
        ["  Name", "Type", "State", "Action"]
            .iter()
            .map(|h| Cell::from(*h).style(heading())),
    );
    let n = rows.len();
    let table = Table::new(
        rows,
        [
            Constraint::Min(22),
            Constraint::Length(10),
            Constraint::Length(22),
            Constraint::Length(22),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Helpers ({n}) ")),
    )
    .row_highlight_style(highlight());
    let mut state = TableState::default();
    state.select(sel_row);
    frame.render_stateful_widget(table, area, &mut state);
}

/// `12m`, `3h`, `2d`.
pub fn age(ts: &str) -> String {
    let Ok(t) = chrono::DateTime::parse_from_rfc3339(ts) else {
        return String::new();
    };
    let secs = (chrono::Utc::now() - t.with_timezone(&chrono::Utc))
        .num_seconds()
        .max(0);
    match secs {
        s if s < 60 => format!("{s}s ago"),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86400),
    }
}

pub fn draw_alerts(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let selected = app.alerts_sel.id().map(str::to_string);
    let mut sel_row = None;
    let rows: Vec<Row> = app
        .visible_alerts()
        .enumerate()
        .map(|(i, a)| {
            if selected.as_deref() == Some(a.id.as_str()) {
                sel_row = Some(i);
            }
            let (dot, color) = if a.is_open() {
                let c = match a.severity.as_str() {
                    "critical" | "error" => Color::Red,
                    _ => Color::Yellow,
                };
                ("●", c)
            } else {
                ("○", Color::DarkGray)
            };
            let device = app
                .device_by_id(&a.device_id)
                .map(|d| d.display_name().to_string())
                .unwrap_or_default();
            Row::new(vec![
                Cell::from(format!(" {dot}")).style(Style::default().fg(color)),
                Cell::from(a.message.clone()),
                Cell::from(device).style(Style::default().fg(Color::Gray)),
                Cell::from(age(&a.raised_at)).style(Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();
    let title = format!(
        " Alerts  Open ({}) · {} in all ",
        app.open_alerts(),
        app.alerts.len()
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Min(30),
            Constraint::Length(20),
            Constraint::Length(9),
        ],
    )
    .block(Block::default().borders(Borders::ALL).title(title))
    .row_highlight_style(highlight());
    let mut state = TableState::default();
    state.select(sel_row);
    frame.render_stateful_widget(table, area, &mut state);
}
