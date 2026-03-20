use crate::app::{App, FocusField, LoginPhase, Tab};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Clear, Gauge, List, ListItem, Paragraph, Row, Table, Tabs, Wrap},
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

    let role = app
        .current_user
        .as_ref()
        .map(|u| format!("{:?}", u.role))
        .unwrap_or_else(|| "unknown".to_string());
    let live = if app.ws_connected { "connected" } else { "disconnected" };
    let mut status_line = format!(
        "user={} role={} live_ws={} | Tab/Shift+Tab switch tab | j/k move | r refresh | q quit",
        app.current_user
            .as_ref()
            .map(|u| u.username.as_str())
            .unwrap_or("n/a"),
        role,
        live
    );
    if matches!(app.active_tab(), Tab::Devices) {
        status_line.push_str(" | t toggle selected device");
    }
    if matches!(app.active_tab(), Tab::Scenes) {
        status_line.push_str(" | a activate selected scene");
    }
    if let Some(err) = &app.error {
        status_line = format!("ERROR: {err}");
    } else {
        status_line = format!("{} | {}", app.status, status_line);
    }
    let footer = Paragraph::new(status_line)
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .wrap(Wrap { trim: true });
    frame.render_widget(footer, layout[2]);
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

    let help = Paragraph::new("Tab switch field | Enter login | Esc quit")
        .alignment(Alignment::Center);
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

    let message = app
        .error
        .clone()
        .unwrap_or_else(|| "Connects to /api/v1/auth/login and loads/saves cache snapshots locally".to_string());
    let msg = Paragraph::new(message).alignment(Alignment::Center);
    frame.render_widget(msg, layout[5]);
}

fn draw_tab_body(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if matches!(app.active_tab(), Tab::Devices) {
        draw_devices_table(frame, app, area);
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
            .map(|a| ListItem::new(format!("{} ({}) devices={}", a.name, a.id, a.device_ids.len())))
            .collect::<Vec<_>>(),
        Tab::Automations => app
            .automations
            .iter()
            .map(|r| ListItem::new(format!("{} ({}) enabled={} priority={}", r.name, r.id, r.enabled, r.priority)))
            .collect::<Vec<_>>(),
        Tab::Events => app
            .events
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
                ListItem::new(format!("{} | {}{}", e.timestamp, e.event_type, extra))
            })
            .collect::<Vec<_>>(),
        Tab::Users => app
            .users
            .iter()
            .map(|u| {
                ListItem::new(format!(
                    "{} ({}) role={:?} created_at={}",
                    u.username, u.id, u.role, u.created_at
                ))
            })
            .collect::<Vec<_>>(),
        Tab::Plugins => app
            .plugins
            .iter()
            .map(|p| ListItem::new(format!("{} status={} registered_at={}", p.plugin_id, p.status, p.registered_at)))
            .collect::<Vec<_>>(),
    };

    let highlight = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(app.active_tab().title()))
        .highlight_style(highlight)
        .highlight_symbol(">> ");

    let mut state = ratatui::widgets::ListState::default();
    if !list_is_empty(app) {
        state.select(Some(app.selected));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_devices_table(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let header = Row::new(vec![
        Cell::from("Name"),
        Cell::from("Status"),
        Cell::from("Availability"),
        Cell::from("Room"),
        Cell::from("Plugin"),
    ])
    .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    let rows = app
        .devices
        .iter()
        .map(|device| {
            Row::new(vec![
                Cell::from(clean_name(&device.name)),
                Cell::from(canonical_device_status(device)),
                Cell::from(if device.available { "Online" } else { "Offline" }),
                Cell::from(device.area.clone().unwrap_or_else(|| "-".to_string())),
                Cell::from(clean_plugin_id(&device.plugin_id)),
            ])
        })
        .collect::<Vec<_>>();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(32),
            Constraint::Percentage(14),
            Constraint::Percentage(18),
            Constraint::Percentage(18),
            Constraint::Percentage(18),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title("Devices"))
    .row_highlight_style(
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );

    let mut state = ratatui::widgets::TableState::default();
    if !app.devices.is_empty() {
        state.select(Some(app.selected));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

fn canonical_device_status(device: &crate::api::DeviceState) -> String {
    if let Some(on) = device.attributes.get("on").and_then(|v| v.as_bool()) {
        return if on { "On".to_string() } else { "Off".to_string() };
    }
    if let Some(state) = device.attributes.get("state").and_then(|v| v.as_str()) {
        return normalize_label(state);
    }
    if let Some(open) = device.attributes.get("open").and_then(|v| v.as_bool()) {
        return if open { "Open".to_string() } else { "Closed".to_string() };
    }
    if let Some(online) = device.attributes.get("online").and_then(|v| v.as_bool()) {
        return if online { "Online".to_string() } else { "Offline".to_string() };
    }
    "Unknown".to_string()
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
    value
        .replace('_', " ")
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str().to_ascii_lowercase()),
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
        Tab::Events => app.events.is_empty(),
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
