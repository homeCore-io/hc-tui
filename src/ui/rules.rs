//! Automations: list, Recent runs pane and the read-only detail view.

use super::widgets::{heading, highlight};
use crate::app::{App, Load, format_timestamp_utc};
use crate::model::RuleFiring;
use crate::text::take_chars;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, TableState, Wrap},
};

pub fn draw_rules_tab(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.rules_tab.detail.is_some() {
        draw_rule_detail(frame, app, area);
        return;
    }
    if app.rules_tab.history.is_some() {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);
        draw_rules_list(frame, app, panes[0]);
        draw_history_pane(frame, app, panes[1]);
    } else {
        draw_rules_list(frame, app, area);
    }
}

fn firing_line(f: &RuleFiring, utc: bool) -> Line<'static> {
    let (glyph, color) = if f.outcome.fired() {
        ("✓", Color::Green)
    } else {
        ("✗", Color::Yellow)
    };
    let mut spans = vec![
        Span::styled(format!("{glyph} "), Style::default().fg(color)),
        Span::raw(format!("{}  ", format_timestamp_utc(&f.timestamp, utc))),
        Span::styled(
            format!("{:<20} ", take_chars(&f.trigger_type, 20)),
            Style::default().fg(Color::DarkGray),
        ),
    ];
    if f.outcome.fired() {
        let ok = f.actions_ok();
        let failed = f.actions_failed();
        spans.push(Span::styled(
            format!("{ok} action{}", if ok == 1 { "" } else { "s" }),
            Style::default().fg(Color::Cyan),
        ));
        if failed > 0 {
            spans.push(Span::styled(
                format!(", {failed} failed"),
                Style::default().fg(Color::Red),
            ));
        }
    } else {
        spans.push(Span::styled(
            f.outcome_label(),
            Style::default().fg(Color::Yellow),
        ));
    }
    spans.push(Span::styled(
        format!("  {}ms", f.eval_ms),
        Style::default().fg(Color::DarkGray),
    ));
    Line::from(spans)
}

fn firing_items(load: &Load<Vec<RuleFiring>>, take: usize, utc: bool) -> Vec<ListItem<'static>> {
    let dim = |s: String| {
        vec![ListItem::new(Line::from(Span::styled(
            s,
            Style::default().fg(Color::DarkGray),
        )))]
    };
    match load {
        Load::Ready(h) if h.is_empty() => dim("no recent firings".into()),
        Load::Ready(h) => h
            .iter()
            .take(take)
            .map(|f| ListItem::new(firing_line(f, utc)))
            .collect(),
        Load::Loading | Load::Idle => dim("loading...".into()),
        Load::Failed(e) => vec![ListItem::new(Line::from(Span::styled(
            format!("history unavailable: {e}"),
            Style::default().fg(Color::Red),
        )))],
    }
}

fn draw_history_pane(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(h) = app.rules_tab.history.as_ref() else {
        return;
    };
    let name = app
        .rules
        .iter()
        .find(|r| r.id == h.rule_id)
        .map(|r| r.name.as_str())
        .unwrap_or("?");
    let take = area.height.saturating_sub(2) as usize;
    let list = List::new(firing_items(&h.items, take.max(1), app.time_utc)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Recent runs: {name}")),
    );
    frame.render_widget(list, area);
}

fn draw_rule_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(d) = app.rules_tab.detail.as_ref() else {
        return;
    };
    let rule = app.rules.iter().find(|r| r.id == d.rule_id);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(5),
            Constraint::Length(8),
        ])
        .split(area);

    let header: Vec<Line> = match rule {
        Some(r) => {
            let mut spans = vec![
                Span::styled(
                    r.name.clone(),
                    Style::default()
                        .add_modifier(Modifier::BOLD)
                        .fg(Color::White),
                ),
                Span::raw("  "),
                if r.enabled {
                    Span::styled("● enabled", Style::default().fg(Color::Green))
                } else {
                    Span::styled("○ disabled", Style::default().fg(Color::DarkGray))
                },
                Span::raw(format!("  · priority {}", r.priority)),
            ];
            if !r.tags.is_empty() {
                spans.push(Span::raw(format!("  · tags [{}]", r.tags.join(","))));
            }
            let mut lines = vec![
                Line::from(spans),
                Line::from(Span::styled(
                    format!("triggers: {}", r.trigger_names().join(", ")),
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            if let Some(err) = &r.error {
                lines.push(Line::from(Span::styled(
                    format!("error: {err}"),
                    Style::default().fg(Color::Red),
                )));
            }
            lines
        }
        None => vec![Line::from(Span::styled(
            "no longer in homeCore",
            Style::default().fg(Color::Yellow),
        ))],
    };
    frame.render_widget(
        Paragraph::new(header).block(Block::default().borders(Borders::ALL).title(" Automation ")),
        layout[0],
    );

    let (ron_text, ron_style) = match &d.ron {
        Load::Ready(t) => (t.clone(), Style::default().fg(Color::Gray)),
        Load::Loading | Load::Idle => (
            "loading...".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
        Load::Failed(e) => (
            format!("RON unavailable: {e}"),
            Style::default().fg(Color::Yellow),
        ),
    };
    frame.render_widget(
        Paragraph::new(ron_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" RON  (j/k scroll · r refresh · Esc back) "),
            )
            .style(ron_style)
            .scroll((d.scroll, 0))
            .wrap(Wrap { trim: false }),
        layout[1],
    );

    let take = (layout[2].height.saturating_sub(2) as usize).max(1);
    let title = match d.history.ready() {
        Some(h) => format!(" Recent runs ({}) ", h.len()),
        None => " Recent runs ".to_string(),
    };
    frame.render_widget(
        List::new(firing_items(&d.history, take, app.time_utc))
            .block(Block::default().borders(Borders::ALL).title(title)),
        layout[2],
    );
}

fn draw_rules_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let tab = &app.rules_tab;
    let total = app.rules.len();
    let shown = app.views.rules.len();
    let marked = tab.marked.len();
    let title = if shown != total {
        format!(" Automations ({shown} of {total}) ")
    } else if marked > 0 {
        format!(" Automations ({total}, {marked} marked) ")
    } else {
        format!(" Automations ({total}) ")
    };

    let hl = highlight();
    let hl_bulk = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let header = Row::new(
        ["Sel", "●", "Name", "Trigger", "Tags", "Pri"]
            .iter()
            .map(|h| Cell::from(*h).style(heading())),
    );
    let selected = app.selected_rule().map(|r| r.id.clone());
    let mut sel_row = None;
    let rows: Vec<Row> = app
        .visible_rules()
        .enumerate()
        .map(|(i, r)| {
            let is_sel = selected.as_deref() == Some(r.id.as_str());
            if is_sel {
                sel_row = Some(i);
            }
            let is_bulk = tab.marked.contains(&r.id);
            let stale = r.error.is_some();
            let mark = if is_bulk { "[✓]" } else { "[ ]" };
            let dot = if r.enabled { "●" } else { "○" };
            let name = take_chars(
                &format!("{}{}", if stale { "⚠ " } else { "  " }, r.name),
                32,
            );
            let trigger = take_chars(&r.trigger_label(), 16);
            let tags = take_chars(&r.tags.join(","), 20);
            let pri = r.priority.to_string();
            let base = if stale {
                Style::default().fg(Color::Red)
            } else if r.enabled {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let dot_color = if r.enabled {
                Color::Green
            } else {
                Color::DarkGray
            };
            if is_sel {
                Row::new(
                    [mark.to_string(), dot.to_string(), name, trigger, tags, pri]
                        .map(|c| Cell::from(c).style(hl)),
                )
            } else if is_bulk {
                Row::new(vec![
                    Cell::from(mark).style(hl_bulk),
                    Cell::from(dot).style(Style::default().fg(dot_color)),
                    Cell::from(name).style(base.add_modifier(Modifier::BOLD)),
                    Cell::from(trigger).style(base),
                    Cell::from(tags).style(base),
                    Cell::from(pri).style(base),
                ])
            } else {
                Row::new(vec![
                    Cell::from(mark).style(Style::default().fg(Color::DarkGray)),
                    Cell::from(dot).style(Style::default().fg(dot_color)),
                    Cell::from(name).style(base),
                    Cell::from(trigger).style(Style::default().fg(Color::DarkGray)),
                    Cell::from(tags).style(Style::default().fg(Color::Cyan)),
                    Cell::from(pri).style(Style::default().fg(Color::DarkGray)),
                ])
            }
        })
        .collect();
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(if total == 0 {
                "No automations."
            } else {
                "No automations match."
            })
            .block(Block::default().borders(Borders::ALL).title(title))
            .alignment(Alignment::Center),
            area,
        );
        return;
    }
    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Length(3),
            Constraint::Min(30),
            Constraint::Length(16),
            Constraint::Length(20),
            Constraint::Length(4),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(title))
    .row_highlight_style(hl);
    let mut state = TableState::default();
    state.select(sel_row);
    frame.render_stateful_widget(table, area, &mut state);
}
