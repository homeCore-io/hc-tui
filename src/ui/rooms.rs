//! Rooms: a table of rooms, and the selected room's devices below.

use super::widgets::{heading, highlight};
use crate::app::{App, Load};
use crate::text::{char_len, truncate};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};

/// `●●○○` for lights on and off, cut to `max` dots.
pub fn light_dots(lights: &[bool], max: usize) -> String {
    let mut s: String = lights
        .iter()
        .take(max)
        .map(|on| if *on { '●' } else { '○' })
        .collect();
    if lights.len() > max {
        s.push('+');
    }
    s
}

pub fn draw_rooms(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let detail_h = if area.height > 16 { 7 } else { 0 };
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(detail_h)])
        .split(area);
    draw_table(frame, app, parts[0]);
    if detail_h > 0 {
        draw_detail(frame, app, parts[1]);
    }
}

fn draw_table(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let hl = highlight();
    let header = Row::new(
        ["  Room", "On", "Lights", "Temp", "Notes"]
            .iter()
            .map(|h| Cell::from(*h).style(heading())),
    );
    let selected = app.rooms.sel.id().map(str::to_string);
    let mut sel_row = None;
    let rows: Vec<Row> = app
        .visible_rooms()
        .enumerate()
        .map(|(i, room)| {
            let s = app.room_summary(room);
            let is_sel = selected.as_deref() == Some(room.id.as_str());
            if is_sel {
                sel_row = Some(i);
            }
            let on = if s.lights_total == 0 {
                "-".to_string()
            } else {
                format!("{}/{}", s.lights_on, s.lights_total)
            };
            let temp = s
                .temp_c
                .map(|t| {
                    format!(
                        "{:.0}{}",
                        app.temp_unit.convert_celsius(t),
                        app.temp_unit.symbol()
                    )
                })
                .unwrap_or_else(|| "-".into());
            let on_color = if s.lights_on > 0 {
                Color::Green
            } else {
                Color::Gray
            };
            Row::new(vec![
                Cell::from(format!("  {}", room.name)),
                Cell::from(on).style(Style::default().fg(on_color)),
                Cell::from(light_dots(&s.lights, 12)).style(Style::default().fg(Color::Yellow)),
                Cell::from(temp),
                Cell::from(s.notes()).style(Style::default().fg(Color::Yellow)),
            ])
        })
        .collect();
    let shown = rows.len();
    let title = if shown == app.areas.len() {
        format!(" Rooms ({shown}) ")
    } else {
        format!(" Rooms ({shown} of {}) ", app.areas.len())
    };
    let table = Table::new(
        rows,
        [
            Constraint::Min(18),
            Constraint::Length(6),
            Constraint::Length(14),
            Constraint::Length(7),
            Constraint::Min(10),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(title))
    .row_highlight_style(hl);
    let mut state = TableState::default();
    state.select(sel_row);
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(room) = app.selected_room() else {
        frame.render_widget(Block::default().borders(Borders::ALL), area);
        return;
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", room.name));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let devices = app.room_devices(room);
    if devices.is_empty() {
        frame.render_widget(
            Paragraph::new("  No devices in this room").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }
    // Cells of about 28 characters, as many across as fit.
    let cell = 28usize;
    let across = (inner.width as usize / cell).max(1);
    let name_w = cell.saturating_sub(12);
    let mut lines = Vec::new();
    for chunk in devices.chunks(across) {
        let mut spans = Vec::new();
        for d in chunk {
            let name = truncate(d.display_name(), name_w);
            let pad = " ".repeat(name_w.saturating_sub(char_len(&name)));
            let st = if !d.available {
                "offline".to_string()
            } else {
                let st = app.device_status(d).to_lowercase();
                match crate::app::devices::device_brightness(d) {
                    Some(b) if st == "on" && b < 100 => format!("on {b}%"),
                    _ => st,
                }
            };
            spans.push(Span::raw(format!("  {name}{pad} ")));
            spans.push(Span::styled(
                format!("{:<9}", truncate(&st, 9)),
                Style::default().fg(Color::Gray),
            ));
        }
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The box over Rooms: what Room on brings back, or where to merge a room.
pub fn draw_room_overlay(frame: &mut Frame<'_>, app: &App) {
    use crate::app::rooms::RoomOverlay;
    use ratatui::widgets::{Clear, Wrap};
    let Some(overlay) = &app.rooms.overlay else {
        return;
    };
    let popup = super::widgets::centered_rect(60, 60, frame.area());
    frame.render_widget(Clear, popup);
    let (title, lines, keys) = match overlay {
        RoomOverlay::Saved { name, lights } => {
            let lines: Vec<Line> = match lights {
                Load::Ready(None) => vec![Line::from(Span::styled(
                    " Room off hasn't saved anything here. Room on turns every light on at full brightness.",
                    Style::default().fg(Color::DarkGray),
                ))],
                Load::Ready(Some(snap)) => saved_lines(app, snap),
                Load::Failed(e) => vec![Line::from(format!(" Couldn't read it: {e}"))],
                _ => vec![Line::from(" loading...")],
            };
            (
                format!(" Room on brings back · {name} "),
                lines,
                " Esc close",
            )
        }
        RoomOverlay::Merge {
            from_name,
            targets,
            sel,
            ..
        } => {
            let lines = targets
                .iter()
                .enumerate()
                .map(|(i, (_, n))| {
                    let style = if i == *sel {
                        Style::default().fg(Color::Black).bg(Color::Cyan)
                    } else {
                        Style::default()
                    };
                    Line::from(Span::styled(format!("  {n}  "), style))
                })
                .collect();
            (
                format!(" Merge {from_name} into "),
                lines,
                " j/k choose   Enter merge (asks first)   Esc cancel",
            )
        }
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), parts[0]);
    frame.render_widget(
        Paragraph::new(keys).style(Style::default().fg(Color::DarkGray)),
        parts[1],
    );
}

/// Each saved light by name: on with its settings, or off; then when.
fn saved_lines(app: &App, snap: &serde_json::Value) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    if let Some(scenes) = snap.get("scenes").and_then(serde_json::Value::as_array)
        && !scenes.is_empty()
    {
        let names: Vec<String> = scenes
            .iter()
            .filter_map(|s| s.as_str())
            .map(|id| {
                app.device_by_id(id)
                    .map(|d| d.display_name().to_string())
                    .unwrap_or_else(|| id.to_string())
            })
            .collect();
        out.push(Line::from(format!(" Scenes first: {}", names.join(", "))));
        out.push(Line::from(""));
    }
    for l in snap
        .get("lights")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        let id = l
            .get("device_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let name = app
            .device_by_id(id)
            .map(|d| d.display_name().to_string())
            .unwrap_or_else(|| id.to_string());
        let on = l
            .get("on")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let how = if on {
            let s = l.get("settings").and_then(serde_json::Value::as_object);
            match s
                .and_then(|s| s.get("brightness_pct"))
                .and_then(serde_json::Value::as_f64)
            {
                Some(b) => format!("on, {}%", b.round()),
                None => "on".to_string(),
            }
        } else {
            "stays off".to_string()
        };
        out.push(Line::from(vec![
            Span::raw(format!("  {name:<32} ")),
            Span::styled(
                how,
                Style::default().fg(if on { Color::Green } else { Color::DarkGray }),
            ),
        ]));
    }
    if let Some(at) = snap.get("saved_at").and_then(serde_json::Value::as_str) {
        out.push(Line::from(""));
        out.push(Line::from(Span::styled(
            format!(" Saved by Room off {}", super::widgets::short_ts(at)),
            Style::default().fg(Color::DarkGray),
        )));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_show_each_light() {
        assert_eq!(light_dots(&[true, false, true], 5), "●○●");
        assert_eq!(light_dots(&[false; 4], 2), "○○+");
    }
}
