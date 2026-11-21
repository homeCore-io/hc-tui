//! People (its tabs), Data & backups, System, and the Settings dialogs.

use super::widgets::{centered_rect, format_uptime, highlight, input, short_ts};
use crate::app::manage::backup::{DataRow, DataSection};
use crate::app::manage::people::PeopleTab;
use crate::app::manage::settings_modal::{
    NewKeyField, PathField, PathPurpose, RestoreSource, SettingsModal,
};
use crate::app::{App, Load};
use crate::model::settings::{client_of, group_connections, read_log_level, size};
use crate::text::{take_chars, truncate};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};

fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn heading(t: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(
        t.into(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))
}

fn list<'a>(items: Vec<ListItem<'a>>, title: String) -> List<'a> {
    List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(highlight())
        .highlight_symbol(">> ")
}

fn render(frame: &mut Frame<'_>, l: List<'_>, selected: Option<usize>, area: Rect) {
    let mut state = ListState::default();
    state.select(selected);
    frame.render_stateful_widget(l, area, &mut state);
}

/// A loading or failed placeholder for a list that is not ready.
fn placeholder<T>(load: &Load<T>, what: &str) -> Option<String> {
    match load {
        Load::Ready(_) => None,
        Load::Failed(e) => Some(format!("Could not load {what}: {e}")),
        Load::Loading | Load::Idle => Some("Loading...".to_string()),
    }
}

fn message(frame: &mut Frame<'_>, text: String, title: &str, area: Rect) {
    frame.render_widget(
        Paragraph::new(text)
            .style(dim())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title.to_string()),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

/// `2026-11-17 21:02` in local time (or UTC with `T`).
fn when(app: &App, ts: &str) -> String {
    use chrono::{DateTime, Local, Utc};
    match DateTime::parse_from_rfc3339(ts) {
        Ok(dt) if app.time_utc => dt.with_timezone(&Utc).format("%Y-%m-%d %H:%M").to_string(),
        Ok(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        Err(_) => short_ts(ts),
    }
}

// ── People ──────────────────────────────────────────────────────────────────

pub fn draw_people(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let tab = app.manage.people.tab;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    let titles: Vec<Line> = PeopleTab::ALL
        .iter()
        .map(|t| Line::from(t.title()))
        .collect();
    let idx = PeopleTab::ALL.iter().position(|t| *t == tab).unwrap_or(0);
    frame.render_widget(
        Tabs::new(titles)
            .select(idx)
            .style(Style::default().fg(Color::Gray))
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
            .divider(" · "),
        rows[0],
    );
    match tab {
        PeopleTab::People => draw_users(frame, app, rows[1]),
        PeopleTab::Roles => draw_roles(frame, app, rows[1]),
        PeopleTab::ApiKeys => draw_api_keys(frame, app, rows[1]),
        PeopleTab::Sessions => draw_sessions(frame, app, rows[1]),
        PeopleTab::Displays => draw_displays(frame, app, rows[1]),
    }
}

fn draw_users(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let me = app.current_user.as_ref().map(|u| u.id.as_str());
    let items: Vec<ListItem> = app
        .views
        .users
        .iter()
        .filter_map(|&i| app.users.get(i))
        .map(|u| {
            let tag = if me == Some(u.id.as_str()) {
                " (you)"
            } else {
                ""
            };
            let pin = if u.has_pin { "PIN" } else { "" };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<24}", take_chars(&format!("{}{tag}", u.username), 24)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{:<18}", u.role.label()),
                    Style::default().fg(if u.role.is_admin() {
                        Color::Yellow
                    } else {
                        Color::Gray
                    }),
                ),
                Span::styled(format!("{pin:<5}"), Style::default().fg(Color::Cyan)),
                Span::styled(take_chars(&u.created_at, 10), dim()),
            ]))
        })
        .collect();
    let n = items.len();
    let sel = (n > 0).then(|| app.manage.users_sel.index().min(n - 1));
    render(frame, list(items, format!("People ({n})")), sel, area);
}

fn draw_roles(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let p = &app.manage.people;
    if let Some(msg) = placeholder(&p.roles, "roles") {
        return message(frame, msg, "Roles", area);
    }
    let roles = p.roles.ready().map(Vec::as_slice).unwrap_or_default();
    let width = area.width.saturating_sub(6) as usize;
    let items: Vec<ListItem> = roles
        .iter()
        .map(|r| {
            let label = crate::text::normalize_label(&r.role);
            let mut lines = vec![Line::from(Span::styled(
                label,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ))];
            // Scopes wrapped under the role, by words.
            let mut cur = String::from("  ");
            for s in &r.scopes {
                if cur.len() + s.len() + 1 > width.max(20) && !cur.trim().is_empty() {
                    lines.push(Line::from(Span::styled(
                        std::mem::replace(&mut cur, String::from("  ")),
                        dim(),
                    )));
                }
                cur.push_str(s);
                cur.push(' ');
            }
            if cur.trim().is_empty() {
                cur.push_str("no scopes");
            }
            lines.push(Line::from(Span::styled(cur, dim())));
            ListItem::new(lines)
        })
        .collect();
    let n = items.len();
    let sel = (n > 0).then(|| p.roles_sel.index().min(n - 1));
    render(
        frame,
        list(items, format!("Roles ({n}), what each may do")),
        sel,
        area,
    );
}

fn draw_api_keys(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let p = &app.manage.people;
    if let Some(msg) = placeholder(&p.keys, "API keys") {
        return message(frame, msg, "API keys", area);
    }
    let now = chrono::Utc::now();
    let keys = app.visible_api_keys();
    let items: Vec<ListItem> = keys
        .iter()
        .map(|k| {
            let (state, color) = if k.is_revoked() {
                ("revoked", Color::Red)
            } else if k.is_expired(&now) {
                ("expired", Color::Red)
            } else if k.display {
                ("display", Color::Cyan)
            } else if k.plugin.is_some() {
                ("plugin", Color::Cyan)
            } else {
                ("in use", Color::Green)
            };
            let used = k
                .last_used_at
                .as_deref()
                .map(|t| when(app, t))
                .unwrap_or_else(|| "never".into());
            let style = if k.is_revoked() {
                dim()
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(format!("{:<27}", truncate(&k.label, 26)), style),
                    Span::styled(format!("{state:<9}"), Style::default().fg(color)),
                    Span::styled(format!("hc_sk_{}...", take_chars(&k.prefix, 8)), dim()),
                ]),
                Line::from(Span::styled(
                    format!(
                        "  made{} {} · last used {used}{}",
                        app.users
                            .iter()
                            .find(|u| u.id == k.owner_uid)
                            .map(|u| format!(" by {}", u.username))
                            .unwrap_or_default(),
                        when(app, &k.created_at),
                        k.revoked_at
                            .as_deref()
                            .map(|t| format!(" · revoked {}", when(app, t)))
                            .unwrap_or_default()
                    ),
                    dim(),
                )),
                Line::from(Span::styled(format!("  can {}", k.scopes.join(" ")), dim())),
            ])
        })
        .collect();
    let n = items.len();
    let sel = (n > 0).then(|| p.keys_sel.index().min(n - 1));
    let title = if n == 0 {
        "API keys (none yet; n makes one)".to_string()
    } else {
        format!("API keys ({n})")
    };
    render(frame, list(items, title), sel, area);
}

fn draw_displays(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let p = &app.manage.people;
    if let Some(msg) = placeholder(&p.displays, "displays") {
        return message(frame, msg, "Displays", area);
    }
    let list_ = p.displays.ready().map(Vec::as_slice).unwrap_or_default();
    if list_.is_empty() {
        return message(
            frame,
            "No displays. n makes one: a key for a hall tablet or kitchen panel, which may use the house and change nothing.".to_string(),
            "Displays",
            area,
        );
    }
    let items: Vec<ListItem> = list_
        .iter()
        .map(|d| {
            ListItem::new(vec![
                Line::from(Span::styled(
                    format!("{:<30}", truncate(&d.name, 30)),
                    Style::default().fg(Color::White),
                )),
                Line::from(Span::styled(
                    match d.last_used_at.as_deref() {
                        Some(t) => format!("  last used {}", when(app, t)),
                        None => "  never used yet".to_string(),
                    },
                    dim(),
                )),
            ])
        })
        .collect();
    let n = items.len();
    let sel = (n > 0).then(|| p.displays_sel.index().min(n - 1));
    let mut state = ratatui::widgets::ListState::default();
    state.select(sel);
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Displays"))
            .highlight_style(highlight()),
        area,
        &mut state,
    );
}

fn draw_sessions(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let p = &app.manage.people;
    if let Some(msg) = placeholder(&p.sessions, "sessions") {
        return message(frame, msg, "Sessions", area);
    }
    let list_ = p.sessions.ready().map(Vec::as_slice).unwrap_or_default();
    let items: Vec<ListItem> = list_
        .iter()
        .map(|s| {
            let this = if s.current { "  this one" } else { "" };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        format!("{:<26}", truncate(&client_of(s.user_agent.as_deref()), 26)),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(this, Style::default().fg(Color::Green)),
                ]),
                Line::from(Span::styled(
                    format!(
                        "  since {} · last active {} · ends {}",
                        s.started_at
                            .as_deref()
                            .map(|t| when(app, t))
                            .unwrap_or_default(),
                        s.last_active
                            .as_deref()
                            .map(|t| when(app, t))
                            .unwrap_or_default(),
                        s.expires_at
                            .as_deref()
                            .map(|t| when(app, t))
                            .unwrap_or_default(),
                    ),
                    dim(),
                )),
            ])
        })
        .collect();
    let n = items.len();
    let sel = (n > 0).then(|| p.sessions_sel.index().min(n - 1));
    render(
        frame,
        list(items, format!("Your sessions ({n})")),
        sel,
        area,
    );
}

// ── Data & backups ──────────────────────────────────────────────────────────

fn section_title(app: &App, s: DataSection) -> String {
    match s {
        DataSection::Backups => "Backups".to_string(),
        DataSection::Kept => match app.manage.backup.kept.ready() {
            Some(k) if k.every_hours > 0 => format!(
                "Kept on the house: one every {} h, the last {}{}",
                k.every_hours,
                k.keep,
                k.dir
                    .as_deref()
                    .map(|d| format!(", in {d}"))
                    .unwrap_or_default()
            ),
            _ => "Kept on the house".to_string(),
        },
        DataSection::Transfer => "Automations and scenes".to_string(),
        DataSection::Calendars => "Calendars".to_string(),
    }
}

fn data_row_line(app: &App, row: &DataRow) -> Line<'static> {
    match row {
        DataRow::Action(a) => Line::from(Span::styled(
            format!("  {}", a.label()),
            Style::default().fg(Color::White),
        )),
        DataRow::Kept(name) => {
            let b = app.kept_backup(name);
            Line::from(vec![
                Span::styled(
                    format!(
                        "  {:<18}",
                        b.map(|b| when(app, &b.created_at)).unwrap_or_default()
                    ),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{:>9}  ", b.map(|b| size(b.bytes)).unwrap_or_default()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(name.clone(), dim()),
            ])
        }
        DataRow::Calendar(id) => {
            let c = app
                .manage
                .backup
                .calendars
                .ready()
                .and_then(|cs| cs.iter().find(|c| &c.id == id));
            let detail = c
                .map(|c| {
                    let from = match (&c.source_url, c.refresh_hours) {
                        (Some(_), Some(h)) => format!(" · fetched every {h} h"),
                        (Some(_), None) => " · fetched".to_string(),
                        _ => " · uploaded".to_string(),
                    };
                    format!(
                        "{} events, {} to come{from}",
                        c.event_count, c.upcoming_count
                    )
                })
                .unwrap_or_default();
            Line::from(vec![
                Span::styled(
                    format!("  {:<24}", take_chars(id, 24)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(detail, dim()),
            ])
        }
    }
}

pub fn draw_data(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let b = &app.manage.backup;
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(area);
    let rows = app.data_rows();
    // With no kept backups to list, their place says why.
    let kept_note = match &b.kept {
        Load::Ready(k) if k.backups.is_empty() => Some(if k.every_hours == 0 {
            "  None kept. homecore.toml's [backup] can keep some on a schedule.".to_string()
        } else {
            "  None kept yet.".to_string()
        }),
        Load::Failed(e) => Some(format!("  Could not list kept backups: {e}")),
        Load::Loading | Load::Idle => Some("  Loading...".to_string()),
        Load::Ready(_) => None,
    };
    let note = |items: &mut Vec<ListItem>| {
        if let Some(n) = &kept_note {
            items.push(ListItem::new(""));
            items.push(ListItem::new(heading(section_title(
                app,
                DataSection::Kept,
            ))));
            items.push(ListItem::new(Span::styled(n.clone(), dim())));
        }
    };
    let mut items: Vec<ListItem> = Vec::new();
    let mut selected = None;
    let mut last: Option<DataSection> = None;
    for (i, (section, row)) in rows.iter().enumerate() {
        if last != Some(*section) {
            if last == Some(DataSection::Backups) && *section != DataSection::Kept {
                note(&mut items);
            }
            if last.is_some() {
                items.push(ListItem::new(""));
            }
            items.push(ListItem::new(heading(section_title(app, *section))));
            last = Some(*section);
        }
        if i == b.sel.index() {
            selected = Some(items.len());
        }
        items.push(ListItem::new(data_row_line(app, row)));
    }
    if last == Some(DataSection::Backups) {
        note(&mut items);
    }
    let title = if b.busy {
        "Data & backups (working)"
    } else {
        "Data & backups"
    };
    render(frame, list(items, title.to_string()), selected, layout[0]);

    let text = if b.status.is_empty() {
        format!(
            "Downloads and exports go to {}, readable only by you.",
            crate::app::manage::backup::tilde(&crate::config::exports_dir())
        )
    } else {
        b.status.clone()
    };
    let color = if b.status.starts_with("Error") {
        Color::Red
    } else if b.status.is_empty() {
        Color::DarkGray
    } else {
        Color::Yellow
    };
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(color))
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        layout[1],
    );
}

// ── System ──────────────────────────────────────────────────────────────────

fn row(label: &str, spans: Vec<Span<'static>>) -> Line<'static> {
    let mut all = vec![Span::styled(format!(" {label:<13}"), dim())];
    all.extend(spans);
    Line::from(all)
}

fn plain(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(Color::White))
}

pub fn draw_system(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let st = &app.manage.status;
    let title = if let Some(r) = &st.restarting {
        format!("System ({r})")
    } else if st.loading {
        "System (loading)".to_string()
    } else {
        "System".to_string()
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    // Health.
    let mut health = match &st.health {
        Load::Ready(h) if h.ok() => vec![Span::styled("● ok", Style::default().fg(Color::Green))],
        Load::Ready(h) => vec![Span::styled(
            format!("● {}", h.status),
            Style::default().fg(Color::Red),
        )],
        Load::Failed(e) => vec![Span::styled(
            format!("● not answering ({})", truncate(e, 40)),
            Style::default().fg(Color::Red),
        )],
        _ => vec![Span::styled("...", dim())],
    };
    let live = if app.ws_connected {
        Span::styled(" · live updates ●", Style::default().fg(Color::Green))
    } else {
        Span::styled(" · live updates ○", Style::default().fg(Color::Red))
    };
    health.push(live);
    if !app.plugins.is_empty() {
        let p = app.plugin_health();
        let mut t = format!(" · {} plugins running", p.running);
        if p.stopped > 0 {
            t.push_str(&format!(" · {} stopped", p.stopped));
        }
        if p.off > 0 {
            t.push_str(&format!(" · {} off", p.off));
        }
        let c = if p.stopped > 0 {
            Color::Yellow
        } else {
            Color::White
        };
        health.push(Span::styled(t, Style::default().fg(c)));
    }
    lines.push(row("Health", health));

    // Versions.
    let versions = match &st.versions {
        Load::Ready(v) if !v.is_empty() => v
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(" · "),
        Load::Failed(e) => format!("unknown ({})", truncate(e, 40)),
        _ => app
            .system_status
            .as_ref()
            .map(|s| format!("core {}", s.version))
            .unwrap_or_else(|| "...".into()),
    };
    lines.push(row("Versions", vec![plain(versions)]));

    // Uptime and counts.
    if let Some(s) = &app.system_status {
        lines.push(row(
            "Uptime",
            vec![
                plain(format!("{:<11}", format_uptime(s.uptime_seconds))),
                Span::styled("Devices ", dim()),
                plain(format!("{:<6}", s.devices_total)),
                Span::styled("Automations ", dim()),
                plain(format!("{} ({} on)  ", s.rules_total, s.rules_enabled)),
                Span::styled("Database ", dim()),
                plain(size(s.state_db_bytes + s.history_db_bytes)),
                Span::styled(
                    format!(
                        " (history {}, the rest {})",
                        size(s.history_db_bytes),
                        size(s.state_db_bytes)
                    ),
                    dim(),
                ),
            ],
        ));
        lines.push(row(
            "Started",
            vec![
                plain(when(app, &s.started_at)),
                Span::styled(
                    s.timezone
                        .as_deref()
                        .map(|z| format!("  ({z})"))
                        .unwrap_or_default(),
                    dim(),
                ),
            ],
        ));
        lines.push(row(
            "Last backup",
            vec![plain(
                s.last_backup_at
                    .as_deref()
                    .map(|t| when(app, t))
                    .unwrap_or_else(|| "none recorded".into()),
            )],
        ));
    } else if let Some(e) = &st.error {
        lines.push(row(
            "Status",
            vec![Span::styled(
                format!("could not load: {}", truncate(e, 60)),
                Style::default().fg(Color::Red),
            )],
        ));
    }

    // Temperatures.
    let unit = match &st.house_temp {
        Some(t) if t.chosen => format!("{} for the household", t.temperature_unit),
        Some(t) => format!(
            "{} (nobody has chosen one for the household)",
            t.temperature_unit
        ),
        None => app.temp_unit.symbol().to_string(),
    };
    lines.push(row("Temperature", vec![plain(unit)]));
    if let Some(b) = &st.battery {
        let pct = |k: &str| b.get(k).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        lines.push(row(
            "Low battery",
            vec![plain(format!(
                "under {}%, cleared {}% above it (set in homecore.toml)",
                pct("threshold_pct"),
                pct("recover_band_pct")
            ))],
        ));
    }

    // Core's log level.
    let level = match &st.log_level {
        Load::Ready(d) => {
            let r = read_log_level(d);
            let mut spans = vec![Span::styled(
                format!("‹ {} ›", r.level.as_deref().unwrap_or("?")),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )];
            if !r.others.is_empty() {
                spans.push(Span::styled(
                    format!("  set apart: {}", r.others.join(", ")),
                    dim(),
                ));
            }
            if app.can_set_log_level() {
                spans.push(Span::styled("  ←/→ change, until homeCore restarts", dim()));
            }
            spans
        }
        Load::Failed(e) => vec![Span::styled(
            format!("unknown ({})", truncate(e, 50)),
            dim(),
        )],
        _ => vec![Span::styled("...", dim())],
    };
    lines.push(row("Core log", level));

    // Who is connected.
    if app.is_admin() {
        match &st.connections {
            Load::Ready(c) => {
                let groups = group_connections(c);
                if groups.is_empty() {
                    lines.push(row("Connected", vec![plain("nobody")]));
                }
                for (i, g) in groups.iter().enumerate() {
                    let label = if i == 0 { "Connected" } else { "" };
                    let n = if g.count > 1 {
                        format!(" ×{}", g.count)
                    } else {
                        String::new()
                    };
                    lines.push(row(
                        label,
                        vec![
                            plain(format!("{} watching {}", g.user, g.watching)),
                            Span::styled(
                                format!(" · {}{n} · since {}", g.client, when(app, &g.since)),
                                dim(),
                            ),
                        ],
                    ));
                }
            }
            Load::Failed(e) => lines.push(row(
                "Connected",
                vec![Span::styled(
                    format!("unknown ({})", truncate(e, 50)),
                    dim(),
                )],
            )),
            _ => lines.push(row("Connected", vec![Span::styled("...", dim())])),
        }
    }
    if app.is_admin() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " R restarts homeCore. It asks first.",
            dim(),
        )));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

// ── Dialogs ─────────────────────────────────────────────────────────────────

/// The footer while a dialog is open.
pub fn modal_hints(m: &SettingsModal) -> Vec<&'static str> {
    match m {
        SettingsModal::DisplayName(_) => vec!["type its name", "Enter make it", "Esc cancel"],
        SettingsModal::NewKey(_) => vec![
            "type a label",
            "Tab label or scopes",
            "↑/↓ scope",
            "Space tick",
            "Enter make",
            "Esc cancel",
        ],
        SettingsModal::Secret(_) => vec!["Enter or Esc done (it won't be shown again)"],
        SettingsModal::Restore(_) => vec!["type the date", "Enter restore", "Esc cancel"],
        SettingsModal::Path(p) if p.purpose == PathPurpose::Calendar => {
            vec!["type a path", "Tab path or name", "Enter add", "Esc cancel"]
        }
        SettingsModal::Path(p) if p.purpose == PathPurpose::NewRuntime => {
            vec!["Tab address or name", "Enter add", "Esc cancel"]
        }
        SettingsModal::Path(p) if p.purpose == PathPurpose::CalendarUrl => {
            vec![
                "type its address",
                "Tab address or name",
                "Enter add",
                "Esc cancel",
            ]
        }
        SettingsModal::Path(_) => vec!["type a path", "Enter check it", "Esc cancel"],
    }
}

fn dialog(frame: &mut Frame<'_>, px: u16, py: u16, title: &str, color: Color) -> Rect {
    let popup = centered_rect(px, py, frame.area());
    frame.render_widget(Clear, popup);
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title.to_string())
        .border_style(Style::default().fg(color));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);
    inner
}

fn error_line(e: &Option<String>) -> Line<'static> {
    match e {
        Some(e) => Line::from(Span::styled(e.clone(), Style::default().fg(Color::Red))),
        None => Line::from(""),
    }
}

pub fn draw_modal(frame: &mut Frame<'_>, app: &App) {
    let Some(m) = &app.manage.modal else {
        return;
    };
    match m {
        SettingsModal::DisplayName(f) => {
            let inner = dialog(frame, 60, 35, "New display", Color::Cyan);
            let r = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),
                    Constraint::Length(3),
                    Constraint::Min(1),
                ])
                .split(inner);
            frame.render_widget(
                Paragraph::new("A screen for the house. Its key may use the house and change nothing; it is shown once.")
                    .style(dim())
                    .wrap(Wrap { trim: true }),
                r[0],
            );
            frame.render_widget(input(&f.name, "Name", true), r[1]);
            let note = match (&f.error, f.busy) {
                (Some(e), _) => Span::styled(e.clone(), Style::default().fg(Color::Red)),
                (None, true) => Span::styled("making it...", dim()),
                _ => Span::raw(""),
            };
            frame.render_widget(Paragraph::new(Line::from(note)), r[2]);
        }
        SettingsModal::NewKey(f) => {
            let title = if f.editing.is_some() {
                "Edit API key"
            } else {
                "New API key"
            };
            let inner = dialog(frame, 70, 70, title, Color::Cyan);
            let r = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(3),
                    Constraint::Length(1),
                ])
                .split(inner);
            frame.render_widget(
                input(
                    &f.label,
                    "What it's for (a label)",
                    f.field == NewKeyField::Label,
                ),
                r[0],
            );
            let items: Vec<ListItem> = f
                .scopes
                .iter()
                .map(|(s, on)| ListItem::new(format!("[{}] {s}", if *on { "x" } else { " " })))
                .collect();
            let focused = f.field == NewKeyField::Scopes;
            let title = if f.scopes.is_empty() {
                "What it may do (loading your scopes...)".to_string()
            } else {
                format!("What it may do ({} ticked)", f.chosen().len())
            };
            let l = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .border_style(super::widgets::focus_style(focused)),
                )
                .highlight_style(if focused {
                    highlight()
                } else {
                    Style::default()
                });
            let sel = (!f.scopes.is_empty()).then_some(f.cursor);
            render(frame, l, sel, r[1]);
            let bottom = if f.busy {
                Line::from(Span::styled("Making it...", dim()))
            } else {
                error_line(&f.error)
            };
            frame.render_widget(Paragraph::new(bottom), r[2]);
        }
        SettingsModal::Secret(k) => {
            let inner = dialog(frame, 70, 40, "Your new key", Color::Yellow);
            let lines = vec![
                Line::from(Span::styled(
                    format!("\"{}\"", k.label),
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    k.token.clone(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Copy it now. It won't be shown again, and hc-tui doesn't keep it.",
                    Style::default().fg(Color::White),
                )),
                Line::from(Span::styled(
                    "Put it where the program reads it. Enter or Esc when done.",
                    dim(),
                )),
            ];
            frame.render_widget(
                Paragraph::new(lines)
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: false }),
                inner,
            );
        }
        SettingsModal::Restore(f) => {
            let inner = dialog(
                frame,
                70,
                70,
                "Put the house back as this backup has it?",
                Color::Red,
            );
            let since = f
                .plan
                .taken_at
                .as_deref()
                .map(|t| when(app, t))
                .unwrap_or_else(|| "the backup was taken".to_string());
            let mut lines = vec![
                Line::from(Span::styled(
                    truncate(&f.source.label(), inner.width as usize),
                    dim(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!("Everything changed since {since} is lost."),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "homeCore puts it in place when it next starts; you're asked about restarting next.",
                    Style::default().fg(Color::White),
                )),
                Line::from(""),
                Line::from(Span::styled("It brings back:", dim())),
            ];
            for s in f.plan.summary() {
                lines.push(Line::from(format!("  {s}")));
            }
            if !f.plan.skipped.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("  Skipped: {}", f.plan.skipped.join(", ")),
                    dim(),
                )));
            }
            if matches!(f.source, RestoreSource::Kept(_)) {
                lines.push(Line::from(Span::styled(
                    "  Back up now first (n) if you might want what's there now.",
                    dim(),
                )));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("Type "),
                Span::styled(
                    f.date.clone(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" to restore: "),
                Span::styled(
                    format!("{}_", f.typed),
                    Style::default().fg(if f.confirmed() {
                        Color::Green
                    } else {
                        Color::White
                    }),
                ),
            ]));
            lines.push(if f.busy {
                Line::from(Span::styled("Handing the backup to homeCore...", dim()))
            } else {
                error_line(&f.error)
            });
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
        }
        SettingsModal::Path(f) => {
            let (title, what) = match f.purpose {
                PathPurpose::RestoreFile => (
                    "Restore from a file",
                    "A homeCore backup (.zip) on this computer. It's checked first; nothing changes yet.",
                ),
                PathPurpose::Calendar => (
                    "Add a calendar",
                    "A .ics file on this computer. A calendar with the same name is replaced (it asks).",
                ),
                PathPurpose::CalendarUrl => (
                    "Add a calendar from a web address",
                    "homeCore downloads the .ics at this address, keeps it, and fetches it again on its own.",
                ),
                PathPurpose::NewRuntime => (
                    "Add a plugin runtime",
                    "homeCore's address as the runtime reaches it (it goes into the key), and a name. The key is shown once.",
                ),
            };
            let inner = dialog(frame, 70, 45, title, Color::Cyan);
            let r = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Min(1),
                ])
                .split(inner);
            frame.render_widget(
                Paragraph::new(what).style(dim()).wrap(Wrap { trim: true }),
                r[0],
            );
            let path_label = if f.purpose == PathPurpose::CalendarUrl {
                "Address"
            } else {
                "Path"
            };
            frame.render_widget(input(&f.path, path_label, f.field == PathField::Path), r[1]);
            if matches!(
                f.purpose,
                PathPurpose::Calendar | PathPurpose::CalendarUrl | PathPurpose::NewRuntime
            ) {
                frame.render_widget(
                    input(
                        &f.name,
                        if f.purpose == PathPurpose::NewRuntime {
                            "Name"
                        } else {
                            "Name (empty takes the file's)"
                        },
                        f.field == PathField::Name,
                    ),
                    r[2],
                );
            }
            let bottom = if f.busy {
                Line::from(Span::styled("Reading it...", dim()))
            } else {
                error_line(&f.error)
            };
            frame.render_widget(Paragraph::new(bottom).wrap(Wrap { trim: true }), r[3]);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::manage::settings_modal::{
        NewKeyForm, PathForm, PathPurpose, RestoreForm, RestoreSource, SettingsModal,
    };
    use crate::app::test_support::*;
    use crate::app::{App, Load};
    use crate::model::Role;
    use crate::model::settings::{Health, KeptBackups, NewApiKey, RestorePlan};
    use crate::net::http::parse_body;
    use crate::screens::ScreenId;
    use ratatui::{Terminal, backend::TestBackend};

    fn render(app: &mut App, w: u16, h: u16) -> String {
        app.refresh_views();
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| crate::ui::draw(f, app)).unwrap();
        let buf = t.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_secret_dialog_says_it_is_shown_once() {
        let mut a = signed_in(Role::Admin);
        let k: NewApiKey = serde_json::from_value(serde_json::json!({
            "id": "1", "label": "nas", "token": "hc_sk_shownonce"
        }))
        .unwrap();
        a.manage.modal = Some(SettingsModal::Secret(k));
        let s = render(&mut a, 140, 40);
        assert!(s.contains("hc_sk_shownonce"), "{s}");
        assert!(s.contains("won't be shown again"), "{s}");
        a.manage.modal = None;
        let s = render(&mut a, 140, 40);
        assert!(!s.contains("hc_sk_shownonce"));
    }

    #[test]
    fn kept_backups_and_the_restore_dialog_draw() {
        let mut a = signed_in(Role::Admin);
        a.screen = ScreenId::Data;
        a.manage.backup.kept = Load::Ready(
            parse_body::<KeptBackups>(
                "/system/backups",
                include_str!("../../tests/fixtures/system_backups.json"),
            )
            .unwrap(),
        );
        let s = render(&mut a, 160, 40);
        assert!(s.contains("homecore-backup-20261117T210247Z.zip"), "{s}");
        assert!(s.contains("Kept on the house: one every 24 h"), "{s}");
        a.manage.modal = Some(SettingsModal::Restore(RestoreForm::new(
            RestoreSource::Kept("homecore-backup-20261117T210247Z.zip".into()),
            RestorePlan::default(),
            "2026-11-17".into(),
        )));
        let s = render(&mut a, 160, 40);
        assert!(s.contains("Type 2026-11-17 to restore"), "{s}");
    }

    #[test]
    fn every_dialog_draws_at_odd_sizes() {
        let mut a = signed_in(Role::Admin);
        let modals: Vec<fn() -> SettingsModal> = vec![
            || SettingsModal::NewKey(NewKeyForm::new(vec!["devices:read".into()])),
            || {
                SettingsModal::Secret(
                    serde_json::from_value(serde_json::json!({"token": "t"})).unwrap(),
                )
            },
            || {
                SettingsModal::Restore(RestoreForm::new(
                    RestoreSource::File("/tmp/x.zip".into()),
                    RestorePlan::default(),
                    "restore".into(),
                ))
            },
            || SettingsModal::Path(PathForm::new(PathPurpose::Calendar)),
            || SettingsModal::Path(PathForm::new(PathPurpose::RestoreFile)),
        ];
        for m in modals {
            a.manage.modal = Some(m());
            for (w, h) in [(20u16, 5u16), (40, 10), (200, 60), (80, 3)] {
                let _ = render(&mut a, w, h);
            }
        }
    }

    #[test]
    fn system_shows_health_versions_and_the_log_level() {
        let mut a = signed_in(Role::Admin);
        a.screen = ScreenId::System;
        a.manage.status.health = Load::Ready(Health {
            status: "ok".into(),
            version: "0.1.116".into(),
        });
        a.manage.status.versions = Load::Ready(vec![
            ("core".into(), "0.1.116".into()),
            ("app".into(), "0.1.82".into()),
        ]);
        a.manage.status.log_level = Load::Ready("info,rumqttc::state=info,rumqttd=info".into());
        let s = render(&mut a, 160, 40);
        assert!(s.contains("● ok"), "{s}");
        assert!(s.contains("core 0.1.116 · app 0.1.82"), "{s}");
        assert!(s.contains("‹ info ›"), "{s}");
        assert!(s.contains("R restart core"), "{s}");
    }
}
