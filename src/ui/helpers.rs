//! Helpers: the table (Name, Type, State, Actions), the editor for a new
//! or changed helper, and the box for typing a value.

use super::widgets::{centered_rect_min, heading, highlight};
use crate::app::App;
use crate::app::helpers_tab::{
    FieldKind, HELPER_TYPES, helper_actions_hint, helper_state_line, type_label, type_what,
};
use crate::text::take_chars;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap},
};

pub fn draw_helpers(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let selected = app.helpers_sel.id().map(str::to_string);
    let mut sel_row = None;
    let rows: Vec<Row> = app
        .visible_helpers()
        .enumerate()
        .map(|(i, d)| {
            let is_sel = selected.as_deref() == Some(d.device_id.as_str());
            if is_sel {
                sel_row = Some(i);
            }
            let (state, on) = helper_state_line(d);
            let color = match on {
                Some(true) => Color::Green,
                Some(false) => Color::Gray,
                None => Color::White,
            };
            let marker = if is_sel { "▸ " } else { "  " };
            Row::new(vec![
                Cell::from(format!("{marker}{}", d.display_name())),
                Cell::from(d.device_type().to_string()).style(Style::default().fg(Color::DarkGray)),
                Cell::from(state).style(Style::default().fg(color)),
                Cell::from(helper_actions_hint(d.device_type()))
                    .style(Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();
    let n = rows.len();
    let title = format!(" Helpers ({n}) ");
    if n == 0 {
        let text = if app.devices.is_empty() {
            "Loading..."
        } else if app.can("devices:write") {
            "No helpers yet. n makes one."
        } else {
            "No helpers yet."
        };
        frame.render_widget(
            Paragraph::new(text)
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL).title(title)),
            area,
        );
        return;
    }
    let header = Row::new(
        ["  Name", "Type", "State", "Actions"]
            .iter()
            .map(|h| Cell::from(*h).style(heading())),
    );
    let table = Table::new(
        rows,
        [
            Constraint::Min(22),
            Constraint::Length(10),
            Constraint::Length(22),
            Constraint::Length(34),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(title))
    .row_highlight_style(highlight());
    let mut state = TableState::default();
    state.select(sel_row);
    frame.render_stateful_widget(table, area, &mut state);
}

/// The keys of an open helper editor or value box.
pub fn modal_hints(app: &App) -> Option<Vec<&'static str>> {
    if app.helpers_tab.set_value.is_some() {
        return Some(vec!["type a value", "Enter set", "Esc cancel"]);
    }
    let ed = app.helpers_tab.editor.as_ref()?;
    if ed.helper_type.is_none() {
        return Some(vec!["j/k kind", "Enter choose", "Esc cancel"]);
    }
    Some(vec![
        "Tab field",
        "Space or ←→ change a choice",
        if ed.target.is_some() {
            "Enter save"
        } else {
            "Enter make it"
        },
        "Esc cancel",
    ])
}

pub fn draw_helper_overlays(frame: &mut Frame<'_>, app: &App) {
    draw_editor(frame, app);
    draw_set_value(frame, app);
}

fn draw_set_value(frame: &mut Frame<'_>, app: &App) {
    let Some(sv) = &app.helpers_tab.set_value else {
        return;
    };
    let area = centered_rect_min(30, 60, 5, frame.area());
    frame.render_widget(Clear, area);
    let hint = match sv.helper_type.as_str() {
        "number" => "a number",
        "datetime" => "like 2026-10-17T07:00; empty clears it",
        _ => "words; empty clears it",
    };
    let lines = vec![
        Line::from(vec![
            Span::raw(sv.text.clone()),
            Span::styled("_", Style::default().fg(Color::Cyan)),
        ]),
        Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray))),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Set {} to ", sv.name))
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        area,
    );
}

fn draw_editor(frame: &mut Frame<'_>, app: &App) {
    let Some(ed) = &app.helpers_tab.editor else {
        return;
    };
    let dim = Style::default().fg(Color::DarkGray);
    let Some(t) = ed.helper_type else {
        // The kind first.
        let h = (HELPER_TYPES.len() as u16 + 4).min(frame.area().height);
        let area = centered_rect_min(40, 90, h, frame.area());
        frame.render_widget(Clear, area);
        let lines: Vec<Line> = HELPER_TYPES
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let sel = i == ed.type_sel;
                let name = format!("{} {:<12}", if sel { "▸" } else { " " }, type_label(t));
                Line::from(vec![
                    Span::styled(name, if sel { highlight() } else { Style::default() }),
                    Span::raw(" "),
                    Span::styled(type_what(t), dim),
                ])
            })
            .collect();
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" New helper: what should it be? ")
                    .border_style(Style::default().fg(Color::Cyan)),
            ),
            area,
        );
        return;
    };
    let title = match &ed.target {
        Some(_) => format!(" {} ", ed.initial_name),
        None => format!(" New {} ", type_label(t).to_lowercase()),
    };
    let label = |focus: bool, s: &str| {
        Span::styled(
            format!("{:<22}", take_chars(s, 22)),
            if focus {
                highlight()
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            },
        )
    };
    let cursor = |focus: bool| {
        Span::styled(
            if focus { "_" } else { "" },
            Style::default().fg(Color::Cyan),
        )
    };
    let mut lines = Vec::new();
    if ed.target.is_none() {
        lines.push(Line::from(Span::styled(type_what(t), dim)));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(vec![
        label(ed.focus == 0, "Name"),
        Span::raw(" "),
        Span::raw(ed.name.clone()),
        cursor(ed.focus == 0),
    ]));
    if ed.target.is_none() {
        let id = crate::app::helpers_tab::slug_of(&ed.name);
        let shown = if id.is_empty() {
            "...".to_string()
        } else {
            format!("{t}_{id}")
        };
        lines.push(Line::from(Span::styled(
            format!("{:<23}Rules will call it {shown}", ""),
            dim,
        )));
    }
    for (i, f) in ed.fields.iter().enumerate() {
        let focus = ed.focus == i + 1;
        let value = match f.kind {
            FieldKind::Toggle | FieldKind::Choice(_) => format!("‹ {} ›", f.value),
            _ => f.value.clone(),
        };
        let mut spans = vec![label(focus, f.label), Span::raw(" "), Span::raw(value)];
        if !matches!(f.kind, FieldKind::Toggle | FieldKind::Choice(_)) {
            spans.push(cursor(focus));
        }
        lines.push(Line::from(spans));
        if !f.help.is_empty() && focus {
            lines.push(Line::from(Span::styled(
                format!("{:<23}{}", "", f.help),
                dim,
            )));
        }
    }
    if ed.fields.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Nothing else to set for this kind.",
            dim,
        )));
    }
    if let Some(e) = &ed.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            e.clone(),
            Style::default().fg(Color::Red),
        )));
    }
    let h = (lines.len() as u16 + 2).min(frame.area().height);
    let area = centered_rect_min(40, 84, h, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        area,
    );
}
