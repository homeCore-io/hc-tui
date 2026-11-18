//! Plugins: the installed list, the Matter tab and the per-plugin
//! detail view.

use super::widgets::{highlight, short_ts, status_detail_row};
use crate::app::{App, Load, PluginDetailPanel, PluginsSub};
use crate::model::{PluginRecord, RequiresRole};
use crate::text::take_chars;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};

pub fn draw_plugins_tab(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.plugins_tab.detail.is_some() {
        draw_plugin_detail(frame, app, area);
        return;
    }
    let area = if app.has_matter() {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);
        let idx = match app.plugins_tab.sub {
            PluginsSub::Installed => 0,
            PluginsSub::Matter => 1,
        };
        let tabs = Tabs::new(vec![
            Line::from(format!("Installed ({})", app.plugins.len())),
            Line::from("Matter"),
        ])
        .select(idx)
        .block(Block::default().borders(Borders::ALL).title(" Plugins "))
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
        frame.render_widget(tabs, layout[0]);
        if app.plugins_tab.sub == PluginsSub::Matter {
            super::manage::draw_matter(frame, app, layout[1]);
            return;
        }
        layout[1]
    } else {
        area
    };
    let items: Vec<ListItem> = app
        .views
        .plugins
        .iter()
        .filter_map(|&i| app.plugins.get(i))
        .map(|p| {
            let (dot, color) = status_dot(p);
            let version = p.version.as_deref().unwrap_or("-");
            let notices = if p.notices.is_empty() {
                String::new()
            } else {
                format!("  ⚠ {}", p.notices.len())
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {dot} "), Style::default().fg(color)),
                Span::styled(
                    format!("{:<30}", take_chars(&p.plugin_id, 30)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(format!("{:<10}", p.status), Style::default().fg(color)),
                Span::styled(
                    format!("{:<10}", take_chars(version, 10)),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    format!("{:>4} dev", p.device_count),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(notices, Style::default().fg(Color::Yellow)),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Installed ({}) ", app.views.plugins.len())),
        )
        .highlight_style(highlight())
        .highlight_symbol(">> ");
    let mut state = ListState::default();
    if !app.views.plugins.is_empty() {
        state.select(Some(app.plugins_tab.sel.index()));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn status_dot(p: &PluginRecord) -> (&'static str, Color) {
    match p.status.as_str() {
        "active" => ("●", Color::Green),
        "degraded" | "starting" => ("●", Color::Yellow),
        _ if !p.enabled => ("○", Color::DarkGray),
        _ => ("○", Color::Red),
    }
}

fn draw_plugin_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(d) = app.plugins_tab.detail.as_ref() else {
        return;
    };
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5)])
        .split(area);
    let tabs = Tabs::new(
        PluginDetailPanel::ALL
            .iter()
            .map(|p| Line::from(p.title()))
            .collect::<Vec<_>>(),
    )
    .select(d.panel.index())
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Plugin Detail: {}", d.plugin_id)),
    )
    .style(Style::default().fg(Color::Gray))
    .highlight_style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(tabs, layout[0]);

    match d.panel {
        PluginDetailPanel::Overview => draw_overview(frame, app, layout[1]),
        PluginDetailPanel::Actions => draw_actions(frame, app, layout[1]),
        PluginDetailPanel::Diagnostics => {
            let rows: Vec<ListItem> = app
                .plugin_events(&d.plugin_id)
                .take(50)
                .map(|e| {
                    ListItem::new(format!(
                        "{} | {} {}",
                        short_ts(&e.timestamp),
                        e.event_type,
                        e.event_detail.as_deref().unwrap_or("")
                    ))
                })
                .collect();
            let list = List::new(rows).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Recent events"),
            );
            frame.render_widget(list, layout[1]);
        }
    }
}

fn draw_overview(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Overview");
    let Some(p) = app.detail_plugin() else {
        let id = app
            .plugins_tab
            .detail
            .as_ref()
            .map(|d| d.plugin_id.as_str())
            .unwrap_or("");
        frame.render_widget(
            Paragraph::new(format!("Plugin '{id}' is not in the current plugin list."))
                .block(block),
            area,
        );
        return;
    };
    let yes_no = |b: bool| if b { "yes" } else { "no" }.to_string();
    let mut lines = vec![
        status_detail_row("Plugin", &p.plugin_id),
        status_detail_row("Version", p.version.as_deref().unwrap_or("-")),
    ];
    if let Some(iv) = p.installed_version.as_deref()
        && Some(iv) != p.version.as_deref()
    {
        lines.push(status_detail_row("Installed", iv));
    }
    lines.extend([
        status_detail_row("Status", &p.status),
        status_detail_row("Enabled", &yes_no(p.enabled)),
        status_detail_row("Managed", &yes_no(p.managed)),
        status_detail_row("Restarts", &p.restart_count.to_string()),
        status_detail_row("Log level", p.log_level.as_deref().unwrap_or("-")),
        status_detail_row("Devices", &p.device_count.to_string()),
        status_detail_row("Registered", &short_ts(&p.registered_at)),
    ]);
    if let Some(hb) = p.last_heartbeat.as_deref() {
        lines.push(status_detail_row("Heartbeat", &short_ts(hb)));
    }
    lines.push(Line::from(""));
    if p.notices.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No notices",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  Notices",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        for n in &p.notices {
            let color = match n.level.as_str() {
                "error" | "critical" => Color::Red,
                "warning" | "warn" => Color::Yellow,
                _ => Color::White,
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  [{}] ", n.level), Style::default().fg(color)),
                Span::raw(n.message.clone()),
            ]));
            if let Some(r) = &n.remedy {
                lines.push(Line::from(Span::styled(
                    format!("      {r}"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_actions(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(d) = app.plugins_tab.detail.as_ref() else {
        return;
    };
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(area);
    let title = match &d.caps {
        Load::Loading => "Actions [loading...]".to_string(),
        Load::Ready(c) => format!("Actions ({} actions, spec v{})", c.actions.len(), c.spec),
        Load::Failed(_) => "Actions [error]".to_string(),
        Load::Idle => "Actions".to_string(),
    };
    let items: Vec<ListItem> = d
        .caps
        .ready()
        .map(|c| {
            c.actions
                .iter()
                .map(|a| {
                    let role_color = match a.requires_role {
                        RequiresRole::Admin => Color::Red,
                        RequiresRole::User => Color::Cyan,
                        RequiresRole::ReadOnly | RequiresRole::SignedIn => Color::DarkGray,
                    };
                    let (badge, bc) = if a.stream {
                        ("stream", Color::Magenta)
                    } else {
                        ("sync  ", Color::DarkGray)
                    };
                    let mut spans = vec![
                        Span::styled(format!("  {:<24}", take_chars(&a.id, 24)), Style::default()),
                        Span::styled(
                            format!(" {:<10}", a.requires_role.as_str()),
                            Style::default().fg(role_color),
                        ),
                        Span::styled(format!(" {badge}"), Style::default().fg(bc)),
                        Span::styled(
                            format!(" {:<7}", a.concurrency.as_str()),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ];
                    if a.danger {
                        spans.push(Span::styled(" ⚠", Style::default().fg(Color::Red)));
                    }
                    spans.push(Span::styled(
                        format!(" {}", a.label),
                        Style::default().fg(Color::White),
                    ));
                    ListItem::new(Line::from(spans))
                })
                .collect()
        })
        .unwrap_or_default();
    let n = items.len();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(highlight())
        .highlight_symbol(">> ");
    let mut state = ListState::default();
    if n > 0 {
        state.select(Some(d.action_sel.index().min(n - 1)));
    }
    frame.render_stateful_widget(list, layout[0], &mut state);

    let (text, color) = if d.action_busy {
        (d.action_status.clone(), Color::Yellow)
    } else if !d.action_status.is_empty() {
        let failed =
            d.action_status.contains(" failed:") || d.action_status.contains(" requires admin");
        (
            d.action_status.clone(),
            if failed { Color::Red } else { Color::Green },
        )
    } else if let Some(e) = d.caps.error() {
        (format!("Error loading capabilities: {e}"), Color::Red)
    } else if d.caps.ready().is_some_and(|c| c.actions.is_empty()) {
        (
            "This plugin declares no actions.".to_string(),
            Color::DarkGray,
        )
    } else {
        (
            "Up/Down to select · Enter to run · Esc to close detail".to_string(),
            Color::DarkGray,
        )
    };
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title("Status"))
            .style(Style::default().fg(color))
            .wrap(Wrap { trim: true }),
        layout[1],
    );
}
