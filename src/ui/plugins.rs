//! Plugins: the tabs (Installed, Catalog, Runtimes, Matter), the
//! per-plugin detail with its own tabs, and the Plugins boxes (an
//! action's parameters, choosing a runtime, a new runtime key).

use super::lists::age;
use super::widgets::{centered_rect, heading, highlight, mask, short_ts, status_detail_row};
use crate::app::{App, Load, PluginDetailPanel, PluginsSub};
use crate::model::RequiresRole;
use crate::plugin_admin::{self as pa, Tone};
use crate::plugin_form::{Control, Row as FormRow, describe};
use crate::text::{take_chars, truncate};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState,
        Tabs, Wrap,
    },
};

fn tone_color(t: Tone) -> Color {
    match t {
        Tone::Good => Color::Green,
        Tone::Quiet => Color::Gray,
        Tone::Bad => Color::Red,
    }
}

fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn split(area: Rect, dir: Direction, c: Vec<Constraint>) -> std::rc::Rc<[Rect]> {
    Layout::default().direction(dir).constraints(c).split(area)
}

pub fn draw_plugins_tab(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let layout = split(
        area,
        Direction::Vertical,
        vec![Constraint::Length(3), Constraint::Min(0)],
    );
    let subs = app.plugin_subs();
    let titles: Vec<Line> = subs
        .iter()
        .map(|s| match s {
            PluginsSub::Installed => Line::from(format!("Installed ({})", app.plugins.len())),
            PluginsSub::Catalog => match &app.plugins_tab.manage.registry {
                Load::Ready(_) => Line::from(format!("Catalog ({})", app.catalog_view().len())),
                _ => Line::from("Catalog"),
            },
            PluginsSub::Runtimes => match &app.plugins_tab.manage.runtimes {
                Load::Ready(r) => Line::from(format!("Runtimes ({})", r.len())),
                _ => Line::from("Runtimes"),
            },
            PluginsSub::Matter => Line::from(s.title()),
        })
        .collect();
    let sub = app.plugin_sub();
    let idx = subs.iter().position(|s| *s == sub).unwrap_or(0);
    let tabs = Tabs::new(titles)
        .select(idx)
        .block(Block::default().borders(Borders::ALL).title(" Plugins "))
        .style(Style::default().fg(Color::Gray))
        .highlight_style(heading());
    frame.render_widget(tabs, layout[0]);
    let body = layout[1];
    match sub {
        PluginsSub::Installed => {
            if app.plugins_tab.detail.is_some() {
                // The list stays above, a few rows of it, as in the mockup.
                let rows = (app.views.plugins.len() as u16 + 3).min(8);
                let parts = split(
                    body,
                    Direction::Vertical,
                    vec![Constraint::Length(rows), Constraint::Min(8)],
                );
                draw_installed(frame, app, parts[0]);
                draw_plugin_detail(frame, app, parts[1]);
            } else {
                draw_installed(frame, app, body);
            }
        }
        PluginsSub::Catalog => draw_catalog(frame, app, body),
        PluginsSub::Runtimes => draw_runtimes(frame, app, body),
        PluginsSub::Matter => super::manage::draw_matter(frame, app, body),
    }
}

// ── Installed ───────────────────────────────────────────────────────────────

fn draw_installed(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let rows: Vec<Row> = app
        .views
        .plugins
        .iter()
        .filter_map(|&i| app.plugins.get(i))
        .map(|p| {
            let (state, tone) = pa::state_of(p);
            let update = app.plugin_update(p);
            let mut notes: Vec<Span> = Vec::new();
            for (i, (w, t)) in pa::notes_of(p, update.as_deref()).into_iter().enumerate() {
                if i > 0 {
                    notes.push(Span::styled(" · ", dim()));
                }
                notes.push(Span::styled(
                    w,
                    Style::default().fg(match t {
                        Tone::Quiet => Color::Yellow,
                        t => tone_color(t),
                    }),
                ));
            }
            Row::new(vec![
                Cell::from(format!("  {}", app.plugin_display_name(p))),
                Cell::from(pa::version_of(p).unwrap_or("-").to_string()).style(dim()),
                Cell::from(state).style(Style::default().fg(tone_color(tone))),
                Cell::from(format!("{:>7}", p.device_count)),
                Cell::from(Line::from(notes)),
            ])
        })
        .collect();
    let header = Row::new(
        ["  Plugin", "Version", "State", "Devices", "Notes"]
            .iter()
            .map(|h| Cell::from(*h).style(heading())),
    );
    let n = rows.len();
    let table = Table::new(
        rows,
        [
            Constraint::Length(22),
            Constraint::Length(10),
            Constraint::Length(22),
            Constraint::Length(8),
            Constraint::Min(10),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL))
    .row_highlight_style(highlight());
    let mut state = TableState::default();
    if n > 0 {
        state.select(Some(app.plugins_tab.sel.index().min(n - 1)));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

// ── Detail ──────────────────────────────────────────────────────────────────

fn draw_plugin_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(d) = app.plugins_tab.detail.as_ref() else {
        return;
    };
    let layout = split(
        area,
        Direction::Vertical,
        vec![Constraint::Length(3), Constraint::Min(5)],
    );
    let name = app
        .detail_plugin()
        .map(|p| app.plugin_display_name(p))
        .unwrap_or_else(|| d.plugin_id.clone());
    let tabs = Tabs::new(
        PluginDetailPanel::ALL
            .iter()
            .map(|p| match p {
                PluginDetailPanel::Notices => {
                    let n = app.detail_plugin().map(|p| p.notices.len()).unwrap_or(0);
                    if n > 0 {
                        Line::from(format!("Notices ({n})"))
                    } else {
                        Line::from("Notices")
                    }
                }
                p => Line::from(p.title()),
            })
            .collect::<Vec<_>>(),
    )
    .select(d.panel.index())
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {name} ")),
    )
    .style(Style::default().fg(Color::Gray))
    .highlight_style(heading());
    frame.render_widget(tabs, layout[0]);

    match d.panel {
        PluginDetailPanel::Overview => draw_overview(frame, app, layout[1]),
        PluginDetailPanel::Settings => draw_settings(frame, app, layout[1]),
        PluginDetailPanel::Actions => draw_actions(frame, app, layout[1]),
        PluginDetailPanel::Notices => draw_notices(frame, app, layout[1]),
        PluginDetailPanel::Logs => draw_logs(frame, app, layout[1]),
    }
}

fn not_listed(frame: &mut Frame<'_>, app: &App, area: Rect, block: Block<'_>) {
    let id = app
        .plugins_tab
        .detail
        .as_ref()
        .map(|d| d.plugin_id.as_str())
        .unwrap_or("");
    frame.render_widget(
        Paragraph::new(format!("{id} is not in the plugin list any more.")).block(block),
        area,
    );
}

fn draw_overview(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Overview");
    let Some(p) = app.detail_plugin() else {
        return not_listed(frame, app, area, block);
    };
    let parts = split(
        area,
        Direction::Vertical,
        vec![Constraint::Min(8), Constraint::Length(8)],
    );
    let yes_no = |b: bool| if b { "yes" } else { "no" }.to_string();
    let (state, tone) = pa::state_of(p);
    let mut lines = vec![
        status_detail_row("Plugin", &p.plugin_id),
        status_detail_row("Version", pa::version_of(p).unwrap_or("-")),
    ];
    if let Some(iv) = p.installed_version.as_deref()
        && Some(iv) != p.version.as_deref()
    {
        lines.push(status_detail_row("Installed", iv));
    }
    if let Some(u) = app.plugin_update(p) {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<16}", "Update"), dim()),
            Span::styled(
                format!("{u} in the registry (U)"),
                Style::default().fg(Color::Green),
            ),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<16}", "State"), dim()),
        Span::styled(state.to_string(), Style::default().fg(tone_color(tone))),
        Span::styled(format!("  ({})", p.status), dim()),
    ]));
    if let Some(up) = p.uptime_started.as_deref().filter(|_| p.status == "active") {
        lines.push(status_detail_row(
            "Running",
            &format!("since {}", short_ts(up)),
        ));
    }
    lines.extend([
        status_detail_row("Turned on", &yes_no(p.enabled)),
        status_detail_row(
            "Runs",
            &match app.placement_of(&p.plugin_id) {
                Some(pl) => {
                    let name = app
                        .runtime_list()
                        .iter()
                        .find(|r| r.runtime_id == pl.runtime_id)
                        .map(|r| r.name.clone())
                        .unwrap_or_else(|| pl.runtime_id.clone());
                    format!("in {name}")
                }
                None if p.managed => "here, started by homeCore".to_string(),
                None => "elsewhere".to_string(),
            },
        ),
        status_detail_row("Restarts", &p.restart_count.to_string()),
        status_detail_row("Log level", p.log_level.as_deref().unwrap_or("-")),
        status_detail_row("Devices", &p.device_count.to_string()),
        status_detail_row("Registered", &short_ts(&p.registered_at)),
    ]);
    if let Some(hb) = p.last_heartbeat.as_deref() {
        lines.push(status_detail_row("Last heard", &age(hb)));
    }
    if let Some(n) = pa::worst_notice(&p.notices) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {} (see Notices)", n.message),
            Style::default().fg(notice_color(&n.level)),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        parts[0],
    );
    let rows: Vec<ListItem> = app
        .plugin_events(&p.plugin_id)
        .take(20)
        .map(|e| {
            ListItem::new(format!(
                "{}  {} {}",
                short_ts(&e.timestamp),
                e.event_type,
                e.event_detail.as_deref().unwrap_or("")
            ))
        })
        .collect();
    frame.render_widget(
        List::new(rows).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Recent events"),
        ),
        parts[1],
    );
}

fn notice_color(level: &str) -> Color {
    match level {
        "error" | "critical" => Color::Red,
        "warning" | "warn" => Color::Yellow,
        _ => Color::White,
    }
}

fn draw_notices(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Notices");
    let Some(p) = app.detail_plugin() else {
        return not_listed(frame, app, area, block);
    };
    let skip = app
        .plugins_tab
        .detail
        .as_ref()
        .map(|d| d.notices_scroll)
        .unwrap_or(0);
    let mut lines = Vec::new();
    if p.notices.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Nothing to report. A plugin says here when it cannot do its job.",
            dim(),
        )));
    }
    for n in pa::sorted_notices(&p.notices).into_iter().skip(skip) {
        let color = notice_color(&n.level);
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<8}", n.level),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(n.message.clone()),
        ]));
        if let Some(r) = &n.remedy {
            lines.push(Line::from(Span::styled(
                format!("          {r}"),
                Style::default().fg(Color::Gray),
            )));
        }
        lines.push(Line::from(Span::styled(
            format!("          {}", n.code),
            dim(),
        )));
        lines.push(Line::from(""));
    }
    if !p.notices.is_empty() {
        lines.push(Line::from(Span::styled(
            "  A notice goes away when the plugin reports the problem fixed.",
            dim(),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_settings(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(d) = app.plugins_tab.detail.as_ref() else {
        return;
    };
    let s = &d.settings;
    let block = Block::default().borders(Borders::ALL).title("Settings");
    let form = match &s.form {
        Load::Ready(f) => f,
        Load::Failed(e) => {
            frame.render_widget(
                Paragraph::new(format!("  {e}"))
                    .style(Style::default().fg(Color::Yellow))
                    .block(block)
                    .wrap(Wrap { trim: false }),
                area,
            );
            return;
        }
        _ => {
            frame.render_widget(Paragraph::new("  Loading...").block(block), area);
            return;
        }
    };
    let parts = split(
        area,
        Direction::Vertical,
        vec![Constraint::Min(4), Constraint::Length(4)],
    );
    let selected = s.row();
    let mut items: Vec<ListItem> = Vec::new();
    let mut sel_item = None;
    for i in form.visible_rows() {
        match &form.rows[i] {
            FormRow::Heading(h) => items.push(ListItem::new(Line::from(Span::styled(
                format!(" {h}"),
                heading(),
            )))),
            FormRow::Note(t) => items.push(ListItem::new(Line::from(Span::styled(
                format!("   {t}"),
                dim(),
            )))),
            FormRow::Field(f) => {
                if Some(i) == selected {
                    sel_item = Some(items.len());
                }
                let typing = Some(i) == selected && s.input.is_some();
                let (value, is_default) = if typing {
                    let text = s.input.as_deref().unwrap_or("");
                    let shown = if f.control == Control::Secret {
                        mask(text, "•")
                    } else {
                        text.to_string()
                    };
                    (format!("{shown}█"), false)
                } else {
                    describe(f)
                };
                let value = match &f.control {
                    Control::Enum(_) if !typing && !f.read_only => format!("‹ {value} ›"),
                    Control::Toggle if !typing => {
                        let on = value == "on";
                        format!("[{}] {value}", if on { "x" } else { " " })
                    }
                    _ => value,
                };
                let changed = f.draft != f.original;
                let mut spans = vec![
                    Span::raw(if changed { " * " } else { "   " }),
                    Span::raw(format!("{:<34}", truncate(&f.label, 33))),
                    Span::styled(
                        value,
                        if typing {
                            Style::default().fg(Color::Yellow)
                        } else if is_default {
                            dim()
                        } else {
                            Style::default().fg(Color::White)
                        },
                    ),
                ];
                if f.read_only {
                    spans.push(Span::styled("  (in the file)", dim()));
                }
                items.push(ListItem::new(Line::from(spans)));
            }
        }
    }
    let title = if form.from_schema {
        "Settings (from the plugin's schema)".to_string()
    } else {
        "Settings".to_string()
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default();
    state.select(sel_item);
    frame.render_stateful_widget(list, parts[0], &mut state);

    // Below: what is wrong, what the selected setting is for, or how to save.
    let f = selected.and_then(|i| form.field_at(i));
    let mut lines: Vec<Line> = Vec::new();
    if let Some(e) = &s.input_error {
        lines.push(Line::from(Span::styled(
            e.clone(),
            Style::default().fg(Color::Red),
        )));
    } else if let Some(h) = f.and_then(|f| f.help.as_deref()) {
        lines.push(Line::from(h.to_string()));
    }
    let tail = if s.saving {
        "Saving...".to_string()
    } else if form.dirty() {
        format!(
            "{} changed · Ctrl+S save · Esc discard",
            form.changed().len()
        )
    } else if let Some(n) = &s.note {
        n.clone()
    } else {
        "Enter edit · Space change a choice · Ctrl+S save".to_string()
    };
    lines.push(Line::from(Span::styled(
        tail,
        Style::default().fg(Color::Cyan),
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        parts[1],
    );
}

fn draw_actions(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(d) = app.plugins_tab.detail.as_ref() else {
        return;
    };
    let layout = split(
        area,
        Direction::Vertical,
        vec![Constraint::Min(5), Constraint::Length(3)],
    );
    let title = match &d.caps {
        Load::Loading => "Actions [loading...]".to_string(),
        Load::Ready(c) => format!("Actions ({})", c.actions.len()),
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
                    let takes = a
                        .params
                        .as_ref()
                        .and_then(crate::plugin_form::params_descriptor)
                        .is_some();
                    let mut spans = vec![
                        Span::styled(
                            format!("  {:<28}", take_chars(a.display_label(), 28)),
                            Style::default(),
                        ),
                        Span::styled(
                            format!(" {:<10}", a.requires_role.as_str()),
                            Style::default().fg(role_color),
                        ),
                        Span::styled(
                            if a.stream { " stream" } else { "       " },
                            Style::default().fg(Color::Magenta),
                        ),
                        Span::styled(if takes { " asks" } else { "     " }, dim()),
                        Span::styled(format!(" {:<6}", a.concurrency.as_str()), dim()),
                    ];
                    if a.danger {
                        spans.push(Span::styled(" asks first", Style::default().fg(Color::Red)));
                    }
                    if let Some(desc) = &a.description {
                        spans.push(Span::styled(format!("  {}", truncate(desc, 60)), dim()));
                    }
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
            "j/k select · Enter run · Esc back".to_string(),
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

fn draw_logs(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(d) = app.plugins_tab.detail.as_ref() else {
        return;
    };
    let l = &d.logs;
    if !app.is_admin() {
        frame.render_widget(
            Paragraph::new("  The log stream needs the admin role.")
                .block(Block::default().borders(Borders::ALL).title("Logs")),
            area,
        );
        return;
    }
    let state = if l.connected {
        "live".to_string()
    } else if let Some(e) = &l.last_error {
        format!("disconnected: {}", truncate(e, 40))
    } else {
        "connecting...".to_string()
    };
    let paused = if l.paused { " · paused" } else { "" };
    let title = format!("Logs ({}, {state}{paused})", l.level.as_str());
    let inner_h = area.height.saturating_sub(2) as usize;
    let end = (l.scroll + 1).min(l.lines.len());
    let start = end.saturating_sub(inner_h);
    let lines: Vec<Line> = l
        .lines
        .iter()
        .skip(start)
        .take(end - start)
        .map(|line| {
            let color = match line.level.to_uppercase().as_str() {
                "ERROR" => Color::Red,
                "WARN" => Color::Yellow,
                "DEBUG" | "TRACE" => Color::DarkGray,
                _ => Color::White,
            };
            Line::from(vec![
                Span::styled(format!("{} ", short_ts(&line.timestamp)), dim()),
                Span::styled(format!("{:<5} ", line.level), Style::default().fg(color)),
                Span::raw(line.message.clone()),
            ])
        })
        .collect();
    let body = if lines.is_empty() {
        vec![Line::from(Span::styled("  Nothing yet.", dim()))]
    } else {
        lines
    };
    frame.render_widget(
        Paragraph::new(body).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

// ── Catalog ─────────────────────────────────────────────────────────────────

fn draw_catalog(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL);
    match &app.plugins_tab.manage.registry {
        Load::Failed(e) => {
            frame.render_widget(
                Paragraph::new(format!("  The registry is not available: {e}"))
                    .style(Style::default().fg(Color::Yellow))
                    .block(block)
                    .wrap(Wrap { trim: false }),
                area,
            );
            return;
        }
        Load::Ready(_) => {}
        _ => {
            frame.render_widget(
                Paragraph::new("  Loading the registry...").block(block),
                area,
            );
            return;
        }
    }
    let parts = split(
        area,
        Direction::Vertical,
        vec![Constraint::Min(5), Constraint::Length(7)],
    );
    let core = app.system_status.as_ref().map(|s| s.version.as_str());
    let view = app.catalog_view();
    let rows: Vec<Row> = view
        .iter()
        .map(|r| {
            Row::new(vec![
                Cell::from(format!("  {}", r.display_name())),
                Cell::from(pa::latest_of(r, core).unwrap_or("-").to_string()).style(dim()),
                Cell::from(r.kind(None)),
                Cell::from(r.category.clone().unwrap_or_default()).style(dim()),
                Cell::from(r.description.clone()),
            ])
        })
        .collect();
    let header = Row::new(
        ["  Plugin", "Version", "Kind", "Category", "What it does"]
            .iter()
            .map(|h| Cell::from(*h).style(heading())),
    );
    let n = rows.len();
    let table = Table::new(
        rows,
        [
            Constraint::Length(20),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Min(10),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(if n == 0 {
        " Every plugin in the registry is installed "
    } else {
        " Not installed yet "
    }))
    .row_highlight_style(highlight());
    let mut state = TableState::default();
    if n > 0 {
        state.select(Some(app.plugins_tab.manage.catalog_sel.index().min(n - 1)));
    }
    frame.render_stateful_widget(table, parts[0], &mut state);

    let mut lines: Vec<Line> = Vec::new();
    if let Some(w) = &app.plugins_tab.manage.working {
        lines.push(Line::from(Span::styled(
            format!("{} {w}", app.spinner()),
            Style::default().fg(Color::Yellow),
        )));
    }
    if let Some(r) = app.selected_catalog() {
        let latest = pa::latest_of(r, core);
        lines.push(Line::from(Span::styled(r.display_name(), heading())));
        lines.push(Line::from(r.description.clone()));
        let mut facts = vec![r.id.clone()];
        if let Some(v) = latest {
            facts.push(format!("version {v}"));
            if let Some(s) = r.size_of(v) {
                facts.push(s);
            }
        }
        facts.push(r.kind(latest));
        if let Some(newest) = r
            .versions
            .iter()
            .max_by(|a, b| pa::compare_versions(&a.version, &b.version))
            && let Some(min) = pa::needs_core(newest, core)
        {
            facts.push(format!("{} needs homeCore {min}", newest.version));
        }
        lines.push(Line::from(Span::styled(facts.join(" · "), dim())));
        if app.can("plugins:write") {
            lines.push(Line::from(Span::styled(
                "i install",
                Style::default().fg(Color::Cyan),
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        parts[1],
    );
}

// ── Runtimes ────────────────────────────────────────────────────────────────

fn draw_runtimes(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL);
    match &app.plugins_tab.manage.runtimes {
        Load::Failed(e) => {
            frame.render_widget(
                Paragraph::new(format!("  {e}"))
                    .style(Style::default().fg(Color::Yellow))
                    .block(block),
                area,
            );
            return;
        }
        Load::Ready(_) => {}
        _ => {
            frame.render_widget(Paragraph::new("  Loading...").block(block), area);
            return;
        }
    }
    if let Some(r) = app.detail_runtime() {
        draw_runtime_detail(frame, app, r, area);
        return;
    }
    let now = chrono::Utc::now();
    let placements = &app.plugins_tab.manage.placements;
    let rows: Vec<Row> = app
        .runtime_list()
        .iter()
        .map(|r| {
            let (status, tone) = pa::runtime_status(r, now);
            let n = pa::placed_on(placements, &r.runtime_id).len();
            Row::new(vec![
                Cell::from(format!(
                    "  {}",
                    if r.name.is_empty() {
                        &r.runtime_id
                    } else {
                        &r.name
                    }
                )),
                Cell::from(r.kind.clone()).style(dim()),
                Cell::from(status).style(Style::default().fg(tone_color(tone))),
                Cell::from(r.host_version.clone().unwrap_or_else(|| "-".into())),
                Cell::from(format!(
                    "{n}{}",
                    r.max_plugins
                        .map(|m| format!(" of {m}"))
                        .unwrap_or_default()
                )),
                Cell::from(r.hostname.clone().unwrap_or_default()).style(dim()),
            ])
        })
        .collect();
    let header = Row::new(
        ["  Runtime", "Kind", "Status", "Host", "Plugins", "Machine"]
            .iter()
            .map(|h| Cell::from(*h).style(heading())),
    );
    let n = rows.len();
    let table = Table::new(
        rows,
        [
            Constraint::Length(22),
            Constraint::Length(8),
            Constraint::Length(26),
            Constraint::Length(8),
            Constraint::Length(9),
            Constraint::Min(8),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(if n == 0 {
        " No runtimes. Add one in the app. "
    } else {
        " Plugin runtimes "
    }))
    .row_highlight_style(highlight());
    let mut state = TableState::default();
    if n > 0 {
        state.select(Some(app.plugins_tab.manage.runtime_sel.index().min(n - 1)));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_runtime_detail(frame: &mut Frame<'_>, app: &App, r: &pa::Runtime, area: Rect) {
    let (status, tone) = pa::runtime_status(r, chrono::Utc::now());
    let opt = |v: &Option<String>| v.clone().unwrap_or_else(|| "-".into());
    let mut lines = vec![
        status_detail_row("Name", &r.name),
        status_detail_row("Id", &r.runtime_id),
        Line::from(vec![
            Span::styled(format!("  {:<16}", "Status"), dim()),
            Span::styled(status, Style::default().fg(tone_color(tone))),
        ]),
        status_detail_row("Kind", &format!("{} ({})", r.kind, r.abi)),
        status_detail_row("Host", &opt(&r.host_version)),
    ];
    if r.outdated {
        lines.push(status_detail_row(
            "Needs",
            &format!(
                "host {} or later: pull the newer image and restart it",
                opt(&r.min_host_version)
            ),
        ));
    }
    lines.extend([
        status_detail_row("Machine", &opt(&r.hostname)),
        status_detail_row("Arch", &opt(&r.arch)),
        status_detail_row("Network", &opt(&r.network_mode)),
        status_detail_row("Address", &opt(&r.source_ip)),
        status_detail_row(
            "Its page",
            &r.page_url
                .clone()
                .or_else(|| r.page_port.map(|p| format!("port {p}")))
                .unwrap_or_else(|| "-".into()),
        ),
        status_detail_row(
            "Last seen",
            &r.last_seen_at
                .as_deref()
                .map(age)
                .unwrap_or_else(|| "never".into()),
        ),
        status_detail_row("Added", &short_ts(&r.created_at)),
        status_detail_row("Connects to", &r.homecore_url),
        status_detail_row(
            "Holds",
            &r.max_plugins
                .map(|m| format!("up to {m} plugins"))
                .unwrap_or_else(|| "any number of plugins".into()),
        ),
        Line::from(""),
        Line::from(Span::styled("  Plugins on it", heading())),
    ]);
    let placed = pa::placed_on(&app.plugins_tab.manage.placements, &r.runtime_id);
    if placed.is_empty() {
        lines.push(Line::from(Span::styled("  None yet", dim())));
    }
    for p in placed {
        let running = app
            .plugins
            .iter()
            .find(|x| x.plugin_id == p.plugin_id)
            .and_then(pa::version_of);
        let note = match running {
            Some(v) if v != p.version => format!("  (running {v}, {} arriving)", p.version),
            _ => String::new(),
        };
        lines.push(Line::from(format!("  {} {}{note}", p.plugin_id, p.version)));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", r.name)),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

// ── Boxes ───────────────────────────────────────────────────────────────────

/// The keys of an open Plugins box, for the footer.
pub fn modal_hints(app: &App) -> Option<Vec<&'static str>> {
    let m = &app.plugins_tab.manage;
    if m.issued.is_some() {
        return Some(vec!["Enter or Esc close"]);
    }
    if m.chooser.is_some() {
        return Some(vec!["j/k move", "Enter install there", "Esc cancel"]);
    }
    if app.plugins_tab.params.is_some() {
        return Some(vec![
            "Tab next",
            "type a value",
            "Space change a choice",
            "Enter run",
            "Esc cancel",
        ]);
    }
    if app.settings_shown()
        && app
            .plugins_tab
            .detail
            .as_ref()
            .is_some_and(|d| d.settings.input.is_some())
    {
        return Some(vec![
            "type a value",
            "Enter keep",
            "Ctrl+S save",
            "Esc cancel",
        ]);
    }
    None
}

pub fn draw_plugin_overlays(frame: &mut Frame<'_>, app: &App) {
    if let Some(p) = &app.plugins_tab.params {
        draw_params(frame, p);
    }
    if let Some(c) = &app.plugins_tab.manage.chooser {
        let popup = centered_rect(50, 40, frame.area());
        frame.render_widget(Clear, popup);
        let items: Vec<ListItem> = c
            .options
            .iter()
            .map(|id| {
                let name = app
                    .runtime_list()
                    .iter()
                    .find(|r| &r.runtime_id == id)
                    .map(|r| format!("{} ({id})", r.name))
                    .unwrap_or_else(|| id.clone());
                ListItem::new(format!("  {name}"))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Which runtime runs {}? ", c.name)),
            )
            .highlight_style(highlight());
        let mut state = ListState::default();
        state.select(Some(c.sel));
        frame.render_stateful_widget(list, popup, &mut state);
    }
    if let Some(k) = &app.plugins_tab.manage.issued {
        let popup = centered_rect(70, 40, frame.area());
        frame.render_widget(Clear, popup);
        let lines = vec![
            Line::from(Span::styled(
                format!("The new key for {}", k.runtime.name),
                heading(),
            )),
            Line::from(""),
            Line::from(Span::styled(
                k.key.clone(),
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from("Paste it into the runtime's page. It is shown once: after"),
            Line::from("this box closes, Reset login makes a new one."),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Connection key ")
                        .border_style(Style::default().fg(Color::Yellow)),
                )
                .wrap(Wrap { trim: false })
                .alignment(Alignment::Left),
            popup,
        );
    }
}

fn draw_params(frame: &mut Frame<'_>, p: &crate::app::plugin_settings::ParamForm) {
    let popup = centered_rect(70, 60, frame.area());
    frame.render_widget(Clear, popup);
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", p.action.display_label()))
        .border_style(Style::default().fg(if p.action.danger {
            Color::Red
        } else {
            Color::Cyan
        }));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);
    let parts = split(
        inner,
        Direction::Vertical,
        vec![Constraint::Min(3), Constraint::Length(2)],
    );
    let sel = p.row();
    let mut lines: Vec<Line> = Vec::new();
    if let Some(d) = &p.action.description {
        lines.push(Line::from(Span::styled(d.clone(), dim())));
        lines.push(Line::from(""));
    }
    for i in p.form.selectable() {
        let Some(f) = p.form.field_at(i) else {
            continue;
        };
        let on = Some(i) == sel;
        let value = if on && p.form.typed(i) {
            let shown = if f.control == Control::Secret {
                mask(&p.buf, "•")
            } else {
                p.buf.clone()
            };
            format!("{shown}█")
        } else {
            let (v, _) = describe(f);
            match f.control {
                Control::Enum(_) => format!("‹ {v} ›"),
                _ => v,
            }
        };
        let req = if f.required { " *" } else { "" };
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "{} {:<26}",
                    if on { ">" } else { " " },
                    format!("{}{req}", f.label)
                ),
                if on {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ),
            Span::raw(value),
        ]));
        if on && let Some(h) = &f.help {
            lines.push(Line::from(Span::styled(format!("  {h}"), dim())));
        }
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), parts[0]);
    let foot = match &p.error {
        Some(e) => Line::from(Span::styled(e.clone(), Style::default().fg(Color::Red))),
        None if p.action.danger => Line::from(Span::styled(
            "Enter asks before it runs",
            Style::default().fg(Color::Red),
        )),
        None => Line::from(Span::styled("Enter run · Esc cancel", dim())),
    };
    frame.render_widget(Paragraph::new(foot), parts[1]);
}
