//! Home, its `a` picker and the heading editor.

use super::widgets::input;
use crate::app::App;
use crate::app::home::{HomeItem, Picker};
use crate::text::{char_len, truncate};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

/// At least this wide for two columns.
const TWO_COLUMNS: u16 = 90;

/// Headings split over `n` columns, each going to the shorter one so
/// far. Returns the heading indexes of each column.
pub fn columns(sizes: &[usize], n: usize) -> Vec<Vec<usize>> {
    let n = n.max(1);
    let mut cols: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut height = vec![0usize; n];
    for (g, size) in sizes.iter().enumerate() {
        let c = (0..n).min_by_key(|&c| height[c]).unwrap_or(0);
        cols[c].push(g);
        // The heading, its items and a blank line after.
        height[c] += size + 2;
    }
    cols
}

pub fn draw_home(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(" Home ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let layout = &app.home.layout;
    if layout.is_empty() {
        let hint = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Home is empty.",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  Press a to add rooms, devices, scenes, modes, helpers and"),
            Line::from("  automations, or * on any list to pin what is selected."),
        ];
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(Color::Gray)),
            inner,
        );
        return;
    }
    let n = if inner.width >= TWO_COLUMNS && layout.groups.len() > 1 {
        2
    } else {
        1
    };
    let sizes: Vec<usize> = layout.groups.iter().map(|g| g.items.len()).collect();
    let cols = columns(&sizes, n);
    let areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, n as u32); n])
        .split(inner);
    let selected = app.home.sel.id().map(str::to_string);
    for (c, groups) in cols.iter().enumerate() {
        let area = areas[c];
        let w = area.width as usize;
        let mut lines: Vec<Line> = Vec::new();
        let mut sel_line = None;
        for &g in groups {
            let grp = &layout.groups[g];
            lines.push(Line::from(Span::styled(
                format!(" {}", truncate(&grp.name, w.saturating_sub(1))),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            for item in &grp.items {
                let is_sel = selected.as_deref() == Some(item.key().as_str());
                if is_sel {
                    sel_line = Some(lines.len());
                }
                lines.push(item_line(app, item, is_sel, w));
            }
            lines.push(Line::from(""));
        }
        // Keep the selection in view.
        let h = area.height as usize;
        let scroll = match sel_line {
            Some(l) if l + 1 > h => (l + 1 - h) as u16,
            _ => 0,
        };
        frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), area);
    }
}

fn item_line(app: &App, item: &HomeItem, selected: bool, width: usize) -> Line<'static> {
    let marker = if selected { " ▸ " } else { "   " };
    let (name, kind, state, color) = match app.home_entry(item) {
        Some(e) => {
            let color = match e.on {
                Some(true) => Color::Green,
                Some(false) => Color::Gray,
                None => Color::White,
            };
            (e.name, e.kind_label, e.state, color)
        }
        None => (
            item.id.clone(),
            item.kind.as_str().to_string(),
            "missing".to_string(),
            Color::Red,
        ),
    };
    // name | kind | state, shrinking the name first.
    let kind_w = 12.min(width / 4);
    let state_w = 18.min(width / 3);
    let name_w = width.saturating_sub(3 + kind_w + state_w + 2);
    let name_style = if selected {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().fg(Color::White)
    };
    Line::from(vec![
        Span::styled(marker, Style::default().fg(Color::Cyan)),
        Span::styled(pad(&name, name_w), name_style),
        Span::raw(" "),
        Span::styled(pad(&kind, kind_w), Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::styled(truncate(&state, state_w), Style::default().fg(color)),
    ])
}

fn pad(s: &str, w: usize) -> String {
    let t = truncate(s, w);
    let n = w.saturating_sub(char_len(&t));
    format!("{t}{}", " ".repeat(n))
}

fn popup(frame: &mut Frame<'_>, w: u16, h: u16, title: &str) -> Rect {
    let area = frame.area();
    let w = w.min(area.width);
    let h = h.min(area.height);
    let r = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };
    frame.render_widget(Clear, r);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(r);
    frame.render_widget(block, r);
    inner
}

pub fn draw_picker(frame: &mut Frame<'_>, app: &App) {
    let Some(p) = &app.home.picker else {
        return;
    };
    let inner = popup(frame, 64, 20, "Add to Home");
    if inner.height < 4 {
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let w = inner.width as usize;
    frame.render_widget(
        Paragraph::new(format!("/{}_", p.query)).style(Style::default().fg(Color::Yellow)),
        rows[0],
    );
    frame.render_widget(Paragraph::new(kind_tabs(p)), rows[1]);

    let list = app.picker_rows();
    let h = rows[2].height as usize;
    let sel = p.sel.index().min(list.len().saturating_sub(1));
    let start = sel.saturating_sub(h.saturating_sub(1));
    let lines: Vec<Line> = list
        .iter()
        .enumerate()
        .skip(start)
        .take(h)
        .map(|(i, c)| {
            let on = app.home.layout.contains(&c.item);
            let mark = if i == sel { "▸ " } else { "  " };
            let style = if i == sel {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            let tail = if on { "on Home" } else { c.context.as_str() };
            let name_w = w.saturating_sub(2 + 12 + 14);
            Line::from(vec![
                Span::raw(mark),
                Span::styled(pad(&c.name, name_w), style),
                Span::styled(
                    format!(" {}", pad(&c.kind_label, 11)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!(" {}", truncate(tail, 13)),
                    Style::default().fg(if on { Color::Green } else { Color::Gray }),
                ),
            ])
        })
        .collect();
    let body = if lines.is_empty() {
        Paragraph::new("  Nothing matches").style(Style::default().fg(Color::DarkGray))
    } else {
        Paragraph::new(lines)
    };
    frame.render_widget(body, rows[2]);

    let headings = app.picker_headings();
    let heading = headings
        .get(p.group.min(headings.len().saturating_sub(1)))
        .cloned()
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Under heading: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("‹ {} ›", truncate(&heading, w.saturating_sub(20))),
                Style::default().fg(Color::Cyan),
            ),
        ])),
        rows[3],
    );
}

fn kind_tabs(p: &Picker) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, k) in Picker::tabs().into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(Color::DarkGray)));
        }
        let name = k.map(|k| k.plural()).unwrap_or("All");
        let style = if k == p.kind {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::Gray)
        };
        spans.push(Span::styled(name, style));
    }
    Line::from(spans)
}

pub fn draw_heading_editor(frame: &mut Frame<'_>, app: &App) {
    let Some(ed) = &app.home.heading else {
        return;
    };
    let inner = popup(frame, 50, 7, "Heading");
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(3)])
        .split(inner);
    let what = match ed.rename {
        Some(_) => "Rename this heading (Tab: add a new one)",
        None => "Add a new heading",
    };
    frame.render_widget(
        Paragraph::new(what).style(Style::default().fg(Color::Gray)),
        rows[0],
    );
    frame.render_widget(input(&ed.name, "Name", true), rows[1]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_balance_over_two_columns() {
        assert_eq!(columns(&[3, 3, 2], 2), vec![vec![0, 2], vec![1]]);
        assert_eq!(columns(&[5, 1, 1, 1], 2), vec![vec![0], vec![1, 2, 3]]);
        assert_eq!(columns(&[1, 1], 1), vec![vec![0, 1]]);
    }
}
