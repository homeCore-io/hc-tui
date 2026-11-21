//! Manage tab and its sub-panels.

use super::widgets::highlight;
use crate::app::App;
use crate::app::manage::logs::LogLevelFilter;
use crate::model::AuditResult;
use crate::text::take_chars;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};

fn selectable_list<'a>(items: Vec<ListItem<'a>>, title: String) -> List<'a> {
    List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(highlight())
        .highlight_symbol(">> ")
}

fn render_list(frame: &mut Frame<'_>, list: List<'_>, len: usize, index: usize, area: Rect) {
    let mut state = ListState::default();
    if len > 0 {
        state.select(Some(index.min(len - 1)));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

pub fn draw_matter(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let m = &app.manage.matter;
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(7)])
        .split(area);
    let items: Vec<ListItem> = app
        .matter_nodes
        .iter()
        .map(|n| {
            let extra = n
                .extra
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| format!("  {s}"))
                .unwrap_or_default();
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {:<30}", take_chars(&n.node_id, 30)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{} device(s)", n.device_count),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(extra, Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();
    let mut title = format!("Matter nodes ({})", app.matter_nodes.len());
    if m.pending {
        title.push_str(" [pending]");
    }
    if m.loading {
        title.push_str(" [loading...]");
    }
    let n = items.len();
    render_list(
        frame,
        selectable_list(items, title),
        n,
        m.sel.index(),
        layout[0],
    );

    let mut lines = vec![Line::from(vec![
        Span::styled("Last: ", Style::default().fg(Color::DarkGray)),
        Span::styled(m.last_action.clone(), Style::default().fg(Color::White)),
    ])];
    if m.activity.is_empty() {
        lines.push(Line::from(Span::styled(
            "No recent Matter activity",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for line in m.activity.iter().take(4) {
            lines.push(Line::from(Span::styled(
                line.clone(),
                Style::default().fg(Color::Gray),
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Matter activity"),
            )
            .wrap(Wrap { trim: true }),
        layout[1],
    );
}

pub fn draw_logs(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let logs = &app.manage.logs;
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(3)])
        .split(area);
    let level_style = |l: LogLevelFilter, c: Color| {
        if logs.level == l {
            Style::default().fg(c).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        }
    };
    let labels = vec![
        Line::from(Span::styled(
            "e ERROR",
            level_style(LogLevelFilter::Error, Color::Red),
        )),
        Line::from(Span::styled(
            "w WARN",
            level_style(LogLevelFilter::Warn, Color::Yellow),
        )),
        Line::from(Span::styled(
            "i INFO",
            level_style(LogLevelFilter::Info, Color::Cyan),
        )),
        Line::from(Span::styled(
            "d DEBUG",
            level_style(LogLevelFilter::Debug, Color::White),
        )),
    ];
    let idx = match logs.level {
        LogLevelFilter::Error => 0,
        LogLevelFilter::Warn => 1,
        LogLevelFilter::Info => 2,
        LogLevelFilter::Debug => 3,
    };
    let live = if logs.connected { "●" } else { "○" };
    let pause = if logs.paused { " [PAUSED]" } else { " [LIVE]" };
    let q = app.query(crate::screens::ScreenId::Logs);
    let shown = app.visible_log_lines();
    let filter = if q.is_empty() {
        String::new()
    } else {
        format!(" | {} match /{q}", shown.len())
    };
    let admin_note = if !app.is_admin() {
        " (admin only)".to_string()
    } else if let (false, Some(e)) = (logs.connected, logs.last_error.as_deref()) {
        format!(" ({})", take_chars(e, 60))
    } else {
        String::new()
    };
    let title = format!(
        "Logs {live} | {} lines{pause}{filter}{admin_note}",
        logs.lines.len()
    );
    frame.render_widget(
        Tabs::new(labels)
            .select(idx)
            .block(Block::default().borders(Borders::ALL).title(title))
            .style(Style::default().fg(Color::Gray)),
        layout[0],
    );

    let body = layout[1];
    let height = body.height.saturating_sub(2) as usize;
    let total = shown.len();
    // `scroll` counts up from the newest line.
    let end = total - logs.scroll.min(total.saturating_sub(height));
    let start = end.saturating_sub(height);
    let items: Vec<ListItem> = shown
        .iter()
        .skip(start)
        .take(height)
        .map(|line| {
            let level = line.level.to_uppercase();
            let (color, m) = match level.as_str() {
                "ERROR" => (Color::Red, Modifier::BOLD),
                "WARN" => (Color::Yellow, Modifier::empty()),
                "INFO" => (Color::Cyan, Modifier::empty()),
                _ => (Color::DarkGray, Modifier::empty()),
            };
            let ts: String = line.timestamp.chars().skip(11).take(8).collect();
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("[{level:<5}] "),
                    Style::default().fg(color).add_modifier(m),
                ),
                Span::styled(format!("{ts} "), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:<24} ", take_chars(&line.target, 24)),
                    Style::default().fg(Color::Blue),
                ),
                Span::raw(line.message.clone()),
            ]))
        })
        .collect();
    let block_title = if total > height && height > 0 {
        format!("Log lines {} to {} of {total}", start + 1, end)
    } else {
        "Log lines".to_string()
    };
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(block_title)),
        body,
    );
}

pub fn draw_events(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .visible_events()
        .map(|e| {
            let mut extra = String::new();
            if let Some(d) = &e.device_id {
                let name = app
                    .device_by_id(d)
                    .map(|x| x.display_name().to_string())
                    .unwrap_or_else(|| d.clone());
                extra = format!(" device={name}");
            } else if let Some(r) = &e.rule_name {
                extra = format!(" rule={r}");
            } else if let Some(p) = &e.plugin_id {
                extra = format!(" plugin={p}");
            }
            if let Some(d) = &e.event_detail {
                extra.push_str(&format!(" {d}"));
            }
            let (tag, color) = if e.custom {
                ("CUS", Color::Magenta)
            } else if e.event_type.starts_with("device_") {
                ("DEV", Color::LightBlue)
            } else if e.event_type.starts_with("rule_") || e.event_type == "action_failed" {
                ("RUL", Color::LightGreen)
            } else if e.event_type.starts_with("alert_") || e.event_type == "system_alert" {
                ("ALR", Color::Red)
            } else if e.event_type.starts_with("plugin_") {
                ("PLG", Color::Yellow)
            } else {
                ("EVT", Color::DarkGray)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("[{tag}] "), Style::default().fg(color)),
                Span::raw(format!(
                    "{} | {}{extra}",
                    crate::app::format_timestamp_utc(&e.timestamp, app.time_utc),
                    e.event_type
                )),
            ]))
        })
        .collect();
    let title = format!(
        "Events [{}] ({})",
        app.manage.events.type_filter.as_deref().unwrap_or("all"),
        items.len()
    );
    let n = items.len();
    render_list(
        frame,
        selectable_list(items, title),
        n,
        app.manage.events.sel.index(),
        area,
    );
}

pub fn draw_audit(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let a = &app.manage.audit;
    let expanded = app.expanded_audit();
    let constraints = if expanded.is_some() {
        vec![
            Constraint::Min(6),
            Constraint::Length(10),
            Constraint::Length(3),
        ]
    } else {
        vec![Constraint::Min(6), Constraint::Length(3)]
    };
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);
    let items: Vec<ListItem> = app
        .visible_audit()
        .into_iter()
        .map(|e| {
            let rc = match e.result {
                AuditResult::Success => Color::Green,
                AuditResult::Denied => Color::Yellow,
                AuditResult::Error => Color::Red,
                AuditResult::Other => Color::Gray,
            };
            let ts = e.ts.split('.').next().unwrap_or(&e.ts).to_string();
            let target = match (&e.target_kind, &e.target_id) {
                (Some(k), Some(id)) => format!("{k}/{id}"),
                (Some(k), None) => k.clone(),
                _ => String::new(),
            };
            ListItem::new(Line::from(vec![
                Span::raw(format!("  {:<19} ", take_chars(&ts, 19))),
                Span::styled(
                    format!("{:<14}", e.actor_type.as_str()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<24}", take_chars(&e.actor_label, 24)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{:<28}", take_chars(&e.event_type, 28)),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("{:<10}", e.result.as_str()),
                    Style::default().fg(rc),
                ),
                Span::styled(
                    take_chars(&target, 40),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();
    let title = if a.loading {
        "Audit [loading...]".to_string()
    } else {
        format!(
            "Audit · {} ({} entries) · f to change",
            a.filter.label(),
            items.len()
        )
    };
    let n = items.len();
    render_list(
        frame,
        selectable_list(items, title),
        n,
        a.sel.index(),
        layout[0],
    );

    if let Some(e) = expanded {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("ts: ", Style::default().fg(Color::DarkGray)),
                Span::raw(e.ts.clone()),
            ]),
            Line::from(vec![
                Span::styled("actor: ", Style::default().fg(Color::DarkGray)),
                Span::raw(format!("{} ({})", e.actor_label, e.actor_type.as_str())),
            ]),
        ];
        if let Some(scope) = &e.scope_used {
            lines.push(Line::from(vec![
                Span::styled("scope: ", Style::default().fg(Color::DarkGray)),
                Span::styled(scope.clone(), Style::default().fg(Color::Cyan)),
            ]));
        }
        if let Some(ip) = &e.ip {
            lines.push(Line::from(vec![
                Span::styled("ip: ", Style::default().fg(Color::DarkGray)),
                Span::raw(ip.clone()),
            ]));
        }
        let detail =
            serde_json::to_string_pretty(&e.detail).unwrap_or_else(|_| e.detail.to_string());
        lines.extend(detail.lines().take(20).map(|l| Line::from(l.to_string())));
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("Detail"))
                .wrap(Wrap { trim: false }),
            layout[1],
        );
    }

    let footer_area = layout[layout.len() - 1];
    let (text, color) = match &a.error {
        Some(err) => (format!("Error: {err}"), Color::Red),
        None => (
            format!(
                "page {} · {} per page · ] next page · [ previous page · Enter detail",
                a.offset / a.limit.max(1) + 1,
                a.limit
            ),
            Color::DarkGray,
        ),
    };
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title("Status"))
            .style(Style::default().fg(color))
            .wrap(Wrap { trim: true }),
        footer_area,
    );
}
