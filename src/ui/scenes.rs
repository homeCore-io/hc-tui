//! The Scenes list and the scene editor.

use super::widgets::{heading, highlight};
use crate::app::App;
use crate::app::scenes::{SceneField, state_summary};
use crate::model::SceneSource;
use crate::text::take_chars;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
};

/// A plugin's name for the From column: `plugin.lutron` reads Lutron.
pub fn plugin_label(plugin_id: &str) -> String {
    let short = plugin_id
        .trim_start_matches("plugin.")
        .trim_start_matches("hc-");
    let mut c = short.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => "-".into(),
    }
}

pub fn draw_scenes_table(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let hl = highlight();
    let header = Row::new(
        ["  Scene", "From", "Has", "State"]
            .iter()
            .map(|h| Cell::from(*h).style(heading())),
    );
    let selected = app.selected_scene().map(|s| s.id.clone());
    let mut sel_row = None;
    let rows: Vec<Row> = app
        .visible_scenes()
        .enumerate()
        .map(|(i, s)| {
            let is_sel = selected.as_deref() == Some(s.id.as_str());
            if is_sel {
                sel_row = Some(i);
            }
            let (source, has) = match s.source {
                SceneSource::Native => {
                    let n = s.states.len();
                    let word = if n == 1 { "device" } else { "devices" };
                    ("homeCore".to_string(), format!("{n} {word}"))
                }
                SceneSource::Device => {
                    let room = app
                        .device_by_id(&s.id)
                        .and_then(|d| app.room_label(d))
                        .or_else(|| s.room.clone())
                        .unwrap_or_else(|| "-".into());
                    (
                        s.plugin_id
                            .as_deref()
                            .map(plugin_label)
                            .unwrap_or_else(|| "-".into()),
                        room,
                    )
                }
            };
            let (st, color) = match s.active {
                Some(true) => ("● active", Color::Green),
                Some(false) => ("○ off", Color::DarkGray),
                None => ("-", Color::DarkGray),
            };
            let style = |c: Color| if is_sel { hl } else { Style::default().fg(c) };
            Row::new(vec![
                Cell::from(format!("  {}", s.name)).style(style(Color::White)),
                Cell::from(source).style(style(Color::DarkGray)),
                Cell::from(has).style(style(Color::Gray)),
                Cell::from(st).style(style(color)),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Min(26),
            Constraint::Length(12),
            Constraint::Length(20),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Scenes ({}) ", app.views.scenes.len())),
    )
    .row_highlight_style(hl);
    let mut state = TableState::default();
    state.select(sel_row);
    frame.render_stateful_widget(table, area, &mut state);
}

/// The scene editor, and the picker when it is open over it.
pub fn draw_overlays(frame: &mut Frame<'_>, app: &App) {
    draw_scene_editor(frame, app);
    super::controls::draw_picker(frame, app);
}

fn draw_scene_editor(frame: &mut Frame<'_>, app: &App) {
    let Some(ed) = &app.scenes_tab.editor else {
        return;
    };
    let area = frame.area();
    let w = 72.min(area.width);
    let h = 24.min(area.height);
    let r = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };
    frame.render_widget(Clear, r);
    let title = if ed.id.is_some() {
        " Edit scene "
    } else {
        " New scene "
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(r);
    frame.render_widget(block, r);
    if inner.height < 4 {
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(inner);
    frame.render_widget(
        super::widgets::input(&ed.name, "Name", ed.field == SceneField::Name),
        rows[0],
    );
    let focused = ed.field == SceneField::Devices;
    let name_w = (inner.width as usize).saturating_sub(6).min(28);
    let lines: Vec<Line> = if ed.members.is_empty() {
        vec![Line::from(Span::styled(
            "  No devices yet. Tab here, then a adds some as they are now.",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        ed.members
            .iter()
            .enumerate()
            .map(|(i, (id, state))| {
                let name = app
                    .device_by_id(id)
                    .map(|d| d.display_name().to_string())
                    .unwrap_or_else(|| format!("{id} (missing)"));
                let style = if focused && i == ed.sel {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                Line::from(vec![
                    Span::styled(format!("  {:<name_w$}", take_chars(&name, name_w)), style),
                    Span::styled(
                        format!("  {}", state_summary(state, app.temp_unit)),
                        Style::default().fg(Color::Cyan),
                    ),
                ])
            })
            .collect()
    };
    let list_h = rows[1].height.saturating_sub(2) as usize;
    let skip = ed.sel.saturating_sub(list_h.saturating_sub(1));
    let lines: Vec<Line> = lines.into_iter().skip(skip).collect();
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Devices ({})", ed.members.len()))
                .border_style(super::widgets::focus_style(focused)),
        ),
        rows[1],
    );
}

/// Footer hints while one of these modals is open.
pub fn modal_hints(app: &App) -> Option<Vec<&'static str>> {
    if app.devices_tab.controls.picker.is_some() {
        return Some(vec![
            "type to search",
            "↑/↓ move",
            "Space mark",
            "Enter add",
            "Esc cancel",
        ]);
    }
    let ed = app.scenes_tab.editor.as_ref()?;
    Some(match ed.field {
        SceneField::Name => vec!["Tab devices", "Enter save", "Esc cancel"],
        SceneField::Devices => vec![
            "a add devices",
            "x remove",
            "c take how it is now",
            "Tab name",
            "Enter save",
            "Esc cancel",
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_names_read_as_words() {
        assert_eq!(plugin_label("plugin.lutron"), "Lutron");
        assert_eq!(plugin_label("plugin.hc-caseta"), "Caseta");
        assert_eq!(plugin_label(""), "-");
    }
}
