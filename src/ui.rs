use crate::app::{App, AreaEditor, DeviceEditField, DeviceViewMode, FocusField, LoginPhase, PluginDetailPanel, Tab, UserEditField, UserEditMode, UserEditor};
use crate::api::DeviceState;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, Gauge, List, ListItem, Paragraph, Tabs, Wrap,
    },
    Frame,
};

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    if !app.authenticated {
        draw_login(frame, app);
        return;
    }

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let tabs = app
        .tabs()
        .iter()
        .map(|tab| Line::from(tab.title()))
        .collect::<Vec<_>>();
    let tabs_widget = Tabs::new(tabs)
        .select(app.tab)
        .block(Block::default().borders(Borders::ALL).title("HomeCore"))
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs_widget, layout[0]);

    draw_tab_body(frame, app, layout[1]);

    draw_status_bar(frame, app, layout[2]);

    if app.device_editor.is_some() {
        draw_device_editor(frame, app);
    }
    if let Some(editor) = app.area_editor.as_ref() {
        draw_area_editor(frame, app, editor);
    }
    if let Some(editor) = app.user_editor.as_ref() {
        draw_user_editor(frame, app, editor);
    }
}

fn draw_status_bar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let role = app
        .current_user
        .as_ref()
        .map(|u| format!("{:?}", u.role))
        .unwrap_or_else(|| "unknown".to_string());
    let live = if app.ws_connected { "●" } else { "○" };
    let live_color = if app.ws_connected { Color::Green } else { Color::Red };

    let status_text = if let Some(err) = &app.error {
        format!("ERROR: {err}")
    } else {
        app.status.clone()
    };

    let user_str = app
        .current_user
        .as_ref()
        .map(|u| u.username.as_str())
        .unwrap_or("n/a");

    let mut hints = vec!["Tab/← → tab", "j/k move", "r refresh", "q quit"];
    match app.active_tab() {
        Tab::Devices => {
            hints.push("Spc toggle");
            hints.push("t on/off");
            hints.push("+/- bright");
            hints.push("l/u lock");
            hints.push("v view");
            hints.push("Enter edit");
            hints.push("d delete");
        }
        Tab::Scenes => { hints.push("a activate"); }
        Tab::Events => { hints.push("f filter"); }
        Tab::Areas => {
            hints.push("n new");
            hints.push("Enter rename");
            hints.push("d delete");
        }
        Tab::Users => {
            hints.push("n new");
            hints.push("Enter role");
            hints.push("p password");
            hints.push("d delete");
        }
        Tab::Plugins => { hints.push("d deregister"); }
        _ => {}
    }
    if app.device_editor.is_some() {
        hints = vec!["Tab field", "Enter save", "Esc cancel"];
    }
    if app.area_editor.is_some() {
        hints = vec!["Enter save", "Esc cancel"];
    }
    if app.user_editor.is_some() {
        hints = vec!["Tab field", "Space cycle role", "Enter save", "Esc cancel"];
    }
    if app.plugin_detail_open {
        hints = vec!["1/2/3 or ←/→ panel", "b discover bridges", "p pair bridges", "r refresh", "Esc close", "q quit"];
    }

    let hint_str = hints.join(" | ");
    let style = if app.error.is_some() {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    };

    let line = Line::from(vec![
        Span::styled(format!("{status_text} | {user_str} ({role}) "), style),
        Span::styled(live, Style::default().fg(live_color)),
        Span::styled(format!(" | {hint_str}"), style),
    ]);

    let footer = Paragraph::new(line)
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .wrap(Wrap { trim: true });
    frame.render_widget(footer, area);
}

fn draw_login(frame: &mut Frame<'_>, app: &App) {
    let popup = centered_rect(70, 55, frame.area());
    frame.render_widget(Clear, popup);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(popup);

    let title = Paragraph::new("HomeCore TUI Login")
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).title("Authenticate"));
    frame.render_widget(title, layout[0]);

    let username_style = if matches!(app.focus, FocusField::Username) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let username = Paragraph::new(app.username.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Username")
            .border_style(username_style),
    );
    frame.render_widget(username, layout[1]);

    let password_style = if matches!(app.focus, FocusField::Password) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let masked = "*".repeat(app.password.len());
    let password = Paragraph::new(masked).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Password")
            .border_style(password_style),
    );
    frame.render_widget(password, layout[2]);

    let help = Paragraph::new("Tab switch field | Enter login | Esc quit").alignment(Alignment::Center);
    frame.render_widget(help, layout[3]);

    let loading_label = if app.login_in_progress {
        match app.login_phase {
            LoginPhase::Authenticating => {
                format!("{} authenticating with HomeCore...", app.login_spinner())
            }
            LoginPhase::Synthesizing => {
                format!("{} Synthesizing homeCore...", app.login_spinner())
            }
        }
    } else {
        "Idle".to_string()
    };
    let loading = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(loading_label))
        .gauge_style(
            Style::default()
                .fg(Color::LightGreen)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .ratio(app.login_progress_ratio());
    frame.render_widget(loading, layout[4]);

    let message = app.error.clone().unwrap_or_else(|| {
        "Connects to /api/v1/auth/login and loads/saves cache snapshots locally".to_string()
    });
    let msg = Paragraph::new(message).alignment(Alignment::Center);
    frame.render_widget(msg, layout[5]);
}

fn draw_tab_body(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if matches!(app.active_tab(), Tab::Devices) {
        draw_dashboard(frame, app, area);
        return;
    }
    if matches!(app.active_tab(), Tab::Plugins) && app.plugin_detail_open {
        draw_plugin_detail(frame, app, area);
        return;
    }

    let items = match app.active_tab() {
        Tab::Devices => Vec::new(),
        Tab::Scenes => app
            .scenes
            .iter()
            .map(|s| ListItem::new(format!("{} ({})", s.name, s.id)))
            .collect::<Vec<_>>(),
        Tab::Areas => app
            .areas
            .iter()
            .map(|a| {
                let count = a.device_ids.len();
                let dev_label = if count == 1 { "device" } else { "devices" };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {:<28}", a.name),
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{count} {dev_label}"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect::<Vec<_>>(),
        Tab::Automations => app
            .automations
            .iter()
            .map(|r| {
                ListItem::new(format!(
                    "{} ({}) enabled={} priority={}",
                    r.name, r.id, r.enabled, r.priority
                ))
            })
            .collect::<Vec<_>>(),
        Tab::Events => app
            .filtered_events()
            .iter()
            .map(|e| {
                let mut extra = String::new();
                if let Some(device) = &e.device_id {
                    extra = format!(" device={device}");
                } else if let Some(rule) = &e.rule_name {
                    extra = format!(" rule={rule}");
                } else if let Some(custom) = &e.event_type_custom {
                    extra = format!(" event={custom}");
                }
                if let Some(detail) = &e.event_detail {
                    extra.push_str(&format!(" detail={detail}"));
                }

                let (tag, tag_color) = match e.event_type.as_str() {
                    "device_button" => ("BTN", Color::LightBlue),
                    "device_rotary" => ("ROT", Color::Cyan),
                    "entertainment_action_applied" => ("ENT", Color::Magenta),
                    "entertainment_status_changed" => ("ENT", Color::Magenta),
                    "plugin_command_result" => ("CMD", Color::LightGreen),
                    "plugin_metrics" => ("MET", Color::Yellow),
                    _ => ("EVT", Color::DarkGray),
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("[{tag}] "), Style::default().fg(tag_color)),
                    Span::raw(format!("{} | {}{}", e.timestamp, e.event_type, extra)),
                ]))
            })
            .collect::<Vec<_>>(),
        Tab::Users => app
            .users
            .iter()
            .map(|u| {
                let is_self = app.current_user.as_ref().map(|me| me.id == u.id).unwrap_or(false);
                let role_str = format!("{:?}", u.role);
                let role_color = match u.role {
                    crate::api::Role::Admin    => Color::Yellow,
                    crate::api::Role::User     => Color::White,
                    crate::api::Role::ReadOnly => Color::DarkGray,
                };
                let me_tag = if is_self { " (you)" } else { "" };
                let date = u.created_at.chars().take(10).collect::<String>();
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {:<22}", format!("{}{}", u.username, me_tag)),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(format!("{:<12}", role_str), Style::default().fg(role_color)),
                    Span::styled(date, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect::<Vec<_>>(),
        Tab::Plugins => app
            .plugins
            .iter()
            .map(|p| {
                let (dot, dot_color) = match p.status.as_str() {
                    "active"   => ("●", Color::Green),
                    "degraded" => ("●", Color::Yellow),
                    _          => ("○", Color::Red),
                };
                let ts = p.registered_at.chars().take(19).collect::<String>().replace('T', " ");
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {dot} "), Style::default().fg(dot_color)),
                    Span::styled(format!("{:<30}", p.plugin_id), Style::default().fg(Color::White)),
                    Span::styled(format!("{:<12}", p.status), Style::default().fg(dot_color)),
                    Span::styled(ts, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect::<Vec<_>>(),
    };

    let highlight = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let title = match app.active_tab() {
        Tab::Events => format!("Events [{}]", app.events_filter_mode.title()),
        other => other.title().to_string(),
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(highlight)
        .highlight_symbol(">> ");

    let mut state = ratatui::widgets::ListState::default();
    if !list_is_empty(app) {
        state.select(Some(app.selected));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_plugin_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(plugin_id) = app.plugin_detail_plugin_id.as_deref() else {
        let msg = Paragraph::new("No plugin selected")
            .block(Block::default().borders(Borders::ALL).title("Plugin Detail"));
        frame.render_widget(msg, area);
        return;
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5)])
        .split(area);

    let panel_labels = [
        PluginDetailPanel::Overview.title(),
        PluginDetailPanel::Diagnostics.title(),
        PluginDetailPanel::Metrics.title(),
    ]
    .into_iter()
    .map(Line::from)
    .collect::<Vec<_>>();

    let panel_idx = match app.plugin_detail_panel {
        PluginDetailPanel::Overview => 0,
        PluginDetailPanel::Diagnostics => 1,
        PluginDetailPanel::Metrics => 2,
    };

    let tabs = Tabs::new(panel_labels)
        .select(panel_idx)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Plugin Detail: {}", plugin_id)),
        )
        .style(Style::default().fg(Color::Gray))
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, layout[0]);

    match app.plugin_detail_panel {
        PluginDetailPanel::Overview => {
            let plugin = app.plugins.iter().find(|p| p.plugin_id == plugin_id);
            let plugin_events = app.plugin_events(plugin_id);
            let event_total = plugin_events.len();
            let bridge_rows = app
                .devices
                .iter()
                .filter(|d| {
                    d.plugin_id == plugin_id
                        && d
                            .attributes
                            .get("kind")
                            .and_then(|v| v.as_str())
                            == Some("hue_bridge")
                })
                .map(|d| {
                    let host = d
                        .attributes
                        .get("host")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let bridge_id = d
                        .attributes
                        .get("bridge_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let online = d
                        .attributes
                        .get("online")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(d.available);
                    let integration_state = d
                        .attributes
                        .get("integration_state")
                        .and_then(|v| v.as_str())
                        .or_else(|| {
                            d.attributes
                                .get("summary")
                                .and_then(|s| s.get("integration_state"))
                                .and_then(|v| v.as_str())
                        })
                        .unwrap_or("unknown");
                    let pairing_status = d
                        .attributes
                        .get("pairing_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or(match integration_state {
                            "connected" => "paired",
                            "auth_required" => "unpaired",
                            "unreachable" => "unreachable",
                            _ => "unknown",
                        });
                    let pairing_in_progress = d
                        .attributes
                        .get("pairing_in_progress")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let pairing_last_result = d
                        .attributes
                        .get("pairing_last_result")
                        .and_then(|v| v.as_str())
                        .unwrap_or("n/a");
                    let pairing_last_error = d
                        .attributes
                        .get("pairing_last_error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("none");

                    let pairing_display = if pairing_in_progress {
                        "in_progress"
                    } else {
                        pairing_status
                    };
                    format!(
                        "- {} | host={} | bridge_id={} | online={} | pairing={} | integration={} | pair_result={} | pair_error={}",
                        d.name,
                        host,
                        bridge_id,
                        online,
                        pairing_display,
                        integration_state,
                        pairing_last_result,
                        pairing_last_error
                    )
                })
                .collect::<Vec<_>>();
            let bridge_text = if bridge_rows.is_empty() {
                "- none discovered".to_string()
            } else {
                bridge_rows.join("\n")
            };
            let count_type = |name: &str| {
                plugin_events
                    .iter()
                    .filter(|e| e.event_type == name)
                    .count()
            };
            let button_events = count_type("device_button");
            let rotary_events = count_type("device_rotary");
            let entertainment_events =
                count_type("entertainment_action_applied") + count_type("entertainment_status_changed");
            let metrics_events = count_type("plugin_metrics");
            let body = if let Some(p) = plugin {
                format!(
                    "plugin_id: {}\nstatus: {}\nregistered_at: {}\nws_connected: {}\n\nbridges:\n{}\n\nrecent_event_total: {}\nbutton_events: {}\nrotary_events: {}\nentertainment_events: {}\nmetrics_events: {}",
                    p.plugin_id,
                    p.status,
                    p.registered_at,
                    app.ws_connected,
                    bridge_text,
                    event_total,
                    button_events,
                    rotary_events,
                    entertainment_events,
                    metrics_events,
                )
            } else {
                format!("Plugin '{}' is not present in current plugin list.", plugin_id)
            };
            let widget = Paragraph::new(body)
                .block(Block::default().borders(Borders::ALL).title("Overview"))
                .wrap(Wrap { trim: true });
            frame.render_widget(widget, layout[1]);
        }
        PluginDetailPanel::Diagnostics => {
            let rows = app
                .plugin_events(plugin_id)
                .into_iter()
                .take(20)
                .map(|e| {
                    let detail = e.event_detail.as_deref().unwrap_or("");
                    ListItem::new(format!("{} | {} {}", e.timestamp, e.event_type, detail))
                })
                .collect::<Vec<_>>();
            let list = List::new(rows)
                .block(Block::default().borders(Borders::ALL).title("Diagnostics Events"));
            frame.render_widget(list, layout[1]);
        }
        PluginDetailPanel::Metrics => {
            let latest = app
                .plugin_events(plugin_id)
                .into_iter()
                .find(|e| e.event_type == "plugin_metrics");

            let body = if let Some(m) = latest {
                let detail = m.event_detail.as_deref().unwrap_or("no metric detail");
                format!("timestamp: {}\n{}", m.timestamp, detail)
            } else {
                "No plugin_metrics event found for this plugin yet.".to_string()
            };

            let widget = Paragraph::new(body)
                .block(Block::default().borders(Borders::ALL).title("Metrics Snapshot"))
                .wrap(Wrap { trim: true });
            frame.render_widget(widget, layout[1]);
        }
    }
}

fn draw_dashboard(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    draw_device_list(frame, app, panes[0]);
    draw_device_detail(frame, app, panes[1]);
}

fn draw_device_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (items, render_selected) = if app.view_mode == DeviceViewMode::Grouped {
        build_grouped_list(app)
    } else {
        build_flat_list(app)
    };

    let mode_label = if app.view_mode == DeviceViewMode::Grouped { "Grouped" } else { "Flat" };
    let title = format!("Devices ({mode_label}) [{}]", app.devices.len());

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));

    let mut state = ratatui::widgets::ListState::default();
    state.select(render_selected);
    frame.render_stateful_widget(list, area, &mut state);
}

fn build_grouped_list(app: &App) -> (Vec<ListItem<'static>>, Option<usize>) {
    let groups = app.grouped_devices();
    let mut items: Vec<ListItem<'static>> = Vec::new();
    let mut render_selected: Option<usize> = None;
    let mut flat_idx = 0usize;
    let mut render_idx = 0usize;

    for (area_name, indices) in &groups {
        // Area header row
        items.push(ListItem::new(Line::from(vec![Span::styled(
            format!(" {area_name} "),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
        )])));
        render_idx += 1;

        for &dev_idx in indices {
            let Some(device) = app.devices.get(dev_idx) else {
                flat_idx += 1;
                render_idx += 1;
                continue;
            };

            let is_selected = flat_idx == app.selected;
            if is_selected {
                render_selected = Some(render_idx);
            }

            items.push(device_list_row(app, device, is_selected, true));
            flat_idx += 1;
            render_idx += 1;
        }
    }

    (items, render_selected)
}

fn build_flat_list(app: &App) -> (Vec<ListItem<'static>>, Option<usize>) {
    let items = app
        .devices
        .iter()
        .enumerate()
        .map(|(i, device)| device_list_row(app, device, i == app.selected, false))
        .collect();
    let selected = if app.devices.is_empty() { None } else { Some(app.selected) };
    (items, selected)
}

fn device_list_row(app: &App, device: &DeviceState, is_selected: bool, indent: bool) -> ListItem<'static> {
    let status = app.device_status(device);
    let sc = status_color(&status, device.available);

    let prefix = if indent { "  " } else { "" };
    let name_raw = clean_name(&device.name);
    let name_truncated: String = name_raw.chars().take(26).collect();

    let avail_dot = if device.available {
        Span::styled("● ", Style::default().fg(Color::Green))
    } else {
        Span::styled("○ ", Style::default().fg(Color::DarkGray))
    };

    // Compact sensor suffix
    let mut suffix = String::new();
    if let Some(b) = App::device_battery(device) {
        suffix.push_str(&format!(" {b}%🔋"));
    }
    if let Some(t) = App::device_temperature(device) {
        suffix.push_str(&format!(" {t:.1}°"));
    }
    if let Some(h) = App::device_humidity(device) {
        suffix.push_str(&format!(" {h:.0}%"));
    }
    // Timer countdown suffix
    if device.plugin_id == "core.timer" {
        let timer_state = device.attributes.get("state").and_then(|v| v.as_str()).unwrap_or("idle");
        if matches!(timer_state, "running" | "paused") {
            let remaining_ms = device.attributes.get("remaining_secs").and_then(|v| v.as_u64()).unwrap_or(0) * 1000;
            let icon = if timer_state == "running" { "▶" } else { "⏸" };
            suffix.push_str(&format!(" {icon} {}", format_duration_ms(remaining_ms)));
        }
    }

    if is_selected {
        let sel_style = Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let line = Line::from(vec![
            Span::styled(format!("{prefix}"), sel_style),
            Span::styled("● ", sel_style),
            Span::styled(format!("{name_truncated:<26}"), sel_style),
            Span::styled(format!(" {:<10}", &status), sel_style),
            Span::styled(suffix, sel_style),
        ]);
        ListItem::new(line)
    } else {
        let base_style = if device.available {
            Style::default()
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let status_style = if device.available {
            Style::default().fg(sc)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let line = Line::from(vec![
            Span::styled(format!("{prefix}"), base_style),
            avail_dot,
            Span::styled(format!("{name_truncated:<26}"), base_style),
            Span::styled(format!(" {:<10}", &status), status_style),
            Span::styled(suffix, Style::default().fg(Color::DarkGray)),
        ]);
        ListItem::new(line)
    }
}

fn draw_device_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Detail");

    let Some(device) = app.selected_device() else {
        let msg = Paragraph::new("No device selected")
            .block(block)
            .alignment(Alignment::Center);
        frame.render_widget(msg, area);
        return;
    };

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Name
    lines.push(Line::from(vec![Span::styled(
        clean_name(&device.name),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(""));

    // Status
    let status = app.device_status(device);
    let sc = status_color(&status, device.available);
    lines.push(detail_row(
        "Status",
        vec![Span::styled(status, Style::default().fg(sc).add_modifier(Modifier::BOLD))],
    ));

    // Availability
    let (avail_str, avail_color) = if device.available {
        ("Online", Color::Green)
    } else {
        ("Offline", Color::Red)
    };
    lines.push(detail_row(
        "Available",
        vec![Span::styled(avail_str, Style::default().fg(avail_color))],
    ));

    // Area
    if let Some(area_name) = &device.area {
        lines.push(detail_row("Area", vec![Span::raw(area_name.clone())]));
    }

    // Plugin
    lines.push(detail_row(
        "Plugin",
        vec![Span::raw(clean_plugin_id(&device.plugin_id))],
    ));

    // ZWave node location (from nodeInfo, distinct from the HC area field)
    if let Some(loc) = device.attributes.get("location").and_then(|v| v.as_str()) {
        if !loc.is_empty() {
            lines.push(detail_row("ZW Location", vec![Span::styled(loc.to_string(), Style::default().fg(Color::DarkGray))]));
        }
    }

    // Last seen
    if !device.last_seen.is_empty() {
        lines.push(detail_row(
            "Last seen",
            vec![Span::styled(
                format_timestamp(&device.last_seen),
                Style::default().fg(Color::DarkGray),
            )],
        ));
    }

    lines.push(Line::from(""));

    // Timer detail (core.timer devices)
    if device.plugin_id == "core.timer" {
        let timer_state = device.attributes.get("state").and_then(|v| v.as_str()).unwrap_or("idle");
        let duration_ms = device.attributes.get("duration_secs").and_then(|v| v.as_u64()).unwrap_or(0) * 1000;
        let remaining_ms = device.attributes.get("remaining_secs").and_then(|v| v.as_u64()).unwrap_or(0) * 1000;
        let repeat = device.attributes.get("repeat").and_then(|v| v.as_bool()).unwrap_or(false);

        let state_color = match timer_state {
            "running"   => Color::Green,
            "paused"    => Color::Yellow,
            "fired"     => Color::Cyan,
            "cancelled" => Color::DarkGray,
            _           => Color::DarkGray,
        };
        lines.push(detail_row(
            "Timer State",
            vec![Span::styled(
                normalize_label(timer_state),
                Style::default().fg(state_color).add_modifier(Modifier::BOLD),
            )],
        ));

        if duration_ms > 0 {
            lines.push(detail_row("Duration", vec![Span::raw(format_duration_ms(duration_ms))]));
        }

        if matches!(timer_state, "running" | "paused") && duration_ms > 0 {
            let progress = 1.0 - (remaining_ms as f64 / duration_ms as f64).clamp(0.0, 1.0);
            let bar = make_bar(progress, 10);
            lines.push(detail_row(
                "Remaining",
                vec![
                    Span::styled(
                        format!("{} ", format_duration_ms(remaining_ms)),
                        Style::default().fg(state_color),
                    ),
                    Span::styled(bar, Style::default().fg(state_color)),
                ],
            ));
        }

        if repeat {
            lines.push(detail_row("Repeat", vec![Span::styled("Yes", Style::default().fg(Color::Yellow))]));
        }

        if let Some(lbl) = device.attributes.get("label").and_then(|v| v.as_str()) {
            if !lbl.is_empty() {
                lines.push(detail_row("Label", vec![Span::raw(lbl.to_string())]));
            }
        }

        if let Some(started) = device.attributes.get("started_at").and_then(|v| v.as_str()) {
            lines.push(detail_row(
                "Started",
                vec![Span::styled(started.to_string(), Style::default().fg(Color::DarkGray))],
            ));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Commands: start {duration_secs:N}  pause  resume  cancel  restart",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            "Via: PATCH /api/v1/devices/{id}/state",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Battery
    if let Some(battery) = App::device_battery(device) {
        let bc = if battery <= 20 {
            Color::Red
        } else if battery <= 40 {
            Color::Yellow
        } else {
            Color::Green
        };
        let bar = make_bar(battery as f64 / 100.0, 10);
        let low = if battery <= 20 { " LOW" } else { "" };
        lines.push(detail_row(
            "Battery",
            vec![
                Span::styled(format!("{battery:3}% "), Style::default().fg(bc)),
                Span::styled(bar, Style::default().fg(bc)),
                Span::styled(low, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            ],
        ));
    }

    // Temperature
    if let Some(temp) = App::device_temperature(device) {
        lines.push(detail_row(
            "Temperature",
            vec![Span::styled(
                format!("{temp:.1}°F"),
                Style::default().fg(Color::Yellow),
            )],
        ));
    }

    // Humidity
    if let Some(humidity) = App::device_humidity(device) {
        lines.push(detail_row(
            "Humidity",
            vec![Span::styled(
                format!("{humidity:.0}%"),
                Style::default().fg(Color::Cyan),
            )],
        ));
    }

    // Brightness
    if let Some(brightness) = App::device_brightness(device) {
        let bar = make_bar(brightness as f64 / 100.0, 10);
        lines.push(detail_row(
            "Brightness",
            vec![
                Span::styled(format!("{brightness:3}% "), Style::default().fg(Color::Yellow)),
                Span::styled(bar, Style::default().fg(Color::Yellow)),
            ],
        ));
    }

    // Lock state (CC 98 currentMode)
    if let Some(locked) = App::device_lock_state(device) {
        let (lock_str, lock_color) = if locked {
            ("Locked", Color::Red)
        } else {
            ("Unlocked", Color::Green)
        };
        lines.push(detail_row(
            "Lock",
            vec![Span::styled(lock_str, Style::default().fg(lock_color))],
        ));

        // Physical bolt sensor (only present when hardware supports it)
        if let Some(bolt) = device.attributes.get("bolt_status").and_then(|v| v.as_str()) {
            let (s, c) = if bolt == "locked" {
                ("Locked", Color::Red)
            } else {
                ("Unlocked", Color::Green)
            };
            lines.push(detail_row("Bolt", vec![Span::styled(s, Style::default().fg(c))]));
        }
        // Physical latch sensor
        if let Some(latch) = device.attributes.get("latch_status").and_then(|v| v.as_str()) {
            let (s, c) = if latch == "closed" {
                ("Closed", Color::Green)
            } else {
                ("Open", Color::Yellow)
            };
            lines.push(detail_row("Latch", vec![Span::styled(s, Style::default().fg(c))]));
        }
        // Door open/closed sensor — string variant (ZWave)
        if let Some(door) = device.attributes.get("door_status").and_then(|v| v.as_str()) {
            let (s, c) = if door == "closed" {
                ("Closed", Color::Green)
            } else {
                ("Open", Color::Yellow)
            };
            lines.push(detail_row("Door", vec![Span::styled(s, Style::default().fg(c))]));
        }
        // Door open/closed sensor — bool variant (YoLink)
        if let Some(door_open) = device.attributes.get("door_open").and_then(|v| v.as_bool()) {
            let (s, c) = if door_open {
                ("Open", Color::Yellow)
            } else {
                ("Closed", Color::Green)
            };
            lines.push(detail_row("Door", vec![Span::styled(s, Style::default().fg(c))]));
        }
        // Last alert (e.g. UnLockFailed, DoorOpenAlarm)
        if let Some(alert) = device.attributes.get("last_alert").and_then(|v| v.as_str()) {
            lines.push(detail_row("Last Alert", vec![Span::styled(alert.to_string(), Style::default().fg(Color::Yellow))]));
        }
        // Auto-lock timeout (YoLink attributes.autoLock)
        if let Some(secs) = device.attributes.get("auto_lock_secs").and_then(|v| v.as_u64()) {
            if secs > 0 {
                lines.push(detail_row("Auto-lock", vec![Span::raw(format!("{secs}s"))]));
            }
        }
        // Operation type: 1=Constant, 2=Timed (ZWave)
        if let Some(op_type) = device.attributes.get("lock_operation_type").and_then(|v| v.as_f64()) {
            let label = match op_type as u64 {
                1 => "Constant",
                2 => "Timed",
                _ => "Unknown",
            };
            lines.push(detail_row("Op Mode", vec![Span::raw(label)]));
        }
        // Timed mode timeout
        if let Some(timeout) = device.attributes.get("lock_timeout_secs").and_then(|v| v.as_f64()) {
            if timeout > 0.0 {
                lines.push(detail_row("Timeout", vec![Span::raw(format!("{timeout:.0}s"))]));
            }
        }
        if let Some(relock) = device.attributes.get("lock_auto_relock_secs").and_then(|v| v.as_f64()) {
            if relock > 0.0 {
                lines.push(detail_row("Auto-relock", vec![Span::raw(format!("{relock:.0}s"))]));
            }
        }
    }

    // Motion sensor
    if let Some(motion) = device.attributes.get("motion").and_then(|v| v.as_bool()) {
        let (s, c) = if motion { ("Motion", Color::Yellow) } else { ("Clear", Color::Green) };
        lines.push(detail_row("Motion", vec![Span::styled(s, Style::default().fg(c))]));
    }

    // Contact sensor
    if let Some(open) = device.attributes.get("contact_open").and_then(|v| v.as_bool()) {
        let (s, c) = if open { ("Open", Color::Red) } else { ("Closed", Color::Green) };
        lines.push(detail_row("Contact", vec![Span::styled(s, Style::default().fg(c))]));
    }

    // Window covering position
    if let Some(pos) = device.attributes.get("position").and_then(|v| v.as_f64()) {
        let bar = make_bar(pos / 100.0, 10);
        lines.push(detail_row(
            "Position",
            vec![
                Span::styled(format!("{pos:3.0}% "), Style::default().fg(Color::Cyan)),
                Span::styled(bar, Style::default().fg(Color::Cyan)),
            ],
        ));
    }

    // Thermostat
    if let Some(mode) = device.attributes.get("mode").and_then(|v| v.as_str()) {
        let mc = match mode { "heat" => Color::Red, "cool" => Color::Cyan, "off" => Color::DarkGray, _ => Color::White };
        lines.push(detail_row("Mode", vec![Span::styled(normalize_label(mode), Style::default().fg(mc))]));
    }
    if let Some(action) = device.attributes.get("hvac_action").and_then(|v| v.as_str()) {
        lines.push(detail_row("HVAC", vec![Span::raw(normalize_label(action))]));
    }
    if let Some(setpoint) = device.attributes.get("target_temp").and_then(|v| v.as_f64()) {
        lines.push(detail_row("Setpoint", vec![Span::styled(format!("{setpoint:.1}°F"), Style::default().fg(Color::Yellow))]));
    }

    // Energy monitoring
    let has_energy = ["power_w", "energy_kwh", "voltage", "current_a"].iter().any(|k| device.attributes.contains_key(*k));
    if has_energy {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("─── Energy ───", Style::default().fg(Color::DarkGray))));
    }
    if let Some(w) = device.attributes.get("power_w").and_then(|v| v.as_f64()) {
        lines.push(detail_row("Power", vec![Span::styled(format!("{w:.1} W"), Style::default().fg(Color::Yellow))]));
    }
    if let Some(kwh) = device.attributes.get("energy_kwh").and_then(|v| v.as_f64()) {
        lines.push(detail_row("Energy", vec![Span::raw(format!("{kwh:.3} kWh"))]));
    }
    if let Some(v) = device.attributes.get("voltage").and_then(|v| v.as_f64()) {
        lines.push(detail_row("Voltage", vec![Span::raw(format!("{v:.1} V"))]));
    }
    if let Some(a) = device.attributes.get("current_a").and_then(|v| v.as_f64()) {
        lines.push(detail_row("Current", vec![Span::raw(format!("{a:.2} A"))]));
    }

    // Environmental extras
    if let Some(lux) = device.attributes.get("illuminance").and_then(|v| v.as_f64()) {
        lines.push(detail_row("Illuminance", vec![Span::raw(format!("{lux:.0} lx"))]));
    }
    if let Some(co2) = device.attributes.get("co2_ppm").and_then(|v| v.as_f64()) {
        lines.push(detail_row("CO₂", vec![Span::raw(format!("{co2:.0} ppm"))]));
    }

    // Alarm states (only show when active)
    for (key, label) in &[("smoke", "Smoke"), ("co", "CO"), ("water_detected", "Water"), ("tamper", "Tamper"), ("vibration", "Vibration")] {
        if let Some(true) = device.attributes.get(*key).and_then(|v| v.as_bool()) {
            lines.push(detail_row(label, vec![Span::styled("ALARM", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))]));
        }
    }

    // Other attributes — everything not already rendered above, excluding ZWave
    // internal noise properties that have no user-visible meaning.
    let shown = [
        "on", "state", "open", "online", "locked",
        "battery", "battery_level", "battery_percent", "battery_low",
        "temperature", "temp", "humidity", "brightness",
        "motion", "contact_open", "position", "location",
        "mode", "hvac_action", "target_temp",
        "power_w", "energy_kwh", "voltage", "current_a",
        "illuminance", "co2_ppm", "pressure", "uv_index",
        "smoke", "co", "water_detected", "tamper", "vibration",
        "color_rgb", "color_temp",
        // Door lock physical sensors + config
        "bolt_status", "latch_status", "door_status", "door_open",
        "lock_operation_type", "lock_timeout_secs", "lock_auto_relock_secs",
        "last_alert", "auto_lock_secs", "sound_level",
        // Timer device attributes
        "duration_secs", "remaining_secs", "repeat", "started_at", "label",
    ];
    // ZWave internal / write-echo properties with no useful display value.
    // Also includes raw nodeInfo keys that survived field_map (shouldn't normally
    // appear, but guard against config mismatches).
    let zwave_noise = [
        "targetValue", "currentValue", "targetMode", "currentMode",
        "duration", "restorePrevious", "targetColor", "currentColor",
        "nodeName", "nodeLocation",  // raw nodeInfo keys (mapped → name/location)
    ];

    let other_attrs: Vec<(&String, &serde_json::Value)> = device
        .attributes
        .iter()
        .filter(|(k, _)| !shown.contains(&k.as_str()) && !zwave_noise.contains(&k.as_str()))
        .collect();

    if !other_attrs.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─── Other ───",
            Style::default().fg(Color::DarkGray),
        )));
        for (key, val) in &other_attrs {
            let val_str = val.as_str().map(|s| s.to_string()).unwrap_or_else(|| val.to_string());
            let display_key = normalize_label(key);
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<13}", display_key), Style::default().fg(Color::DarkGray)),
                Span::raw(val_str),
            ]));
        }
    }

    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(para, inner);
}

fn draw_device_editor(frame: &mut Frame<'_>, app: &App) {
    let Some(editor) = app.device_editor.as_ref() else {
        return;
    };

    let popup = centered_rect(72, 58, frame.area());
    frame.render_widget(Clear, popup);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(popup);

    let title = Paragraph::new(format!("Device: {}", editor.device_id))
        .alignment(Alignment::Left)
        .block(Block::default().borders(Borders::ALL).title("Edit Device"));
    frame.render_widget(title, layout[0]);

    let name_style = if matches!(editor.field, DeviceEditField::Name) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let name = Paragraph::new(editor.name.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Name")
            .border_style(name_style),
    );
    frame.render_widget(name, layout[1]);

    let area_style = if matches!(editor.field, DeviceEditField::Area) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let area = Paragraph::new(editor.area.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Area / Room")
            .border_style(area_style),
    );
    frame.render_widget(area, layout[2]);

    let status = app
        .devices
        .iter()
        .find(|device| device.device_id == editor.device_id)
        .map(|device| app.device_status(device))
        .unwrap_or_else(|| "Unknown".to_string());
    let status_line = Paragraph::new(format!("Status: {status}"))
        .block(Block::default().borders(Borders::ALL).title("Device Status"));
    frame.render_widget(status_line, layout[3]);

    let help = Paragraph::new("Tab switch field | Enter save | Esc cancel")
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(help, layout[4]);
}

fn draw_area_editor(frame: &mut Frame<'_>, app: &App, editor: &AreaEditor) {
    let title = if editor.id.is_none() { "New Area" } else { "Rename Area" };
    let popup = centered_rect(60, 30, frame.area());
    frame.render_widget(Clear, popup);

    let outer = Block::default().borders(Borders::ALL).title(title);
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(inner);

    let name_field = Paragraph::new(editor.name.as_str())
        .block(Block::default().borders(Borders::ALL).title("Name")
            .border_style(Style::default().fg(Color::Yellow)));
    frame.render_widget(name_field, layout[0]);

    let help = Paragraph::new("Enter save | Esc cancel").alignment(Alignment::Center);
    frame.render_widget(help, layout[1]);

    let _ = app;
}

fn draw_user_editor(frame: &mut Frame<'_>, app: &App, editor: &UserEditor) {
    let title = match editor.mode {
        UserEditMode::Create         => "New User",
        UserEditMode::EditRole       => "Change Role",
        UserEditMode::ChangePassword => "Change Password",
    };
    let popup = centered_rect(64, 60, frame.area());
    frame.render_widget(Clear, popup);

    let outer = Block::default().borders(Borders::ALL).title(title);
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let focused = Style::default().fg(Color::Yellow);
    let normal  = Style::default();

    match editor.mode {
        UserEditMode::Create => {
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Min(1),
                ])
                .split(inner);

            frame.render_widget(
                Paragraph::new(editor.username.as_str()).block(
                    Block::default().borders(Borders::ALL).title("Username")
                        .border_style(if editor.field == UserEditField::Username { focused } else { normal })
                ),
                layout[0],
            );
            let pw_mask = "*".repeat(editor.password.len());
            frame.render_widget(
                Paragraph::new(pw_mask).block(
                    Block::default().borders(Borders::ALL).title("Password")
                        .border_style(if editor.field == UserEditField::Password { focused } else { normal })
                ),
                layout[1],
            );
            let cpw_mask = "*".repeat(editor.confirm_password.len());
            frame.render_widget(
                Paragraph::new(cpw_mask).block(
                    Block::default().borders(Borders::ALL).title("Confirm Password")
                        .border_style(if editor.field == UserEditField::ConfirmPassword { focused } else { normal })
                ),
                layout[2],
            );
            let role_str = format!("{:?}  (Space to cycle)", editor.role);
            frame.render_widget(
                Paragraph::new(role_str).block(
                    Block::default().borders(Borders::ALL).title("Role")
                        .border_style(if editor.field == UserEditField::Role { focused } else { normal })
                ),
                layout[3],
            );
            let help = Paragraph::new("Tab/↑↓ field | Space cycle role | Enter save | Esc cancel")
                .alignment(Alignment::Center);
            frame.render_widget(help, layout[4]);
        }
        UserEditMode::EditRole => {
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),
                    Constraint::Length(3),
                    Constraint::Min(1),
                ])
                .split(inner);
            let label = Paragraph::new(format!("User: {}", editor.username))
                .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD));
            frame.render_widget(label, layout[0]);
            let role_str = format!("{:?}  (Space to cycle)", editor.role);
            frame.render_widget(
                Paragraph::new(role_str).block(Block::default().borders(Borders::ALL).title("Role").border_style(focused)),
                layout[1],
            );
            let help = Paragraph::new("Space cycle | Enter save | Esc cancel").alignment(Alignment::Center);
            frame.render_widget(help, layout[2]);
        }
        UserEditMode::ChangePassword => {
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Min(1),
                ])
                .split(inner);
            let label = Paragraph::new(format!("User: {}", editor.username))
                .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD));
            frame.render_widget(label, layout[0]);
            let cpw_mask = "*".repeat(editor.current_password.len());
            frame.render_widget(
                Paragraph::new(cpw_mask).block(
                    Block::default().borders(Borders::ALL).title("Current Password")
                        .border_style(if editor.field == UserEditField::CurrentPassword { focused } else { normal })
                ),
                layout[1],
            );
            let pw_mask = "*".repeat(editor.password.len());
            frame.render_widget(
                Paragraph::new(pw_mask).block(
                    Block::default().borders(Borders::ALL).title("New Password")
                        .border_style(if editor.field == UserEditField::Password { focused } else { normal })
                ),
                layout[2],
            );
            let confirm_mask = "*".repeat(editor.confirm_password.len());
            frame.render_widget(
                Paragraph::new(confirm_mask).block(
                    Block::default().borders(Borders::ALL).title("Confirm New Password")
                        .border_style(if editor.field == UserEditField::ConfirmPassword { focused } else { normal })
                ),
                layout[3],
            );
            let help = Paragraph::new("Tab/↑↓ field | Enter save | Esc cancel").alignment(Alignment::Center);
            frame.render_widget(help, layout[4]);
        }
    }

    // Suppress unused warning
    let _ = app;
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn status_color(status: &str, available: bool) -> Color {
    if !available {
        return Color::DarkGray;
    }
    match status {
        "On" | "Open" | "Unlocked" | "Online" => Color::Green,
        "Off" | "Closed" | "Locked" | "Offline" => Color::Red,
        "Unknown" => Color::DarkGray,
        _ => Color::White,
    }
}

/// Build a `"Label  : "` + spans detail row.
fn detail_row(label: &str, spans: Vec<Span<'static>>) -> Line<'static> {
    let mut all = vec![Span::styled(
        format!("{label:<11}: "),
        Style::default().fg(Color::DarkGray),
    )];
    all.extend(spans);
    Line::from(all)
}

fn make_bar(ratio: f64, width: usize) -> String {
    let filled = (ratio.clamp(0.0, 1.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

fn format_duration_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}h {mins:02}m {secs:02}s")
    } else if mins > 0 {
        format!("{mins}:{secs:02}")
    } else {
        format!("0:{secs:02}")
    }
}

fn format_timestamp(ts: &str) -> String {
    if let Some(time_part) = ts.split('T').nth(1) {
        time_part.split('.').next().unwrap_or(time_part).to_string()
    } else {
        ts.chars().take(20).collect()
    }
}

fn clean_plugin_id(plugin_id: &str) -> String {
    let mut value = plugin_id.to_string();
    for prefix in ["plugin.", "plugin_", "hc-"] {
        if let Some(stripped) = value.strip_prefix(prefix) {
            value = stripped.to_string();
        }
    }
    if let Some(last) = value.rsplit('.').next() {
        value = last.to_string();
    }
    normalize_label(&value)
}

fn clean_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "-".to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_label(value: &str) -> String {
    // Split camelCase before handling underscores/spaces.
    // "targetValue" → "target Value", "hvacAction" → "hvac Action"
    let mut spaced = String::with_capacity(value.len() + 4);
    let mut prev_lower = false;
    for ch in value.chars() {
        if ch.is_uppercase() && prev_lower {
            spaced.push(' ');
        }
        prev_lower = ch.is_lowercase();
        spaced.push(ch);
    }
    spaced
        .replace('_', " ")
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!(
                    "{}{}",
                    first.to_ascii_uppercase(),
                    chars.as_str().to_ascii_lowercase()
                ),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn list_is_empty(app: &App) -> bool {
    match app.active_tab() {
        Tab::Devices => app.devices.is_empty(),
        Tab::Scenes => app.scenes.is_empty(),
        Tab::Areas => app.areas.is_empty(),
        Tab::Automations => app.automations.is_empty(),
        Tab::Events => app.filtered_events().is_empty(),
        Tab::Users => app.users.is_empty(),
        Tab::Plugins => app.plugins.is_empty(),
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
