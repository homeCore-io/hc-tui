use crate::app::{
    format_timestamp_utc, AdminSubPanel, App, AreaEditor, AutomationFilterBar,
    AutomationFilterField, DeleteConfirm,
    DeviceEditField, DeviceSubPanel, DeviceViewMode, FocusField, LogLevelFilter, LoginPhase,
    ModeEditField, ModeEditor, ModeKind, PluginDetailPanel, SwitchEditField, SwitchEditor,
    Tab, TimerEditField, TimerEditor, UserEditField, UserEditMode, UserEditor,
};
use crate::api::DeviceState;
use chrono::Utc;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, Gauge, List, ListItem, Paragraph, Row, Table, Tabs, Wrap,
    },
    Frame,
};

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    if !app.authenticated {
        draw_login(frame, app);
        return;
    }

    let footer_height = compute_footer_height(app, frame.area().width);

    // Main layout: left menu sidebar + right content + footer
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),
            Constraint::Length(footer_height),
        ])
        .split(frame.area());

    // Split content area into left menu and right content
    let content_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20),  // Fixed width for left menu
            Constraint::Min(10),      // Remaining space for content
        ])
        .split(layout[0]);

    // Draw left sidebar menu with tabs
    draw_menu_sidebar(frame, app, content_layout[0]);

    // Draw tab content
    draw_tab_body(frame, app, content_layout[1]);

    draw_status_bar(frame, app, layout[1]);

    if app.device_editor.is_some() {
        draw_device_editor(frame, app);
    }
    if let Some(editor) = app.area_editor.as_ref() {
        draw_area_editor(frame, app, editor);
    }
    if let Some(editor) = app.user_editor.as_ref() {
        draw_user_editor(frame, app, editor);
    }
    if let Some(editor) = app.switch_editor.as_ref() {
        draw_switch_editor(frame, editor);
    }
    if let Some(editor) = app.timer_editor.as_ref() {
        draw_timer_editor(frame, editor);
    }
    if let Some(editor) = app.mode_editor.as_ref() {
        draw_mode_editor(frame, editor);
    }
    // Automation overlays (drawn on top of everything)
    if let Some(confirm) = app.automation_delete_confirm.as_ref() {
        draw_delete_confirm(frame, confirm);
    }
    if app.groups_open {
        draw_groups_overlay(frame, app);
    }
    if let Some(filter_bar) = app.automation_filter_bar.as_ref() {
        draw_filter_bar(frame, filter_bar);
    }
    if app.log_module_input_open {
        draw_log_module_input(frame, app);
    }
}

fn draw_menu_sidebar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let menu_items: Vec<ListItem> = app
        .tabs()
        .iter()
        .enumerate()
        .map(|(idx, tab)| {
            let num = idx + 1;
            let prefix = if idx == app.tab { "► " } else { "  " };
            let style = if idx == app.tab {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(Span::styled(
                format!("{}{}) {}", prefix, num, tab.title()),
                style,
            ))
        })
        .collect();

    let menu_list = List::new(menu_items)
        .block(Block::default().borders(Borders::RIGHT).title("Menu"));
    frame.render_widget(menu_list, area);
}

fn draw_status_bar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let role = app
        .current_user
        .as_ref()
        .map(|u| format!("{:?}", u.role))
        .unwrap_or_else(|| "unknown".to_string());
    let live = if app.ws_connected { "●" } else { "○" };

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

    let hints = status_hints(app);

    let inner_width = area.width.saturating_sub(2) as usize;
    let status_line = fit_single_line(
        &format!("{status_text} | {user_str} ({role}) {live}"),
        inner_width,
    );
    let hint_lines = fit_hints_lines(&hints, inner_width);
    let style = if app.error.is_some() {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    };

    let mut lines = vec![Line::from(vec![Span::styled(status_line, style)])];
    for (idx, text) in hint_lines.iter().enumerate() {
        let prefix = if idx == 0 { "Keys: " } else { "      " };
        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(Color::DarkGray)),
            Span::styled(text.clone(), Style::default().fg(Color::White)),
        ]));
    }

    let footer = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .wrap(Wrap { trim: false });
    frame.render_widget(footer, area);
}

fn compute_footer_height(app: &App, width: u16) -> u16 {
    let inner_width = width.saturating_sub(2) as usize;
    let hint_lines = fit_hints_lines(&status_hints(app), inner_width);
    // content lines + top/bottom borders
    let content_lines = 1usize + hint_lines.len();
    (content_lines + 2) as u16
}

fn status_hints(app: &App) -> Vec<&'static str> {
    let mut hints = vec!["Tab/Shift+Tab menu", "1-9 jump tab", "j/k move", "r refresh", "q quit", "T time"];
    match app.active_tab() {
        Tab::Devices => {
            hints.push("◄/► panel");
            match app.device_sub {
                DeviceSubPanel::All => {
                    hints.push("Spc toggle");
                    hints.push("t on/off");
                    hints.push("+/- brightness");
                    hints.push("l/u lock");
                    hints.push("v grouped/flat");
                    hints.push("Enter edit");
                    hints.push("d delete");
                }
                DeviceSubPanel::Switches => {
                    hints.push("n new switch");
                    hints.push("Enter edit");
                    hints.push("d delete");
                }
                DeviceSubPanel::Timers => {
                    hints.push("n new timer");
                    hints.push("Enter edit");
                    hints.push("d delete");
                }
            }
        }
        Tab::Scenes => { hints.push("a activate"); }
        Tab::Areas => {
            hints.push("n new area");
            hints.push("Enter rename");
            hints.push("◄/► pane");
            hints.push("Spc select dev");
            hints.push("+ add device");
            hints.push("- remove device");
            hints.push("d delete");
        }
        Tab::Plugins => { hints.push("d deregister"); }
        Tab::Manage => {
            hints.push("◄/► panel");
            if matches!(app.admin_sub, AdminSubPanel::Status) {
                hints.push("r refresh");
            } else if matches!(app.admin_sub, AdminSubPanel::Users) {
                hints.push("n new");
                hints.push("Enter role");
                hints.push("p password");
                hints.push("d delete");
            } else if matches!(app.admin_sub, AdminSubPanel::Logs) {
                hints.push("p pause");
                hints.push("Spc pause");
                hints.push("e/w/i level");
                hints.push("/ module");
                hints.push("c clear");
            } else if matches!(app.admin_sub, AdminSubPanel::Events) {
                hints.push("f filter");
            } else {
                hints.push("n new");
                hints.push("d delete");
            }
        }
        Tab::Automations => {
            hints.push("e enable");
            hints.push("d disable");
            hints.push("c clone");
            hints.push("x delete");
            hints.push("h history");
            hints.push("f filter");
            hints.push("s stale");
            hints.push("g groups");
            hints.push("Spc select");
        }
    }

    if app.device_editor.is_some() {
        return vec!["Tab field", "Enter save", "Esc cancel"];
    }
    if app.area_editor.is_some() {
        return vec!["Enter save", "Esc cancel"];
    }
    if app.user_editor.is_some() {
        return vec!["Tab field", "Space cycle role", "Enter save", "Esc cancel"];
    }
    if app.plugin_detail_open {
        return vec!["1/2/3 panel", "b discover", "p pair", "r refresh", "Esc close", "q quit"];
    }
    if app.switch_editor.is_some() {
        return vec!["Tab field", "Enter create", "Esc cancel"];
    }
    if app.timer_editor.is_some() {
        return vec!["Tab field", "Enter create", "Esc cancel"];
    }
    if app.mode_editor.is_some() {
        return vec!["Tab field", "Space kind", "Enter create", "Esc cancel"];
    }

    hints
}

fn fit_single_line(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    let mut out = input
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}

fn fit_hints_lines(hints: &[&str], max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return vec![String::new()];
    }

    let body_width = max_chars.saturating_sub(6).max(1);
    let sep = " | ";
    let sep_len = sep.chars().count();
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut used = 0usize;

    for hint in hints {
        let hint_len = hint.chars().count();
        let join_len = if current.is_empty() { 0 } else { sep_len };

        if used + join_len + hint_len > body_width {
            if current.is_empty() {
                lines.push(fit_single_line(hint, body_width));
                used = 0;
                continue;
            }
            lines.push(current);
            current = String::new();
            used = 0;
        }

        if !current.is_empty() {
            current.push_str(sep);
            used += sep_len;
        }
        current.push_str(hint);
        used += hint_len;
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
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
    if matches!(app.active_tab(), Tab::Scenes) {
        draw_scenes_table(frame, app, area);
        return;
    }
    if matches!(app.active_tab(), Tab::Automations) {
        draw_automations_tab(frame, app, area);
        return;
    }
    if matches!(app.active_tab(), Tab::Manage) {
        draw_manage_tab(frame, app, area);
        return;
    }
    if matches!(app.active_tab(), Tab::Plugins) && app.plugin_detail_open {
        draw_plugin_detail(frame, app, area);
        return;
    }
    if matches!(app.active_tab(), Tab::Manage) {
        draw_manage_tab(frame, app, area);
        return;
    }
    if matches!(app.active_tab(), Tab::Areas) {
        draw_areas_pane(frame, app, area);
        return;
    }

    let items = match app.active_tab() {
        Tab::Devices | Tab::Scenes | Tab::Automations | Tab::Manage => Vec::new(),
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
    let title = app.active_tab().title().to_string();

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

fn draw_scenes_table(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let highlight = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let header_cells = ["  Scene", "Plugin", "Area / Room", "Status"]
        .iter()
        .map(|h| Cell::from(*h).style(
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    let header = Row::new(header_cells).height(1).bottom_margin(0);

    let rows: Vec<Row> = app
        .scenes
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_sel = i == app.selected;

            let plugin = s.plugin_id.as_deref()
                .map(|p| p.trim_start_matches("plugin.").to_string())
                .unwrap_or_else(|| "-".to_string());
            let area_str = s.area.clone().unwrap_or_else(|| "-".to_string());

            let (status_str, status_color) = match s.active {
                Some(true)  => ("● Active",   Color::Green),
                Some(false) => ("○ Inactive", Color::DarkGray),
                None        => ("-",          Color::DarkGray),
            };

            if is_sel {
                Row::new(vec![
                    Cell::from(format!("  {}", s.name)).style(highlight),
                    Cell::from(plugin).style(highlight),
                    Cell::from(area_str).style(highlight),
                    Cell::from(status_str).style(highlight),
                ])
            } else {
                Row::new(vec![
                    Cell::from(format!("  {}", s.name))
                        .style(Style::default().fg(Color::White)),
                    Cell::from(plugin)
                        .style(Style::default().fg(Color::DarkGray)),
                    Cell::from(area_str)
                        .style(Style::default().fg(Color::Gray)),
                    Cell::from(status_str)
                        .style(Style::default().fg(status_color)),
                ])
            }
        })
        .collect();

    let title = format!("Scenes [{}]", app.scenes.len());
    let table = Table::new(
        rows,
        [
            Constraint::Min(30),
            Constraint::Length(16),
            Constraint::Length(20),
            Constraint::Length(12),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(title))
    .row_highlight_style(highlight);

    let mut state = ratatui::widgets::TableState::default();
    if !app.scenes.is_empty() {
        state.select(Some(app.selected));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

// ── Areas Tab (Two-Pane) ──────────────────────────────────────────────────────

fn draw_areas_pane(frame: &mut Frame<'_>, app: &App, area: Rect) {
    // Split into left (40%) and right (60%) panes
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    draw_areas_list(frame, app, panes[0]);
    draw_area_devices(frame, app, panes[1]);
}

fn draw_areas_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    use crate::app::AreasPane;

    let highlight = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let is_focused = matches!(app.areas_pane_focus, AreasPane::AreasList);
    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let items: Vec<ListItem> = app
        .areas
        .iter()
        .map(|a| {
            let count = a.device_ids.len();
            let dev_label = if count == 1 { "device" } else { "devices" };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {:<24}", a.name),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("[{} {}]", count, dev_label),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let list = if is_focused {
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Areas")
                    .border_style(border_style),
            )
            .highlight_style(highlight)
            .highlight_symbol(">> ")
    } else {
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Areas")
                    .border_style(border_style),
            )
            .highlight_symbol(">> ")
    };

    let mut state = ratatui::widgets::ListState::default();
    if !app.areas.is_empty() && is_focused {
        state.select(Some(app.areas_list_selected));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_area_devices(frame: &mut Frame<'_>, app: &App, area: Rect) {
    use crate::app::AreasPane;

    let highlight = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let is_focused = matches!(app.areas_pane_focus, AreasPane::DeviceList);
    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = if let Some(area_id) = &app.areas_selected_area_id {
        app.areas
            .iter()
            .find(|a| &a.id == area_id)
            .map(|a| format!("Devices in \"{}\"", a.name))
            .unwrap_or_else(|| "Devices".to_string())
    } else {
        "Select an area".to_string()
    };

    if app.areas_selected_area_id.is_none() {
        let msg = Paragraph::new("← Select an area to view devices")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title.clone())
                    .border_style(border_style),
            )
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    let area_id = app.areas_selected_area_id.as_ref().unwrap();
    let device_ids = app
        .areas
        .iter()
        .find(|a| &a.id == area_id)
        .map(|a| a.device_ids.clone())
        .unwrap_or_default();

    let items: Vec<ListItem> = app
        .devices
        .iter()
        .filter(|d| device_ids.contains(&d.device_id))
        .enumerate()
        .map(|(_i, d)| {
            let is_selected = app.areas_selected_devices.contains(&d.device_id);
            let sel_mark = if is_selected { "[✓]" } else { "[ ]" };
            let (avail_dot, avail_color) = if d.available {
                ("●", Color::Green)
            } else {
                ("○", Color::Red)
            };
            let status = app.device_status(d);

            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", sel_mark), Style::default().fg(Color::Yellow)),
                Span::styled(format!("{} ", avail_dot), Style::default().fg(avail_color)),
                Span::styled(
                    format!("{:<20}", d.name),
                    Style::default().fg(Color::White),
                ),
                Span::styled(format!(" [{}]", status), Style::default().fg(Color::Gray)),
            ]))
        })
        .collect();

    let list = if is_focused {
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title.clone())
                    .border_style(border_style),
            )
            .highlight_style(highlight)
            .highlight_symbol(">> ")
    } else {
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title.clone())
                    .border_style(border_style),
            )
            .highlight_symbol(">> ")
    };

    let mut state = ratatui::widgets::ListState::default();
    if !device_ids.is_empty() && is_focused {
        let visible_devices = app
            .devices
            .iter()
            .filter(|d| device_ids.contains(&d.device_id))
            .count();
        if visible_devices > 0 {
            state.select(Some(app.areas_devices_selected.min(visible_devices - 1)));
        }
    }
    frame.render_stateful_widget(list, area, &mut state);
}

// ── Automations tab ───────────────────────────────────────────────────────────

fn draw_automations_tab(frame: &mut Frame<'_>, app: &App, area: Rect) {
    // Split: if history pane is open, divide horizontally 60/40
    let (list_area, history_area) = if app.fire_history_open {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);
        (panes[0], Some(panes[1]))
    } else {
        (area, None)
    };

    draw_automations_list(frame, app, list_area);

    if let Some(ha) = history_area {
        draw_automation_history(frame, app, ha);
    }
}

fn draw_automations_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let visible = app.visible_automations();
    let total = app.automations.len();
    let filtered = visible.len();
    let filter_active = app.automation_filter_stale
        || !app.automation_filter_tag.is_empty()
        || !app.automation_filter_trigger.is_empty();
    let bulk_count = app.automation_selected_ids.len();

    let title = if filter_active {
        format!("Automations [{}/{}]", filtered, total)
    } else if bulk_count > 0 {
        format!("Automations [{}] ({} selected)", total, bulk_count)
    } else {
        format!("Automations [{}]", total)
    };

    let highlight_sel = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let highlight_bulk = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

    let header_cells = ["Sel", "●", "Name", "Trigger", "Tags", "Pri"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1).bottom_margin(0);

    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let is_sel = i == app.selected;
            let is_bulk = app.automation_selected_ids.contains(&r.id);
            let is_stale = r.error.is_some();

            let sel_mark = if is_bulk { "[✓]" } else { "[ ]" };
            let dot = if r.enabled { "●" } else { "○" };
            let name_prefix = if is_stale { "⚠ " } else { "  " };
            let name_display: String = format!("{}{}", name_prefix, r.name).chars().take(32).collect();
            let trigger_str = r.trigger
                .as_ref()
                .and_then(|t| t.get("type").and_then(|v| v.as_str()))
                .unwrap_or("-")
                .to_string();
            let tags_str: String = r.tags.join(",").chars().take(20).collect();
            let pri_str = r.priority.to_string();

            let base_style = if is_stale {
                Style::default().fg(Color::Red)
            } else if r.enabled {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let dot_color = if r.enabled { Color::Green } else { Color::DarkGray };

            if is_sel {
                Row::new(vec![
                    Cell::from(sel_mark).style(highlight_sel),
                    Cell::from(dot).style(highlight_sel),
                    Cell::from(name_display).style(highlight_sel),
                    Cell::from(trigger_str).style(highlight_sel),
                    Cell::from(tags_str).style(highlight_sel),
                    Cell::from(pri_str).style(highlight_sel),
                ])
            } else if is_bulk {
                Row::new(vec![
                    Cell::from(sel_mark).style(highlight_bulk),
                    Cell::from(dot).style(Style::default().fg(dot_color)),
                    Cell::from(name_display).style(base_style.add_modifier(Modifier::BOLD)),
                    Cell::from(trigger_str).style(base_style),
                    Cell::from(tags_str).style(base_style),
                    Cell::from(pri_str).style(base_style),
                ])
            } else {
                Row::new(vec![
                    Cell::from(sel_mark).style(Style::default().fg(Color::DarkGray)),
                    Cell::from(dot).style(Style::default().fg(dot_color)),
                    Cell::from(name_display).style(base_style),
                    Cell::from(trigger_str).style(Style::default().fg(Color::DarkGray)),
                    Cell::from(tags_str).style(Style::default().fg(Color::Cyan)),
                    Cell::from(pri_str).style(Style::default().fg(Color::DarkGray)),
                ])
            }
        })
        .collect();

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
    .row_highlight_style(highlight_sel);

    let mut state = ratatui::widgets::TableState::default();
    if !visible.is_empty() {
        state.select(Some(app.selected));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_automation_history(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let rule_name = app
        .fire_history_rule_id
        .as_deref()
        .and_then(|id| app.automations.iter().find(|r| r.id == id))
        .map(|r| r.name.as_str())
        .unwrap_or("?");

    let title = format!("Fire History: {}", rule_name);

    if app.fire_history.is_empty() {
        let msg = Paragraph::new("No fire history found.")
            .block(Block::default().borders(Borders::ALL).title(title))
            .alignment(Alignment::Center);
        frame.render_widget(msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .fire_history
        .iter()
        .map(|f| {
            let ts: String = f.timestamp.chars().take(19).collect::<String>().replace('T', " ");
            let pass_str = if f.conditions_passed { "✓" } else { "✗" };
            let pass_color = if f.conditions_passed { Color::Green } else { Color::Red };
            let eval_str = format!("{}ms", f.eval_ms);
            ListItem::new(Line::from(vec![
                Span::styled(format!("{pass_str} "), Style::default().fg(pass_color)),
                Span::styled(format!("{ts} "), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("act={} ", f.actions_ran), Style::default().fg(Color::Cyan)),
                Span::styled(eval_str, Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title));
    frame.render_widget(list, area);
}

fn draw_delete_confirm(frame: &mut Frame<'_>, confirm: &DeleteConfirm) {
    let popup = centered_rect(60, 25, frame.area());
    frame.render_widget(Clear, popup);

    let outer = Block::default()
        .borders(Borders::ALL)
        .title("Confirm Delete")
        .border_style(Style::default().fg(Color::Red));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    let msg = Paragraph::new(format!("Delete rule: \"{}\"?", confirm.rule_name))
        .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center);
    frame.render_widget(msg, layout[0]);

    let sub = Paragraph::new(format!("ID: {}", confirm.rule_id))
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(sub, layout[1]);

    let help = Paragraph::new("Y confirm  |  N/Esc cancel")
        .style(Style::default().fg(Color::Yellow))
        .alignment(Alignment::Center);
    frame.render_widget(help, layout[2]);
}

fn draw_groups_overlay(frame: &mut Frame<'_>, app: &App) {
    let popup = centered_rect(60, 60, frame.area());
    frame.render_widget(Clear, popup);

    let outer = Block::default()
        .borders(Borders::ALL)
        .title("Automation Groups")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);

    let highlight = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    if app.groups.is_empty() {
        let msg = Paragraph::new("No groups defined.")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, layout[0]);
    } else {
        let items: Vec<ListItem> = app
            .groups
            .iter()
            .map(|g| {
                let rule_count = g.rule_ids.len();
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {:<28}", g.name), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("{} rule(s)", rule_count), Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default())
            .highlight_style(highlight)
            .highlight_symbol(">> ");
        let mut state = ratatui::widgets::ListState::default();
        if !app.groups.is_empty() {
            state.select(Some(app.groups_selected));
        }
        frame.render_stateful_widget(list, layout[0], &mut state);
    }

    let help = Paragraph::new("e enable  |  d disable  |  x delete  |  Esc close")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, layout[1]);
}

fn draw_filter_bar(frame: &mut Frame<'_>, filter_bar: &AutomationFilterBar) {
    let area = frame.area();
    // Draw a small bar at the bottom of the screen (above the status bar)
    let bar_area = Rect {
        x: area.x,
        y: area.height.saturating_sub(6).max(area.y),
        width: area.width,
        height: 3,
    };
    frame.render_widget(Clear, bar_area);

    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(40), Constraint::Percentage(20)])
        .split(bar_area);

    let tag_style = if filter_bar.active_field == AutomationFilterField::Tag {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };
    let trigger_style = if filter_bar.active_field == AutomationFilterField::Trigger {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    let tag_field = Paragraph::new(filter_bar.tag.as_str())
        .block(Block::default().borders(Borders::ALL).title("Tag filter").border_style(tag_style));
    frame.render_widget(tag_field, layout[0]);

    let trigger_field = Paragraph::new(filter_bar.trigger.as_str())
        .block(Block::default().borders(Borders::ALL).title("Trigger filter").border_style(trigger_style));
    frame.render_widget(trigger_field, layout[1]);

    let stale_str = if filter_bar.stale { "ON " } else { "off" };
    let stale_color = if filter_bar.stale { Color::Red } else { Color::DarkGray };
    let stale_field = Paragraph::new(Span::styled(stale_str, Style::default().fg(stale_color)))
        .block(Block::default().borders(Borders::ALL).title("Stale only"));
    frame.render_widget(stale_field, layout[2]);
}

fn draw_log_module_input(frame: &mut Frame<'_>, app: &App) {
    let popup = centered_rect(60, 20, frame.area());
    frame.render_widget(Clear, popup);

    let outer = Block::default()
        .borders(Borders::ALL)
        .title("Module / Target Filter")
        .border_style(Style::default().fg(Color::Yellow));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(inner);

    let input = Paragraph::new(app.log_module_input.as_str())
        .block(Block::default().borders(Borders::ALL).title("Filter string").border_style(Style::default().fg(Color::Yellow)));
    frame.render_widget(input, layout[0]);

    let help = Paragraph::new("Enter apply  |  Esc cancel")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, layout[1]);
}

// ── Logs tab ──────────────────────────────────────────────────────────────────

fn draw_logs_tab(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(3)])
        .split(area);

    // Header: level filter tabs + status
    draw_logs_header(frame, app, layout[0]);
    draw_logs_body(frame, app, layout[1]);
}

fn draw_logs_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let level_labels: Vec<Line> = vec![
        Line::from(Span::styled("e ERROR", if app.log_level_filter == LogLevelFilter::Error {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        })),
        Line::from(Span::styled("w WARN", if app.log_level_filter == LogLevelFilter::Warn {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        })),
        Line::from(Span::styled("i INFO", if app.log_level_filter == LogLevelFilter::Info {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        })),
        Line::from(Span::styled("d DEBUG", if app.log_level_filter == LogLevelFilter::Debug {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        })),
    ];

    let selected_idx = match app.log_level_filter {
        LogLevelFilter::Error => 0,
        LogLevelFilter::Warn  => 1,
        LogLevelFilter::Info  => 2,
        LogLevelFilter::Debug => 3,
    };

    let live_dot = if app.log_ws_connected { "●" } else { "○" };
    let live_color = if app.log_ws_connected { Color::Green } else { Color::Red };
    let pause_str = if app.log_paused { " [PAUSED]" } else { " [LIVE]" };
    let module_str = if app.log_module_filter.is_empty() {
        String::new()
    } else {
        format!(" module={}", app.log_module_filter)
    };

    let title = format!(
        "Logs {} | {} lines{}{}",
        Span::styled(live_dot, Style::default().fg(live_color)).content,
        app.log_lines.len(),
        pause_str,
        module_str
    );

    let tabs = Tabs::new(level_labels)
        .select(selected_idx)
        .block(Block::default().borders(Borders::ALL).title(title))
        .style(Style::default().fg(Color::Gray))
        .highlight_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, area);
}

fn draw_logs_body(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let height = area.height.saturating_sub(2) as usize;
    let total = app.log_lines.len();

    // Calculate visible window
    let start = if total > height {
        let max_start = total - height;
        app.log_scroll_offset.min(max_start)
    } else {
        0
    };
    let end = (start + height).min(total);

    let items: Vec<ListItem> = app
        .log_lines
        .iter()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(|line| {
            let level_upper = line.level.to_uppercase();
            let (level_color, level_mod) = match level_upper.as_str() {
                "ERROR" => (Color::Red, Modifier::BOLD),
                "WARN"  => (Color::Yellow, Modifier::empty()),
                "INFO"  => (Color::Cyan, Modifier::empty()),
                _       => (Color::DarkGray, Modifier::empty()),
            };
            let ts: String = line.timestamp.chars().skip(11).take(8).collect();
            let target_short: String = line.target.chars().take(24).collect();

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("[{:<5}] ", &level_upper),
                    Style::default().fg(level_color).add_modifier(level_mod),
                ),
                Span::styled(format!("{ts} "), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<24} ", target_short), Style::default().fg(Color::Blue)),
                Span::raw(line.message.clone()),
            ]))
        })
        .collect();

    let block_title = if total > height {
        format!("Log lines (scroll: {}/{})", start + 1, total.saturating_sub(height - 1))
    } else {
        "Log lines".to_string()
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(block_title));
    frame.render_widget(list, area);
}

// ── System Status tab ─────────────────────────────────────────────────────────

fn draw_status_tab(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("System Status");

    let Some(status) = app.system_status.as_ref() else {
        let loading_msg = if app.system_status_last_refresh.is_none() {
            "Press r to load system status."
        } else {
            "Loading..."
        };
        let msg = Paragraph::new(loading_msg)
            .block(block)
            .alignment(Alignment::Center);
        frame.render_widget(msg, area);
        return;
    };

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Two-column layout
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    // Left column: core stats
    let uptime_secs = status.uptime_seconds;
    let uptime_str = format_uptime(uptime_secs);
    let started_at_str: String = status.started_at.chars().take(19).collect::<String>().replace('T', " ");

    let left_refresh = app.system_status_last_refresh.as_deref().unwrap_or("never");
    let time_label = if app.time_utc { "(UTC)" } else { "(local)" };

    let mut left_lines: Vec<Line> = vec![
        Line::from(Span::styled("System", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED))),
        Line::from(""),
        status_detail_row("Version", &status.version),
        status_detail_row("Started", &started_at_str),
        status_detail_row("Uptime", &uptime_str),
        status_detail_row("Last refresh", &format!("{} {}", left_refresh, time_label)),
        Line::from(""),
        Line::from(Span::styled("Automations", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED))),
        Line::from(""),
        status_detail_row("Total rules", &status.rules_total.to_string()),
        status_detail_row("Enabled", &status.rules_enabled.to_string()),
    ];

    if status.rules_total > 0 {
        let disabled = status.rules_total.saturating_sub(status.rules_enabled);
        left_lines.push(status_detail_row("Disabled", &disabled.to_string()));
    }

    let left = Paragraph::new(left_lines).wrap(Wrap { trim: false });
    frame.render_widget(left, cols[0]);

    // Right column: devices, plugins, storage
    let state_db_kb = status.state_db_bytes / 1024;
    let history_db_kb = status.history_db_bytes / 1024;

    let right_lines: Vec<Line> = vec![
        Line::from(Span::styled("Devices & Plugins", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED))),
        Line::from(""),
        status_detail_row("Total devices", &status.devices_total.to_string()),
        status_detail_row("Active plugins", &status.plugins_active.to_string()),
        Line::from(""),
        Line::from(Span::styled("Storage", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED))),
        Line::from(""),
        status_detail_row("State DB", &format!("{} KB", state_db_kb)),
        status_detail_row("History DB", &format!("{} KB", history_db_kb)),
    ];

    let right = Paragraph::new(right_lines).wrap(Wrap { trim: false });
    frame.render_widget(right, cols[1]);
}

fn status_detail_row(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:<16}", label),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(value.to_string(), Style::default().fg(Color::White)),
    ])
}

fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    if days > 0 {
        format!("{}d {}h {}m", days, hours, mins)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
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
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let active_idx = match app.device_sub {
        DeviceSubPanel::All => 0,
        DeviceSubPanel::Switches => 1,
        DeviceSubPanel::Timers => 2,
    };
    let sub_tabs = Tabs::new(vec![
        Line::from("All"),
        Line::from("Switches"),
        Line::from("Timers"),
    ])
    .select(active_idx)
    .block(Block::default().borders(Borders::ALL).title("Devices"))
    .style(Style::default().fg(Color::Gray))
    .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    frame.render_widget(sub_tabs, layout[0]);

    match app.device_sub {
        DeviceSubPanel::All => {
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(layout[1]);
            draw_device_list(frame, app, panes[0]);
            draw_device_detail(frame, app, panes[1]);
        }
        DeviceSubPanel::Switches => {
            draw_switches_list(frame, app, layout[1]);
        }
        DeviceSubPanel::Timers => {
            draw_timers_list(frame, app, layout[1]);
        }
    }
}

fn draw_device_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (items, render_selected) = if app.view_mode == DeviceViewMode::Grouped {
        build_grouped_list(app)
    } else {
        build_flat_list(app)
    };

    let mode_label = if app.view_mode == DeviceViewMode::Grouped { "Grouped" } else { "Flat" };
    let title = format!("Devices ({mode_label}) [{}]", app.visible_devices().len());

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));

    let mut state = ratatui::widgets::ListState::default();
    state.select(render_selected);
    frame.render_stateful_widget(list, area, &mut state);
}

fn build_grouped_list(app: &App) -> (Vec<ListItem<'static>>, Option<usize>) {
    let visible = app.visible_devices();
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
            let Some(device) = visible.get(dev_idx) else {
                flat_idx += 1;
                render_idx += 1;
                continue;
            };
            let device = *device;

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
    let visible = app.visible_devices();
    let items = visible
        .iter()
        .enumerate()
        .map(|(i, device)| device_list_row(app, device, i == app.selected, false))
        .collect();
    let selected = if visible.is_empty() { None } else { Some(app.selected) };
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
            let remaining_ms = timer_remaining_secs(device) * 1000;
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
                format_timestamp_utc(&device.last_seen, app.time_utc),
                Style::default().fg(Color::DarkGray),
            )],
        ));
    }

    lines.push(Line::from(""));

    // Timer detail (core.timer devices)
    if device.plugin_id == "core.timer" {
        let timer_state = device.attributes.get("state").and_then(|v| v.as_str()).unwrap_or("idle");
        let duration_ms = device.attributes.get("duration_secs").and_then(|v| v.as_u64()).unwrap_or(0) * 1000;
        let remaining_ms = timer_remaining_secs(device) * 1000;
        let repeat = device.attributes.get("repeat").and_then(|v| v.as_bool()).unwrap_or(false);

        let state_color = match timer_state {
            "running"   => Color::Green,
            "paused"    => Color::Yellow,
            "finished"  => Color::Cyan,
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
        "battery", "battery_level", "battery_percent", "battery_pct", "battery_state", "battery_low",
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

fn draw_switches_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let highlight = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let items: Vec<ListItem> = app
        .switches
        .iter()
        .map(|s| {
            let on = s.attributes.get("on").and_then(|v| v.as_bool()).unwrap_or(false);
            let (dot, dot_color) = if on { ("●", Color::Green) } else { ("○", Color::DarkGray) };
            let label = if s.name != s.device_id { format!("  {}", s.name) } else { String::new() };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {dot} "), Style::default().fg(dot_color)),
                Span::styled(format!("{:<36}", s.device_id), Style::default().fg(Color::White)),
                Span::styled(label, Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!("Switches [{}]", app.switches.len())))
        .highlight_style(highlight)
        .highlight_symbol(">> ");

    let mut state = ratatui::widgets::ListState::default();
    if !app.switches.is_empty() {
        state.select(Some(app.selected.min(app.switches.len() - 1)));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_timers_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let highlight = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let items: Vec<ListItem> = app
        .timers
        .iter()
        .map(|t| {
            let state = t.attributes.get("state").and_then(|v| v.as_str()).unwrap_or("idle");
            let state_color = match state {
                "running"  => Color::Green,
                "finished" => Color::Yellow,
                "paused"   => Color::Cyan,
                _          => Color::DarkGray,
            };
            let label = if t.name != t.device_id { format!("  {}", t.name) } else { String::new() };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {:<36}", t.device_id), Style::default().fg(Color::White)),
                Span::styled(format!("{:<10}", state), Style::default().fg(state_color)),
                Span::styled(label, Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!("Timers [{}]", app.timers.len())))
        .highlight_style(highlight)
        .highlight_symbol(">> ");

    let mut state = ratatui::widgets::ListState::default();
    if !app.timers.is_empty() {
        state.select(Some(app.selected.min(app.timers.len() - 1)));
    }
    frame.render_stateful_widget(list, area, &mut state);
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
        "On" | "Open" | "Unlocked" | "Online" | "Occupied" => Color::Green,
        "Off" | "Closed" | "Locked" | "Offline" | "Vacant" => Color::Red,
        "Unknown" | "—" => Color::DarkGray,
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

/// Compute a running timer's remaining seconds locally from started_at + duration_secs
/// so the countdown ticks on every redraw without a network request.
/// For paused/idle/finished timers falls back to the stored remaining_secs.
fn timer_remaining_secs(device: &DeviceState) -> u64 {
    let is_running = device.attributes.get("state").and_then(|v| v.as_str()) == Some("running");
    if is_running {
        let duration = device.attributes.get("duration_secs").and_then(|v| v.as_u64()).unwrap_or(0);
        let started_at = device
            .attributes
            .get("started_at")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        if let Some(started) = started_at {
            let elapsed = (Utc::now() - started).num_seconds().max(0) as u64;
            return duration.saturating_sub(elapsed);
        }
    }
    device.attributes.get("remaining_secs").and_then(|v| v.as_u64()).unwrap_or(0)
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
        Tab::Automations => app.visible_automations().is_empty(),
        Tab::Plugins => app.plugins.is_empty(),
        Tab::Manage => false,
    }
}

fn draw_manage_tab(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let active_idx = match app.admin_sub {
        AdminSubPanel::Modes => 0,
        AdminSubPanel::Status => 1,
        AdminSubPanel::Users => 2,
        AdminSubPanel::Logs => 3,
        AdminSubPanel::Events => 4,
    };
    let sub_tabs = Tabs::new(vec![
        Line::from("Modes"),
        Line::from("Status"),
        Line::from("Users"),
        Line::from("Logs"),
        Line::from("Events"),
    ])
    .select(active_idx)
    .block(Block::default().borders(Borders::ALL).title("Manage"))
    .style(Style::default().fg(Color::Gray))
    .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    frame.render_widget(sub_tabs, layout[0]);

    let highlight = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    if matches!(app.admin_sub, AdminSubPanel::Status) {
        draw_status_tab(frame, app, layout[1]);
        return;
    }

    if matches!(app.admin_sub, AdminSubPanel::Users) {
        draw_users_list(frame, app, layout[1]);
        return;
    }

    if matches!(app.admin_sub, AdminSubPanel::Logs) {
        draw_logs_tab(frame, app, layout[1]);
        return;
    }

    if matches!(app.admin_sub, AdminSubPanel::Events) {
        let items = app
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
            .collect::<Vec<_>>();
        let title = format!("Events [{}]", app.events_filter_mode.title());
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(highlight)
            .highlight_symbol(">> ");
        let mut state = ratatui::widgets::ListState::default();
        if !app.filtered_events().is_empty() {
            state.select(Some(app.selected.min(app.filtered_events().len() - 1)));
        }
        frame.render_stateful_widget(list, layout[1], &mut state);
        return;
    }

    let (items, title, len): (Vec<ListItem<'_>>, &str, usize) = match app.admin_sub {
        AdminSubPanel::Modes => {
            let items = app.modes.iter().map(|m| {
                let on = m.state.as_ref()
                    .and_then(|s| s.attributes.get("on"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let (dot, dot_color) = if on { ("●", Color::Green) } else { ("○", Color::DarkGray) };
                let kind_color = match m.config.kind.as_str() {
                    "solar"  => Color::Yellow,
                    "manual" => Color::Cyan,
                    _        => Color::White,
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {dot} "), Style::default().fg(dot_color)),
                    Span::styled(format!("{:<28}", m.config.id), Style::default().fg(Color::White)),
                    Span::styled(format!("  {:<8}", m.config.kind), Style::default().fg(kind_color)),
                    Span::styled(format!("  {}", m.config.name), Style::default().fg(Color::DarkGray)),
                ]))
            }).collect();
            let len = app.modes.len();
            (items, "Modes", len)
        }
        AdminSubPanel::Status => (Vec::new(), "Status", 0),
        AdminSubPanel::Users => (Vec::new(), "Users", 0),
        AdminSubPanel::Logs => (Vec::new(), "Logs", 0),
        AdminSubPanel::Events => (Vec::new(), "Events", 0),
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(highlight)
        .highlight_symbol(">> ");
    let mut state = ratatui::widgets::ListState::default();
    if len > 0 {
        state.select(Some(app.selected.min(len - 1)));
    }
    frame.render_stateful_widget(list, layout[1], &mut state);
}

fn draw_users_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let highlight = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let items: Vec<ListItem> = app
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
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Users"))
        .highlight_style(highlight)
        .highlight_symbol(">> ");
    let mut state = ratatui::widgets::ListState::default();
    let len = app.users.len();
    if len > 0 {
        state.select(Some(app.selected.min(len - 1)));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_switch_editor(frame: &mut Frame<'_>, editor: &SwitchEditor) {
    let popup = centered_rect(64, 40, frame.area());
    frame.render_widget(Clear, popup);
    let outer = Block::default().borders(Borders::ALL).title("New Switch");
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let focused = Style::default().fg(Color::Yellow);
    let normal  = Style::default();

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Min(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(editor.id.as_str()).block(
            Block::default().borders(Borders::ALL)
                .title("ID  (switch_ prefix added automatically)")
                .border_style(if editor.field == SwitchEditField::Id { focused } else { normal })
        ),
        layout[0],
    );
    frame.render_widget(
        Paragraph::new(editor.label.as_str()).block(
            Block::default().borders(Borders::ALL).title("Label  (optional)")
                .border_style(if editor.field == SwitchEditField::Label { focused } else { normal })
        ),
        layout[1],
    );
    frame.render_widget(
        Paragraph::new("Tab field  |  Enter create  |  Esc cancel")
            .alignment(Alignment::Center),
        layout[2],
    );
}

fn draw_timer_editor(frame: &mut Frame<'_>, editor: &TimerEditor) {
    let popup = centered_rect(64, 40, frame.area());
    frame.render_widget(Clear, popup);
    let outer = Block::default().borders(Borders::ALL).title("New Timer");
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let focused = Style::default().fg(Color::Yellow);
    let normal  = Style::default();

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Min(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(editor.id.as_str()).block(
            Block::default().borders(Borders::ALL)
                .title("ID  (timer_ prefix added automatically)")
                .border_style(if editor.field == TimerEditField::Id { focused } else { normal })
        ),
        layout[0],
    );
    frame.render_widget(
        Paragraph::new(editor.label.as_str()).block(
            Block::default().borders(Borders::ALL).title("Label  (optional)")
                .border_style(if editor.field == TimerEditField::Label { focused } else { normal })
        ),
        layout[1],
    );
    frame.render_widget(
        Paragraph::new("Tab field  |  Enter create  |  Esc cancel")
            .alignment(Alignment::Center),
        layout[2],
    );
}

fn draw_mode_editor(frame: &mut Frame<'_>, editor: &ModeEditor) {
    let popup = centered_rect(64, 50, frame.area());
    frame.render_widget(Clear, popup);
    let outer = Block::default().borders(Borders::ALL).title("New Mode");
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let focused = Style::default().fg(Color::Yellow);
    let normal  = Style::default();

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(editor.id.as_str()).block(
            Block::default().borders(Borders::ALL).title("ID  (must start with mode_)")
                .border_style(if editor.field == ModeEditField::Id { focused } else { normal })
        ),
        layout[0],
    );
    frame.render_widget(
        Paragraph::new(editor.name.as_str()).block(
            Block::default().borders(Borders::ALL).title("Name")
                .border_style(if editor.field == ModeEditField::Name { focused } else { normal })
        ),
        layout[1],
    );
    let kind_color = match editor.kind {
        ModeKind::Solar  => Color::Yellow,
        ModeKind::Manual => Color::Cyan,
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!("{}  (Space to toggle)", editor.kind.as_str()),
            Style::default().fg(kind_color),
        )).block(
            Block::default().borders(Borders::ALL).title("Kind")
                .border_style(if editor.field == ModeEditField::Kind { focused } else { normal })
        ),
        layout[2],
    );
    frame.render_widget(
        Paragraph::new("Tab field  |  Space cycle kind  |  Enter create  |  Esc cancel")
            .alignment(Alignment::Center),
        layout[3],
    );
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
