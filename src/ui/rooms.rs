//! Rooms: a table of rooms, and the selected room's devices below.

use super::widgets::{heading, highlight};
use crate::app::App;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_show_each_light() {
        assert_eq!(light_dots(&[true, false, true], 5), "●○●");
        assert_eq!(light_dots(&[false; 4], 2), "○○+");
    }
}
