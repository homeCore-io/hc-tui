use crate::api::DeviceState;
use crate::app::{
    AdminSubPanel, App, AreaEditor, DeleteConfirm, DeviceEditField, DeviceSubPanel, DeviceViewMode,
    FocusField, GlueCreator, GlueEditField, LogLevelFilter, LoginPhase, MatterCommissionEditor,
    MatterCommissionField, ModeEditField, ModeEditor, ModeKind, PluginDetailPanel, RuleFilterBar,
    RuleFilterField, StreamingAction, StreamingStage, SwitchEditField, SwitchEditor, Tab,
    TimerEditField, TimerEditor, UserEditField, UserEditMode, UserEditor, format_timestamp_utc,
};
use chrono::Utc;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, Gauge, List, ListItem, Paragraph, Row, Table, Tabs, Wrap,
    },
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
        .constraints([Constraint::Min(5), Constraint::Length(footer_height)])
        .split(frame.area());

    // Split content area into left menu and right content
    let content_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20), // Fixed width for left menu
            Constraint::Min(10),    // Remaining space for content
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
    if let Some(creator) = app.glue_creator.as_ref() {
        draw_glue_creator(frame, creator);
    }
    if let Some(editor) = app.matter_commission_editor.as_ref() {
        draw_matter_commission_editor(frame, editor);
    }
    // Rule overlays (drawn on top of everything)
    if let Some(confirm) = app.rule_delete_confirm.as_ref() {
        draw_delete_confirm(frame, confirm);
    }
    if app.streaming_action.is_some() {
        draw_streaming_action_modal(frame, app);
    }
    if app.groups_open {
        draw_groups_overlay(frame, app);
    }
    if let Some(filter_bar) = app.rule_filter_bar.as_ref() {
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

    let menu_list =
        List::new(menu_items).block(Block::default().borders(Borders::RIGHT).title("Menu"));
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
    let mut hints = vec![
        "Tab/Shift+Tab menu",
        "1-9 jump tab",
        "j/k move",
        "r refresh",
        "q quit",
        "T time",
    ];
    match app.active_tab() {
        Tab::Devices => {
            hints.push("◄/► panel");
            match app.device_sub {
                DeviceSubPanel::All => {
                    hints.push("Spc toggle");
                    hints.push("t toggle");
                    hints.push("+/- brightness");
                    hints.push("l/u lock");
                    hints.push("v grouped/flat");
                    hints.push("f filter");
                    hints.push("s sort");
                    hints.push("/ search");
                    hints.push("Enter edit");
                    hints.push("d delete");
                }
                DeviceSubPanel::MediaPlayers => {
                    hints.push("Spc/t play-stop");
                    hints.push("p play/pause");
                    hints.push("x stop");
                    hints.push("n next");
                    hints.push("b previous");
                    hints.push("+/- volume");
                    hints.push("m mute");
                    hints.push("Enter edit");
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
        Tab::Scenes => {
            hints.push("a activate");
        }
        Tab::Areas => {
            hints.push("n new area");
            hints.push("Enter rename");
            hints.push("◄/► pane");
            hints.push("Spc select dev");
            hints.push("+ add device");
            hints.push("- remove device");
            hints.push("d delete");
        }
        Tab::Plugins => {
            hints.push("d deregister");
        }
        Tab::Manage => {
            hints.push("◄/► panel");
            if matches!(app.admin_sub, AdminSubPanel::Matter) {
                hints.push("c commission");
                hints.push("r refresh");
                hints.push("i reinterview");
                hints.push("d remove");
            } else if matches!(app.admin_sub, AdminSubPanel::Status) {
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
            } else if matches!(app.admin_sub, AdminSubPanel::Audit) {
                hints.push("r refresh");
                hints.push("n next page");
                hints.push("p prev page");
                hints.push("Enter detail");
            } else if matches!(app.admin_sub, AdminSubPanel::Backup) {
                hints.push("Enter run");
            } else {
                hints.push("n new");
                hints.push("d delete");
            }
        }
        Tab::Rules => {
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
    if let Some(ref s) = app.streaming_action {
        if s.stage.is_terminal() {
            return vec!["Esc close"];
        }
        if s.pending_prompt.is_some() {
            return vec!["type response", "Enter send", "c cancel run", "Esc close"];
        }
        return vec!["c cancel", "Esc close"];
    }
    if app.rule_detail_open {
        return vec!["j/k scroll", "PgUp/PgDn", "r refresh", "Esc back", "q quit"];
    }
    if app.plugin_detail_open {
        return vec![
            "1/2/3 panel",
            "b discover",
            "p pair",
            "r refresh",
            "Esc close",
            "q quit",
        ];
    }
    if app.switch_editor.is_some() {
        return vec!["Tab field", "Enter create", "Esc cancel"];
    }
    if app.timer_editor.is_some() {
        return vec!["Tab field", "Enter create", "Esc cancel"];
    }
    if app.glue_creator.is_some() {
        return vec!["Tab field", "Space type", "Enter create", "Esc cancel"];
    }
    if app.mode_editor.is_some() {
        return vec!["Tab field", "Space kind", "Enter create", "Esc cancel"];
    }
    if app.matter_commission_editor.is_some() {
        return vec!["Tab field", "Enter commission", "Esc cancel"];
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

/// Login screen — centered card with wordmark, base-URL preview,
/// inline username + password fields, animated phase indicator,
/// distinct error banner, and a key-binding footer.
fn draw_login(frame: &mut Frame<'_>, app: &App) {
    // Modest dimensions — login is a focused interaction, not a full
    // settings dialog. Constraints below assume ≥ 24 rows.
    let popup = centered_rect_min(60, 22, 18, frame.area());
    frame.render_widget(Clear, popup);

    // Outer card with a teal-leaning accent. The block intentionally
    // has no title — the wordmark inside owns the identity.
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(LOGIN_ACCENT));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    // Vertical layout: wordmark, kicker, gap, server line, gap,
    // username, password, gap, status line, gap, footer hints.
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // wordmark
            Constraint::Length(1), // kicker
            Constraint::Length(1), // gap
            Constraint::Length(1), // server URL
            Constraint::Length(1), // gap
            Constraint::Length(3), // username
            Constraint::Length(3), // password
            Constraint::Length(1), // gap
            Constraint::Length(1), // status / spinner / error
            Constraint::Min(0),    // expand
            Constraint::Length(1), // footer hint
        ])
        .horizontal_margin(2)
        .vertical_margin(1)
        .split(inner);

    let wordmark = Paragraph::new(Line::from(vec![Span::styled(
        "homeCore",
        Style::default()
            .fg(LOGIN_ACCENT)
            .add_modifier(Modifier::BOLD),
    )]))
    .alignment(Alignment::Center);
    frame.render_widget(wordmark, layout[0]);

    let kicker = Paragraph::new(Span::styled(
        "T E R M I N A L",
        Style::default().fg(Color::DarkGray),
    ))
    .alignment(Alignment::Center);
    frame.render_widget(kicker, layout[1]);

    // Server URL pulled from the API client. Useful for catching
    // "wrong host" mistakes before the user types credentials.
    let server_line = Paragraph::new(Line::from(vec![
        Span::styled("server  ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.client.base_url(), Style::default().fg(Color::Gray)),
    ]))
    .alignment(Alignment::Center);
    frame.render_widget(server_line, layout[3]);

    // Username field — visible cursor when focused, dimmed
    // placeholder when empty.
    let username_focused = matches!(app.focus, FocusField::Username);
    let username_block = field_block("username", username_focused);
    let username_text = field_text(&app.username, username_focused, "type your username");
    let username = Paragraph::new(username_text).block(username_block);
    frame.render_widget(username, layout[5]);

    // Password — same shape, but mask + cursor placement.
    let password_focused = matches!(app.focus, FocusField::Password);
    let password_block = field_block("password", password_focused);
    let masked = if app.password.is_empty() && !password_focused {
        Line::from(Span::styled(
            "•••••••••",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        let mut spans: Vec<Span> = Vec::new();
        spans.push(Span::raw("•".repeat(app.password.len())));
        if password_focused {
            spans.push(Span::styled("▎", Style::default().fg(LOGIN_ACCENT)));
        }
        Line::from(spans)
    };
    let password = Paragraph::new(masked).block(password_block);
    frame.render_widget(password, layout[6]);

    // Status row: error banner if set, otherwise spinner/idle.
    if let Some(err) = app.error.as_ref() {
        let err_line = Paragraph::new(Line::from(vec![
            Span::styled(
                " ✗ ",
                Style::default()
                    .bg(Color::Red)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(err.clone(), Style::default().fg(Color::Red)),
        ]))
        .alignment(Alignment::Center);
        frame.render_widget(err_line, layout[8]);
    } else if app.login_in_progress {
        let (glyph, label) = match app.login_phase {
            LoginPhase::Authenticating => ("[*]", "authenticating"),
            LoginPhase::Synthesizing => ("[#]", "synthesizing local cache"),
        };
        let status = Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} ", app.login_spinner()),
                Style::default().fg(LOGIN_ACCENT),
            ),
            Span::styled(format!("{glyph} "), Style::default().fg(Color::DarkGray)),
            Span::styled(label, Style::default().fg(Color::Gray)),
        ]))
        .alignment(Alignment::Center);
        frame.render_widget(status, layout[8]);
    } else {
        let hint = Paragraph::new(Span::styled(
            "credentials are sent over /api/v1/auth/login",
            Style::default().fg(Color::DarkGray),
        ))
        .alignment(Alignment::Center);
        frame.render_widget(hint, layout[8]);
    }

    let footer = Paragraph::new(Line::from(vec![
        Span::styled("Tab", Style::default().fg(LOGIN_ACCENT)),
        Span::styled(" switch field  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Enter", Style::default().fg(LOGIN_ACCENT)),
        Span::styled(" sign in  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Esc", Style::default().fg(LOGIN_ACCENT)),
        Span::styled(" quit", Style::default().fg(Color::DarkGray)),
    ]))
    .alignment(Alignment::Center);
    frame.render_widget(footer, layout[10]);
}

/// Accent color used by the login screen — slight teal lean to match
/// the Leptos client's identity refresh palette without depending on
/// terminal RGB support.
const LOGIN_ACCENT: Color = Color::Cyan;

fn field_block(label: &'static str, focused: bool) -> Block<'static> {
    let style = if focused {
        Style::default()
            .fg(LOGIN_ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(style)
        .title(format!(" {label} "))
        .title_style(style)
}

fn field_text<'a>(value: &'a str, focused: bool, placeholder: &'static str) -> Line<'a> {
    if value.is_empty() && !focused {
        return Line::from(Span::styled(
            placeholder,
            Style::default().fg(Color::DarkGray),
        ));
    }
    let mut spans: Vec<Span<'a>> = Vec::new();
    spans.push(Span::raw(value));
    if focused {
        spans.push(Span::styled("▎", Style::default().fg(LOGIN_ACCENT)));
    }
    Line::from(spans)
}

/// Centered rectangle of fixed `width` (clamped to `[min_w, max_w]`
/// against `area.width`) and fixed `height` (clamped to `area.height`).
/// Fits gracefully on small terminals: when `area.width < min_w` it
/// uses whatever width is available rather than overflowing.
fn centered_rect_min(min_w: u16, max_w: u16, height: u16, area: Rect) -> Rect {
    let width = max_w.min(area.width).max(min_w.min(area.width));
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
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
    if matches!(app.active_tab(), Tab::Rules) {
        draw_rules_tab(frame, app, area);
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
        Tab::Devices | Tab::Scenes | Tab::Rules | Tab::Manage => Vec::new(),
        Tab::Areas => app
            .areas
            .iter()
            .map(|a| {
                let count = a.device_ids.len();
                let dev_label = if count == 1 { "device" } else { "devices" };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {:<28}", a.name),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
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
                    "active" => ("●", Color::Green),
                    "degraded" => ("●", Color::Yellow),
                    _ => ("○", Color::Red),
                };
                let ts = p
                    .registered_at
                    .chars()
                    .take(19)
                    .collect::<String>()
                    .replace('T', " ");
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {dot} "), Style::default().fg(dot_color)),
                    Span::styled(
                        format!("{:<30}", p.plugin_id),
                        Style::default().fg(Color::White),
                    ),
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
        .map(|h| {
            Cell::from(*h).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        });
    let header = Row::new(header_cells).height(1).bottom_margin(0);

    let rows: Vec<Row> = app
        .scenes
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_sel = i == app.selected;

            let plugin = s
                .plugin_id
                .as_deref()
                .map(|p| p.trim_start_matches("plugin.").to_string())
                .unwrap_or_else(|| "-".to_string());
            let area_str = s.area.clone().unwrap_or_else(|| "-".to_string());

            let (status_str, status_color) = match s.active {
                Some(true) => ("● Active", Color::Green),
                Some(false) => ("○ Inactive", Color::DarkGray),
                None => ("-", Color::DarkGray),
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
                    Cell::from(format!("  {}", s.name)).style(Style::default().fg(Color::White)),
                    Cell::from(plugin).style(Style::default().fg(Color::DarkGray)),
                    Cell::from(area_str).style(Style::default().fg(Color::Gray)),
                    Cell::from(status_str).style(Style::default().fg(status_color)),
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
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
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
        .map(|d| {
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
                Span::styled(format!("{:<20}", d.name), Style::default().fg(Color::White)),
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

// ── Rules tab ───────────────────────────────────────────────────────────

fn draw_rules_tab(frame: &mut Frame<'_>, app: &App, area: Rect) {
    // Read-only detail view takes over the whole tab pane when open —
    // mirrors the plugin_detail full-screen replacement pattern.
    if app.rule_detail_open {
        draw_rule_detail(frame, app, area);
        return;
    }

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

    draw_rules_list(frame, app, list_area);

    if let Some(ha) = history_area {
        draw_rule_history(frame, app, ha);
    }
}

/// Read-only rule detail view. Layout (top-down):
/// 1. Header — name, enabled badge, priority, tags, error banner
/// 2. RON pane (Min) — scrollable mono-style display of the on-disk
///    .ron file content
/// 3. Fire history pane (fixed Length) — last firings, oldest at the
///    bottom
fn draw_rule_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let rule = app
        .rule_detail_id
        .as_deref()
        .and_then(|id| app.rules.iter().find(|r| r.id == id));

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // RON pane (takes the rest)
            Constraint::Length(8), // fire history (fixed)
        ])
        .split(area);

    // Header.
    let header_lines: Vec<Line> = match rule {
        Some(r) => {
            let mut spans: Vec<Span> = Vec::new();
            spans.push(Span::styled(
                r.name.clone(),
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::White),
            ));
            spans.push(Span::raw("  "));
            spans.push(if r.enabled {
                Span::styled("● enabled", Style::default().fg(Color::Green))
            } else {
                Span::styled("○ disabled", Style::default().fg(Color::DarkGray))
            });
            spans.push(Span::raw(format!("  · priority {}", r.priority)));
            if !r.tags.is_empty() {
                spans.push(Span::raw(format!("  · tags [{}]", r.tags.join(","))));
            }
            let mut lines = vec![Line::from(spans)];
            if let Some(err) = r.error.as_ref() {
                lines.push(Line::from(Span::styled(
                    format!("error: {err}"),
                    Style::default().fg(Color::Red),
                )));
            } else if let Some(err) = app.rule_detail_error.as_ref() {
                lines.push(Line::from(Span::styled(
                    format!("warning: {err}"),
                    Style::default().fg(Color::Yellow),
                )));
            }
            lines
        }
        None => vec![Line::from(Span::styled(
            "rule no longer in list",
            Style::default().fg(Color::Yellow),
        ))],
    };
    let header =
        Paragraph::new(header_lines).block(Block::default().borders(Borders::ALL).title(" Rule "));
    frame.render_widget(header, layout[0]);

    // RON pane.
    let ron_block = Block::default()
        .borders(Borders::ALL)
        .title(" RON  (j/k scroll · r refresh · Esc back) ");
    let ron_text = if app.rule_detail_loading && app.rule_detail_ron.is_none() {
        "loading…".to_string()
    } else {
        app.rule_detail_ron
            .clone()
            .unwrap_or_else(|| "(no .ron file backing this rule)".to_string())
    };
    let ron = Paragraph::new(ron_text)
        .block(ron_block)
        .style(Style::default().fg(Color::Gray))
        .scroll((app.rule_detail_scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(ron, layout[1]);

    // Fire history pane.
    let history_items: Vec<ListItem> = match app.rule_detail_history.as_ref() {
        Some(h) if h.is_empty() => vec![ListItem::new(Line::from(Span::styled(
            "no recent firings",
            Style::default().fg(Color::DarkGray),
        )))],
        Some(h) => {
            let take = (layout[2].height.saturating_sub(2) as usize).max(1);
            // Newest first in the history vec; show newest at the top
            // and let older entries fall off if the pane is short.
            h.iter()
                .take(take)
                .map(|f| {
                    let ok_glyph = if f.conditions_passed {
                        Span::styled("✓", Style::default().fg(Color::Green))
                    } else {
                        Span::styled("✗", Style::default().fg(Color::Yellow))
                    };
                    Line::from(vec![
                        Span::raw(format_timestamp_utc(&f.timestamp, false)),
                        Span::raw("  "),
                        ok_glyph,
                        Span::raw(format!(
                            "  {} action{}  {}ms",
                            f.actions_ran,
                            if f.actions_ran == 1 { "" } else { "s" },
                            f.eval_ms
                        )),
                    ])
                })
                .map(ListItem::new)
                .collect()
        }
        None => vec![ListItem::new(Line::from(Span::styled(
            if app.rule_detail_loading {
                "loading…"
            } else {
                "(history unavailable)"
            },
            Style::default().fg(Color::DarkGray),
        )))],
    };
    let history_title = match app.rule_detail_history.as_ref() {
        Some(h) => format!(" Fire history ({}) ", h.len()),
        None => " Fire history ".to_string(),
    };
    let history =
        List::new(history_items).block(Block::default().borders(Borders::ALL).title(history_title));
    frame.render_widget(history, layout[2]);
}

fn draw_rules_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let visible = app.visible_rules();
    let total = app.rules.len();
    let filtered = visible.len();
    let filter_active = app.rule_filter_stale
        || !app.rule_filter_tag.is_empty()
        || !app.rule_filter_trigger.is_empty();
    let bulk_count = app.rule_selected_ids.len();

    let title = if filter_active {
        format!("Rules [{}/{}]", filtered, total)
    } else if bulk_count > 0 {
        format!("Rules [{}] ({} selected)", total, bulk_count)
    } else {
        format!("Rules [{}]", total)
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
        .map(|h| {
            Cell::from(*h).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        });
    let header = Row::new(header_cells).height(1).bottom_margin(0);

    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let is_sel = i == app.selected;
            let is_bulk = app.rule_selected_ids.contains(&r.id);
            let is_stale = r.error.is_some();

            let sel_mark = if is_bulk { "[✓]" } else { "[ ]" };
            let dot = if r.enabled { "●" } else { "○" };
            let name_prefix = if is_stale { "⚠ " } else { "  " };
            let name_display: String = format!("{}{}", name_prefix, r.name)
                .chars()
                .take(32)
                .collect();
            let trigger_str = r
                .trigger
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
            let dot_color = if r.enabled {
                Color::Green
            } else {
                Color::DarkGray
            };

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

fn draw_rule_history(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let rule_name = app
        .fire_history_rule_id
        .as_deref()
        .and_then(|id| app.rules.iter().find(|r| r.id == id))
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
            let ts: String = f
                .timestamp
                .chars()
                .take(19)
                .collect::<String>()
                .replace('T', " ");
            let pass_str = if f.conditions_passed { "✓" } else { "✗" };
            let pass_color = if f.conditions_passed {
                Color::Green
            } else {
                Color::Red
            };
            let eval_str = format!("{}ms", f.eval_ms);
            ListItem::new(Line::from(vec![
                Span::styled(format!("{pass_str} "), Style::default().fg(pass_color)),
                Span::styled(format!("{ts} "), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("act={} ", f.actions_ran),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(eval_str, Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
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
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(inner);

    let msg = Paragraph::new(format!("Delete rule: \"{}\"?", confirm.rule_name))
        .style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
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

/// Streaming-action modal. Layout (top-down):
/// 1. Header — action label + status pill
/// 2. Progress bar (when a `progress` event has arrived)
/// 3. Recent items list (when item events have arrived)
/// 4. Awaiting-user prompt with response input (when in AwaitingUser stage)
/// 5. Warnings list (when any)
/// 6. Terminal payload summary (when terminal stage reached)
/// 7. Footer with status text
fn draw_streaming_action_modal(frame: &mut Frame<'_>, app: &App) {
    let Some(state) = app.streaming_action.as_ref() else {
        return;
    };

    let popup = centered_rect(80, 70, frame.area());
    frame.render_widget(Clear, popup);

    let pill_color = match state.stage {
        StreamingStage::Starting => Color::Gray,
        StreamingStage::Running => Color::Cyan,
        StreamingStage::AwaitingUser => Color::Yellow,
        StreamingStage::Complete => Color::Green,
        StreamingStage::Error | StreamingStage::Timeout => Color::Red,
        StreamingStage::Canceled => Color::Magenta,
    };

    let title = format!(" {} — {} ", state.label, state.stage.label());
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(pill_color));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    // Decide which sub-blocks to allocate space for.
    let has_progress = state.last_progress.is_some();
    let has_items = !state.items.is_empty();
    let has_prompt = state.pending_prompt.is_some();
    let has_warnings = !state.warnings.is_empty();
    let has_terminal = state.terminal.is_some();

    let mut constraints: Vec<Constraint> = Vec::new();
    if has_progress {
        constraints.push(Constraint::Length(3));
    }
    if has_items {
        constraints.push(Constraint::Min(5));
    }
    if has_prompt {
        constraints.push(Constraint::Length(7));
    }
    if has_warnings {
        constraints.push(Constraint::Length(4));
    }
    if has_terminal {
        constraints.push(Constraint::Length(5));
    }
    // Footer always last.
    constraints.push(Constraint::Length(2));
    if constraints.len() == 1 {
        // No content yet — show a centered "starting" placeholder.
        constraints.insert(0, Constraint::Min(1));
    }

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let mut idx = 0;

    if !has_progress && !has_items && !has_prompt && !has_warnings && !has_terminal {
        let waiting = Paragraph::new(state.footer.as_str())
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(waiting, layout[idx]);
        idx += 1;
    }

    if has_progress {
        draw_streaming_progress(frame, layout[idx], state);
        idx += 1;
    }
    if has_items {
        draw_streaming_items(frame, layout[idx], state);
        idx += 1;
    }
    if has_prompt {
        draw_streaming_prompt(frame, layout[idx], state);
        idx += 1;
    }
    if has_warnings {
        draw_streaming_warnings(frame, layout[idx], state);
        idx += 1;
    }
    if has_terminal {
        draw_streaming_terminal(frame, layout[idx], state);
        idx += 1;
    }

    // Footer
    let footer = Paragraph::new(state.footer.as_str())
        .style(Style::default().fg(Color::Yellow))
        .alignment(Alignment::Left);
    frame.render_widget(footer, layout[idx]);
}

fn draw_streaming_progress(frame: &mut Frame<'_>, area: Rect, state: &StreamingAction) {
    let Some(progress) = state.last_progress.as_ref() else {
        return;
    };
    let pct = progress
        .get("pct")
        .and_then(serde_json::Value::as_f64)
        .map(|p| p.clamp(0.0, 100.0));
    let message = progress
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    if let Some(p) = pct {
        let label = if message.is_empty() {
            format!("{:>3.0}%", p)
        } else {
            format!("{:>3.0}% — {}", p, message)
        };
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title("Progress"))
            .gauge_style(Style::default().fg(Color::Cyan))
            .ratio((p / 100.0).clamp(0.0, 1.0))
            .label(label);
        frame.render_widget(gauge, area);
    } else {
        let para = Paragraph::new(message.to_string())
            .block(Block::default().borders(Borders::ALL).title("Progress"))
            .style(Style::default().fg(Color::Cyan));
        frame.render_widget(para, area);
    }
}

fn draw_streaming_items(frame: &mut Frame<'_>, area: Rect, state: &StreamingAction) {
    // Show the last N items (one line each). Older items scroll off the
    // top — the modal's job is "what's happening now", not full history.
    let take = area.height.saturating_sub(2) as usize;
    let start = state.items.len().saturating_sub(take);
    let items: Vec<ListItem> = state.items[start..]
        .iter()
        .map(|v| {
            let line = match v {
                serde_json::Value::Object(map) => map
                    .get("name")
                    .or_else(|| map.get("id"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| serde_json::to_string(v).unwrap_or_default()),
                serde_json::Value::String(s) => s.clone(),
                _ => v.to_string(),
            };
            ListItem::new(Line::from(Span::raw(line)))
        })
        .collect();
    let title = format!("Items ({})", state.items.len());
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    frame.render_widget(list, area);
}

fn draw_streaming_prompt(frame: &mut Frame<'_>, area: Rect, state: &StreamingAction) {
    let Some(prompt) = state.pending_prompt.as_ref() else {
        return;
    };
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(" Awaiting your response ")
        .border_style(Style::default().fg(Color::Yellow));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(2),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let prompt_text = prompt
        .get("prompt")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            // Fall back to dumping the schema so the user has *something*
            // to act on if the plugin omitted a human-readable prompt.
            prompt
                .get("schema")
                .map(|s| s.to_string())
                .unwrap_or_default()
        });
    let para = Paragraph::new(prompt_text)
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, layout[0]);

    let input_label = "Response:";
    let label_widget = Paragraph::new(input_label).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(label_widget, layout[1]);

    let input =
        Paragraph::new(state.response_input.as_str()).style(Style::default().fg(Color::Yellow));
    frame.render_widget(input, layout[2]);
}

fn draw_streaming_warnings(frame: &mut Frame<'_>, area: Rect, state: &StreamingAction) {
    let take = area.height.saturating_sub(2) as usize;
    let start = state.warnings.len().saturating_sub(take);
    let items: Vec<ListItem> = state.warnings[start..]
        .iter()
        .map(|v| {
            let msg = v
                .get("message")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| v.to_string());
            ListItem::new(Line::from(Span::styled(
                msg,
                Style::default().fg(Color::Yellow),
            )))
        })
        .collect();
    let title = format!("Warnings ({})", state.warnings.len());
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    frame.render_widget(list, area);
}

fn draw_streaming_terminal(frame: &mut Frame<'_>, area: Rect, state: &StreamingAction) {
    let Some(terminal) = state.terminal.as_ref() else {
        return;
    };
    let stage = terminal
        .get("stage")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let detail = terminal
        .get("error")
        .or_else(|| terminal.get("message"))
        .or_else(|| terminal.get("result"))
        .map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default();
    let color = match stage {
        "complete" => Color::Green,
        "error" | "timeout" => Color::Red,
        "canceled" => Color::Magenta,
        _ => Color::Gray,
    };
    let body = if detail.is_empty() {
        stage.to_string()
    } else {
        format!("{stage}: {detail}")
    };
    let para = Paragraph::new(body)
        .block(Block::default().borders(Borders::ALL).title(" Result "))
        .style(Style::default().fg(color))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn draw_groups_overlay(frame: &mut Frame<'_>, app: &App) {
    let popup = centered_rect(60, 60, frame.area());
    frame.render_widget(Clear, popup);

    let outer = Block::default()
        .borders(Borders::ALL)
        .title("Rule Groups")
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
                    Span::styled(
                        format!("  {:<28}", g.name),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{} rule(s)", rule_count),
                        Style::default().fg(Color::DarkGray),
                    ),
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

fn draw_filter_bar(frame: &mut Frame<'_>, filter_bar: &RuleFilterBar) {
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
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(40),
            Constraint::Percentage(20),
        ])
        .split(bar_area);

    let tag_style = if filter_bar.active_field == RuleFilterField::Tag {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };
    let trigger_style = if filter_bar.active_field == RuleFilterField::Trigger {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    let tag_field = Paragraph::new(filter_bar.tag.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Tag filter")
            .border_style(tag_style),
    );
    frame.render_widget(tag_field, layout[0]);

    let trigger_field = Paragraph::new(filter_bar.trigger.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Trigger filter")
            .border_style(trigger_style),
    );
    frame.render_widget(trigger_field, layout[1]);

    let stale_str = if filter_bar.stale { "ON " } else { "off" };
    let stale_color = if filter_bar.stale {
        Color::Red
    } else {
        Color::DarkGray
    };
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

    let input = Paragraph::new(app.log_module_input.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Filter string")
            .border_style(Style::default().fg(Color::Yellow)),
    );
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
        Line::from(Span::styled(
            "e ERROR",
            if app.log_level_filter == LogLevelFilter::Error {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        )),
        Line::from(Span::styled(
            "w WARN",
            if app.log_level_filter == LogLevelFilter::Warn {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        )),
        Line::from(Span::styled(
            "i INFO",
            if app.log_level_filter == LogLevelFilter::Info {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        )),
        Line::from(Span::styled(
            "d DEBUG",
            if app.log_level_filter == LogLevelFilter::Debug {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        )),
    ];

    let selected_idx = match app.log_level_filter {
        LogLevelFilter::Error => 0,
        LogLevelFilter::Warn => 1,
        LogLevelFilter::Info => 2,
        LogLevelFilter::Debug => 3,
    };

    let live_dot = if app.log_ws_connected { "●" } else { "○" };
    let live_color = if app.log_ws_connected {
        Color::Green
    } else {
        Color::Red
    };
    let pause_str = if app.log_paused {
        " [PAUSED]"
    } else {
        " [LIVE]"
    };
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
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
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
                "WARN" => (Color::Yellow, Modifier::empty()),
                "INFO" => (Color::Cyan, Modifier::empty()),
                _ => (Color::DarkGray, Modifier::empty()),
            };
            let ts: String = line.timestamp.chars().skip(11).take(8).collect();
            let target_short: String = line.target.chars().take(24).collect();

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("[{:<5}] ", &level_upper),
                    Style::default().fg(level_color).add_modifier(level_mod),
                ),
                Span::styled(format!("{ts} "), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:<24} ", target_short),
                    Style::default().fg(Color::Blue),
                ),
                Span::raw(line.message.clone()),
            ]))
        })
        .collect();

    let block_title = if total > height {
        format!(
            "Log lines (scroll: {}/{})",
            start + 1,
            total.saturating_sub(height - 1)
        )
    } else {
        "Log lines".to_string()
    };

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(block_title));
    frame.render_widget(list, area);
}

// ── Backup / export / import tab ──────────────────────────────────────────────
//
// Renders the fixed list of BACKUP_ACTIONS with the current selection
// highlighted, plus a status footer showing the last action's result
// (path of saved export, count of imported items, error message).

// ── Audit log tab ─────────────────────────────────────────────────────────────
//
// Read-only paginated view of `GET /audit`. Three rows:
//   - Row 1: list of audit entries (mono ts | actor | event | target)
//   - Row 2: expanded detail JSON (only when audit_expanded_idx is Some)
//   - Row 3: pagination footer (page X · Y entries · last error if any)

fn draw_audit_tab(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let has_expanded = app.audit_expanded_idx.is_some();
    let constraints: Vec<Constraint> = if has_expanded {
        vec![
            Constraint::Min(6),     // entry list
            Constraint::Length(10), // detail panel
            Constraint::Length(3),  // footer
        ]
    } else {
        vec![Constraint::Min(6), Constraint::Length(3)]
    };
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    // Entry list
    let items: Vec<ListItem> = app
        .audit_entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let style = if i == app.selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let result_color = match e.result {
                crate::api::AuditResult::Success => Color::Green,
                crate::api::AuditResult::Denied => Color::Yellow,
                crate::api::AuditResult::Error => Color::Red,
            };
            // Compact ts (skip TZ for readability), then 4 columns.
            let ts = e.ts.split('.').next().unwrap_or(e.ts.as_str()).to_string();
            let target = match (&e.target_kind, &e.target_id) {
                (Some(k), Some(id)) => format!("{k}/{id}"),
                (Some(k), None) => k.clone(),
                _ => String::new(),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {:<19} ", ts), style),
                Span::styled(
                    format!("{:<14}", e.actor_type.as_str()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<24}", e.actor_label.chars().take(24).collect::<String>()),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{:<28}", e.event_type.chars().take(28).collect::<String>()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("{:<10}", e.result.as_str()),
                    Style::default().fg(result_color),
                ),
                Span::styled(
                    target.chars().take(40).collect::<String>(),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();
    let title = if app.audit_loading {
        "Audit [loading…]".to_string()
    } else {
        format!("Audit ({} entries)", app.audit_entries.len())
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    let mut state = ratatui::widgets::ListState::default();
    if !app.audit_entries.is_empty() {
        state.select(Some(
            app.selected.min(app.audit_entries.len().saturating_sub(1)),
        ));
    }
    frame.render_stateful_widget(list, layout[0], &mut state);

    // Optional expanded detail
    if let Some(idx) = app.audit_expanded_idx
        && let Some(entry) = app.audit_entries.get(idx)
    {
        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(vec![
            Span::styled("ts: ", Style::default().fg(Color::DarkGray)),
            Span::styled(entry.ts.clone(), Style::default().fg(Color::White)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("actor: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} ({})", entry.actor_label, entry.actor_type.as_str()),
                Style::default().fg(Color::White),
            ),
        ]));
        if let Some(scope) = &entry.scope_used {
            lines.push(Line::from(vec![
                Span::styled("scope: ", Style::default().fg(Color::DarkGray)),
                Span::styled(scope.clone(), Style::default().fg(Color::Cyan)),
            ]));
        }
        if let Some(ip) = &entry.ip {
            lines.push(Line::from(vec![
                Span::styled("ip: ", Style::default().fg(Color::DarkGray)),
                Span::styled(ip.clone(), Style::default().fg(Color::White)),
            ]));
        }
        let detail_str = serde_json::to_string_pretty(&entry.detail)
            .unwrap_or_else(|_| entry.detail.to_string());
        for line in detail_str.lines().take(20) {
            lines.push(Line::from(line.to_string()));
        }
        let detail_widget = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Detail"))
            .wrap(Wrap { trim: false });
        frame.render_widget(detail_widget, layout[1]);
    }

    // Footer: page + offset + error
    let footer_idx = if has_expanded { 2 } else { 1 };
    let page = app.audit_offset / app.audit_limit + 1;
    let footer_text = if let Some(err) = &app.audit_error {
        format!("Error: {err}")
    } else {
        format!(
            "page {} · offset {} · limit {} · n=next p=prev r=refresh Enter=detail",
            page, app.audit_offset, app.audit_limit
        )
    };
    let footer_color = if app.audit_error.is_some() {
        Color::Red
    } else {
        Color::DarkGray
    };
    let footer = Paragraph::new(footer_text)
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .style(Style::default().fg(footer_color))
        .wrap(Wrap { trim: true });
    frame.render_widget(footer, layout[footer_idx]);
}

fn draw_backup_tab(frame: &mut Frame<'_>, app: &App, area: Rect) {
    use crate::app::{BACKUP_ACTIONS, backup_exports_dir, backup_imports_dir};

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),    // action list
            Constraint::Length(3), // path hint
            Constraint::Length(3), // status line
        ])
        .split(area);

    // Action list
    let items: Vec<ListItem> = BACKUP_ACTIONS
        .iter()
        .enumerate()
        .map(|(i, (_, label))| {
            let style = if i == app.selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(format!("  {label}"), style)))
        })
        .collect();
    let list_title = if app.backup_busy {
        "Backup [busy]"
    } else {
        "Backup"
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(list_title))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(
        app.selected.min(BACKUP_ACTIONS.len().saturating_sub(1)),
    ));
    frame.render_stateful_widget(list, layout[0], &mut state);

    // Path hint
    let hint = format!(
        "Exports → {}    Imports ← {}",
        backup_exports_dir().display(),
        backup_imports_dir().display(),
    );
    let hint_widget = Paragraph::new(hint)
        .block(Block::default().borders(Borders::ALL).title("Paths"))
        .style(Style::default().fg(Color::DarkGray))
        .wrap(Wrap { trim: true });
    frame.render_widget(hint_widget, layout[1]);

    // Status line — last result or empty.
    let status_text = if app.backup_status.is_empty() {
        "Press Enter to run the selected action.".to_string()
    } else {
        app.backup_status.clone()
    };
    let status_color = if app.backup_status.starts_with("Error:") {
        Color::Red
    } else if app.backup_status.starts_with("Saved")
        || app.backup_status.starts_with("Exported")
        || app.backup_status.starts_with("Imported")
    {
        Color::Green
    } else {
        Color::Yellow
    };
    let status_widget = Paragraph::new(status_text)
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .style(Style::default().fg(status_color))
        .wrap(Wrap { trim: true });
    frame.render_widget(status_widget, layout[2]);
}

// ── System Status tab ─────────────────────────────────────────────────────────

fn draw_status_tab(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("System Status");

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
    let started_at_str: String = status
        .started_at
        .chars()
        .take(19)
        .collect::<String>()
        .replace('T', " ");

    let left_refresh = app.system_status_last_refresh.as_deref().unwrap_or("never");
    let time_label = if app.time_utc { "(UTC)" } else { "(local)" };

    let mut left_lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "System",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )),
        Line::from(""),
        status_detail_row("Version", &status.version),
        status_detail_row("Started", &started_at_str),
        status_detail_row("Uptime", &uptime_str),
        status_detail_row("Last refresh", &format!("{} {}", left_refresh, time_label)),
        Line::from(""),
        Line::from(Span::styled(
            "Rules",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )),
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
        Line::from(Span::styled(
            "Devices & Plugins",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )),
        Line::from(""),
        status_detail_row("Total devices", &status.devices_total.to_string()),
        status_detail_row("Active plugins", &status.plugins_active.to_string()),
        Line::from(""),
        Line::from(Span::styled(
            "Storage",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )),
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

// ── Plugin Actions panel ──────────────────────────────────────────────────────
//
// Renders the manifest's actions list with role + streaming + concurrency
// badges, plus a status footer for the last action result. Up/Down moves
// selection (handled by app); Enter triggers run_selected_plugin_action.

fn draw_plugin_actions(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(area);

    // Action list
    let title = if app.plugin_capabilities_loading {
        "Actions [loading…]".to_string()
    } else if let Some(caps) = app.plugin_capabilities.as_ref() {
        format!(
            "Actions ({} actions, spec v{})",
            caps.actions.len(),
            caps.spec
        )
    } else if app.plugin_capabilities_error.is_some() {
        "Actions [error]".to_string()
    } else {
        "Actions".to_string()
    };

    let items: Vec<ListItem> = if let Some(caps) = app.plugin_capabilities.as_ref() {
        caps.actions
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let row_style = if i == app.selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let role_color = match a.requires_role {
                    crate::api::RequiresRole::Admin => Color::Red,
                    crate::api::RequiresRole::User => Color::Cyan,
                    crate::api::RequiresRole::ReadOnly => Color::DarkGray,
                };
                let stream_badge = if a.stream { "stream" } else { "sync  " };
                let stream_color = if a.stream {
                    Color::Magenta
                } else {
                    Color::DarkGray
                };
                let concurrency = a.concurrency.as_str();
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {:<24}", a.id), row_style),
                    Span::styled(
                        format!(" {:<10}", a.requires_role.as_str()),
                        Style::default().fg(role_color),
                    ),
                    Span::styled(
                        format!(" {stream_badge}"),
                        Style::default().fg(stream_color),
                    ),
                    Span::styled(
                        format!(" {:<7}", concurrency),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(format!(" {}", a.label), Style::default().fg(Color::White)),
                ]))
            })
            .collect()
    } else {
        Vec::new()
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    let mut state = ratatui::widgets::ListState::default();
    if let Some(caps) = app.plugin_capabilities.as_ref()
        && !caps.actions.is_empty()
    {
        state.select(Some(app.selected.min(caps.actions.len().saturating_sub(1))));
    }
    frame.render_stateful_widget(list, layout[0], &mut state);

    // Status footer
    let footer_text = if app.action_busy {
        format!("Running… {}", app.action_status)
    } else if !app.action_status.is_empty() {
        app.action_status.clone()
    } else if let Some(err) = &app.plugin_capabilities_error {
        format!("Error loading capabilities: {err}")
    } else if app
        .plugin_capabilities
        .as_ref()
        .map(|c| c.actions.is_empty())
        .unwrap_or(false)
    {
        "This plugin declares no actions.".to_string()
    } else {
        "Up/Down to select · Enter to run · Esc to close detail".to_string()
    };
    let footer_color = if app.action_status.starts_with("`")
        && (app.action_status.contains(" failed:") || app.action_status.contains(" requires admin"))
        || app.plugin_capabilities_error.is_some()
    {
        Color::Red
    } else if app.action_busy {
        Color::Yellow
    } else if app.action_status.contains(" ok ") {
        Color::Green
    } else {
        Color::DarkGray
    };
    let footer = Paragraph::new(footer_text)
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .style(Style::default().fg(footer_color))
        .wrap(Wrap { trim: true });
    frame.render_widget(footer, layout[1]);
}

fn draw_plugin_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(plugin_id) = app.plugin_detail_plugin_id.as_deref() else {
        let msg = Paragraph::new("No plugin selected").block(
            Block::default()
                .borders(Borders::ALL)
                .title("Plugin Detail"),
        );
        frame.render_widget(msg, area);
        return;
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5)])
        .split(area);

    let panel_labels = [
        PluginDetailPanel::Overview.title(),
        PluginDetailPanel::Actions.title(),
        PluginDetailPanel::Diagnostics.title(),
        PluginDetailPanel::Metrics.title(),
    ]
    .into_iter()
    .map(Line::from)
    .collect::<Vec<_>>();

    let panel_idx = match app.plugin_detail_panel {
        PluginDetailPanel::Overview => 0,
        PluginDetailPanel::Actions => 1,
        PluginDetailPanel::Diagnostics => 2,
        PluginDetailPanel::Metrics => 3,
    };

    let tabs = Tabs::new(panel_labels)
        .select(panel_idx)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Plugin Detail: {}", plugin_id)),
        )
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
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
            let entertainment_events = count_type("entertainment_action_applied")
                + count_type("entertainment_status_changed");
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
                format!(
                    "Plugin '{}' is not present in current plugin list.",
                    plugin_id
                )
            };
            let widget = Paragraph::new(body)
                .block(Block::default().borders(Borders::ALL).title("Overview"))
                .wrap(Wrap { trim: true });
            frame.render_widget(widget, layout[1]);
        }
        PluginDetailPanel::Actions => {
            draw_plugin_actions(frame, app, layout[1]);
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
            let list = List::new(rows).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Diagnostics Events"),
            );
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
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Metrics Snapshot"),
                )
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
        DeviceSubPanel::MediaPlayers => 1,
        DeviceSubPanel::Switches => 2,
        DeviceSubPanel::Timers => 3,
    };
    let sub_tabs = Tabs::new(vec![
        Line::from("All"),
        Line::from("Media Players"),
        Line::from("Switches"),
        Line::from("Timers"),
    ])
    .select(active_idx)
    .block(Block::default().borders(Borders::ALL).title("Devices"))
    .style(Style::default().fg(Color::Gray))
    .highlight_style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
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
        DeviceSubPanel::MediaPlayers => {
            draw_media_players_panel(frame, app, layout[1]);
        }
        DeviceSubPanel::Switches => {
            draw_switches_list(frame, app, layout[1]);
        }
        DeviceSubPanel::Timers => {
            draw_timers_list(frame, app, layout[1]);
        }
    }
}

fn draw_media_players_panel(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);

    let players = app.visible_media_players();
    let items = players
        .iter()
        .enumerate()
        .map(|(i, device)| {
            let model = App::media_player_model(device);
            let state = model
                .as_ref()
                .map(|value| normalize_label(&value.playback_state))
                .unwrap_or_else(|| app.device_status(device));
            let summary = model
                .as_ref()
                .and_then(|value| value.title.as_ref())
                .map(|value| value.chars().take(18).collect::<String>())
                .unwrap_or_default();
            let style = if i == app.selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if device.available {
                Style::default()
            } else {
                Style::default().fg(Color::DarkGray)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<22}", clean_name(&device.name)), style),
                Span::styled(format!(" {:<10}", state), style),
                Span::styled(
                    if summary.is_empty() {
                        "".to_string()
                    } else {
                        format!(" {summary}")
                    },
                    style,
                ),
            ]))
        })
        .collect::<Vec<_>>();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Media Players [{}]", players.len())),
    );
    let mut state = ratatui::widgets::ListState::default();
    state.select(if players.is_empty() {
        None
    } else {
        Some(app.selected)
    });
    frame.render_stateful_widget(list, panes[0], &mut state);

    draw_media_player_detail(frame, app, panes[1]);
}

fn draw_media_player_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Media Player Detail");

    let Some(model) = app.selected_media_player_model() else {
        let msg = Paragraph::new("No media player selected")
            .block(block)
            .alignment(Alignment::Center);
        frame.render_widget(msg, area);
        return;
    };

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        clean_name(&model.display_name),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(""));
    lines.push(detail_row(
        "Device ID",
        vec![Span::raw(model.device_id.clone())],
    ));
    if let Some(canonical_name) = &model.canonical_name {
        lines.push(detail_row(
            "Canonical",
            vec![Span::styled(
                canonical_name.clone(),
                Style::default().fg(Color::Cyan),
            )],
        ));
    }
    lines.push(detail_row(
        "Plugin",
        vec![Span::raw(clean_plugin_id(&model.plugin_id))],
    ));

    let playback_color = match model.playback_state.as_str() {
        "playing" => Color::Green,
        "paused" => Color::Yellow,
        "stopped" => Color::DarkGray,
        "buffering" => Color::Cyan,
        _ => Color::Gray,
    };
    lines.push(detail_row(
        "State",
        vec![Span::styled(
            normalize_label(&model.playback_state),
            Style::default()
                .fg(playback_color)
                .add_modifier(Modifier::BOLD),
        )],
    ));

    if let Some(title) = &model.title {
        lines.push(detail_row("Title", vec![Span::raw(title.clone())]));
    }
    if let Some(artist) = &model.artist {
        lines.push(detail_row("Artist", vec![Span::raw(artist.clone())]));
    }
    if let Some(album) = &model.album {
        lines.push(detail_row("Album", vec![Span::raw(album.clone())]));
    }
    if let Some(source) = &model.source {
        lines.push(detail_row("Source", vec![Span::raw(source.clone())]));
    }
    if let Some(volume) = model.volume {
        let bar = make_bar(volume as f64 / 100.0, 10);
        lines.push(detail_row(
            "Volume",
            vec![
                Span::styled(format!("{volume:3}% "), Style::default().fg(Color::Yellow)),
                Span::styled(bar, Style::default().fg(Color::Yellow)),
            ],
        ));
    }
    if let Some(muted) = model.muted {
        lines.push(detail_row(
            "Mute",
            vec![Span::styled(
                if muted { "Muted" } else { "Unmuted" },
                Style::default().fg(if muted { Color::Yellow } else { Color::Green }),
            )],
        ));
    }
    if let (Some(position), Some(duration)) = (model.position_secs, model.duration_secs) {
        let progress = if duration == 0 {
            0.0
        } else {
            position as f64 / duration as f64
        }
        .clamp(0.0, 1.0);
        lines.push(detail_row(
            "Progress",
            vec![
                Span::raw(format!(
                    "{} / {} ",
                    format_duration_ms(position * 1000),
                    format_duration_ms(duration * 1000)
                )),
                Span::styled(make_bar(progress, 10), Style::default().fg(Color::Cyan)),
            ],
        ));
    }

    lines.push(Line::from(""));
    let mut commands = Vec::new();
    if model.capabilities.can_play {
        commands.push("play");
    }
    if model.capabilities.can_pause {
        commands.push("pause");
    }
    if model.capabilities.can_stop {
        commands.push("stop");
    }
    if model.capabilities.can_previous {
        commands.push("previous");
    }
    if model.capabilities.can_next {
        commands.push("next");
    }
    if model.capabilities.can_set_volume {
        commands.push("volume");
    }
    if model.capabilities.can_mute {
        commands.push("mute");
    }
    lines.push(detail_row(
        "Controls",
        vec![Span::styled(
            if commands.is_empty() {
                "none".to_string()
            } else {
                commands.join(", ")
            },
            Style::default().fg(Color::DarkGray),
        )],
    ));

    for detail in &model.extra_details {
        lines.push(detail_row(
            &detail.label,
            vec![Span::styled(
                detail.value.clone(),
                Style::default().fg(Color::Magenta),
            )],
        ));
    }

    let widget = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(widget, inner);
}

fn draw_device_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (items, render_selected) = if app.view_mode == DeviceViewMode::Grouped {
        build_grouped_list(app)
    } else {
        build_flat_list(app)
    };

    let mode_label = if app.view_mode == DeviceViewMode::Grouped {
        "Grouped"
    } else {
        "Flat"
    };
    let search = if app.device_search_query.trim().is_empty() {
        "-".to_string()
    } else {
        app.device_search_query.clone()
    };
    let title = format!(
        "Devices ({mode_label}) [{}] f:{} s:{} q:{}",
        app.visible_devices().len(),
        app.device_filter_mode.title(),
        app.device_sort_mode.title(),
        search,
    );

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
    let selected = if visible.is_empty() {
        None
    } else {
        Some(app.selected)
    };
    (items, selected)
}

fn device_list_row(
    app: &App,
    device: &DeviceState,
    is_selected: bool,
    indent: bool,
) -> ListItem<'static> {
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
    let status_shows_temp = status.contains('°');
    let status_shows_humidity = status.ends_with("%rh");
    let status_shows_battery = status.ends_with('%') && !status_shows_humidity;

    if let Some(b) = App::device_battery(device)
        && !status_shows_battery
    {
        suffix.push_str(&format!(" {b}%🔋"));
    }
    if let Some(t) = App::device_temperature(device)
        && !status_shows_temp
    {
        suffix.push_str(&format!(" {t:.1}°"));
    }
    if let Some(h) = App::device_humidity(device)
        && !status_shows_humidity
    {
        suffix.push_str(&format!(" {h:.0}%"));
    }
    // Timer countdown suffix
    if device.plugin_id == "core.timer" {
        let timer_state = device
            .attributes
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("idle");
        if matches!(timer_state, "running" | "paused") {
            let remaining_ms = timer_remaining_secs(device) * 1000;
            let icon = if timer_state == "running" {
                "▶"
            } else {
                "⏸"
            };
            suffix.push_str(&format!(" {icon} {}", format_duration_ms(remaining_ms)));
        }
    }

    if is_selected {
        let sel_style = Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let line = Line::from(vec![
            Span::styled(prefix.to_string(), sel_style),
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
            Span::styled(prefix.to_string(), base_style),
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
    lines.push(detail_row(
        "Device ID",
        vec![Span::raw(device.device_id.clone())],
    ));
    if let Some(canonical_name) = &device.canonical_name
        && !canonical_name.is_empty()
    {
        lines.push(detail_row(
            "Canonical",
            vec![Span::styled(
                canonical_name.clone(),
                Style::default().fg(Color::Cyan),
            )],
        ));
    }

    // Status
    let status = app.device_status(device);
    let sc = status_color(&status, device.available);
    lines.push(detail_row(
        "Status",
        vec![Span::styled(
            status,
            Style::default().fg(sc).add_modifier(Modifier::BOLD),
        )],
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
    if let Some(loc) = device.attributes.get("location").and_then(|v| v.as_str())
        && !loc.is_empty()
    {
        lines.push(detail_row(
            "ZW Location",
            vec![Span::styled(
                loc.to_string(),
                Style::default().fg(Color::DarkGray),
            )],
        ));
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
        let timer_state = device
            .attributes
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("idle");
        let duration_ms = device
            .attributes
            .get("duration_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            * 1000;
        let remaining_ms = timer_remaining_secs(device) * 1000;
        let repeat = device
            .attributes
            .get("repeat")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let state_color = match timer_state {
            "running" => Color::Green,
            "paused" => Color::Yellow,
            "finished" => Color::Cyan,
            "cancelled" => Color::DarkGray,
            _ => Color::DarkGray,
        };
        lines.push(detail_row(
            "Timer State",
            vec![Span::styled(
                normalize_label(timer_state),
                Style::default()
                    .fg(state_color)
                    .add_modifier(Modifier::BOLD),
            )],
        ));

        if duration_ms > 0 {
            lines.push(detail_row(
                "Duration",
                vec![Span::raw(format_duration_ms(duration_ms))],
            ));
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
            lines.push(detail_row(
                "Repeat",
                vec![Span::styled("Yes", Style::default().fg(Color::Yellow))],
            ));
        }

        if let Some(lbl) = device.attributes.get("label").and_then(|v| v.as_str())
            && !lbl.is_empty()
        {
            lines.push(detail_row("Label", vec![Span::raw(lbl.to_string())]));
        }

        if let Some(started) = device.attributes.get("started_at").and_then(|v| v.as_str()) {
            lines.push(detail_row(
                "Started",
                vec![Span::styled(
                    started.to_string(),
                    Style::default().fg(Color::DarkGray),
                )],
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
                Span::styled(
                    low,
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
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
                Span::styled(
                    format!("{brightness:3}% "),
                    Style::default().fg(Color::Yellow),
                ),
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
        if let Some(bolt) = device
            .attributes
            .get("bolt_status")
            .and_then(|v| v.as_str())
        {
            let (s, c) = if bolt == "locked" {
                ("Locked", Color::Red)
            } else {
                ("Unlocked", Color::Green)
            };
            lines.push(detail_row(
                "Bolt",
                vec![Span::styled(s, Style::default().fg(c))],
            ));
        }
        // Physical latch sensor
        if let Some(latch) = device
            .attributes
            .get("latch_status")
            .and_then(|v| v.as_str())
        {
            let (s, c) = if latch == "closed" {
                ("Closed", Color::Green)
            } else {
                ("Open", Color::Yellow)
            };
            lines.push(detail_row(
                "Latch",
                vec![Span::styled(s, Style::default().fg(c))],
            ));
        }
        // Door open/closed sensor — string variant (ZWave)
        if let Some(door) = device
            .attributes
            .get("door_status")
            .and_then(|v| v.as_str())
        {
            let (s, c) = if door == "closed" {
                ("Closed", Color::Green)
            } else {
                ("Open", Color::Yellow)
            };
            lines.push(detail_row(
                "Door",
                vec![Span::styled(s, Style::default().fg(c))],
            ));
        }
        // Door open/closed sensor — bool variant (YoLink)
        if let Some(door_open) = device.attributes.get("door_open").and_then(|v| v.as_bool()) {
            let (s, c) = if door_open {
                ("Open", Color::Yellow)
            } else {
                ("Closed", Color::Green)
            };
            lines.push(detail_row(
                "Door",
                vec![Span::styled(s, Style::default().fg(c))],
            ));
        }
        // Last alert (e.g. UnLockFailed, DoorOpenAlarm)
        if let Some(alert) = device.attributes.get("last_alert").and_then(|v| v.as_str()) {
            lines.push(detail_row(
                "Last Alert",
                vec![Span::styled(
                    alert.to_string(),
                    Style::default().fg(Color::Yellow),
                )],
            ));
        }
        // Auto-lock timeout (YoLink attributes.autoLock)
        if let Some(secs) = device
            .attributes
            .get("auto_lock_secs")
            .and_then(|v| v.as_u64())
            && secs > 0
        {
            lines.push(detail_row("Auto-lock", vec![Span::raw(format!("{secs}s"))]));
        }
        // Operation type: 1=Constant, 2=Timed (ZWave)
        if let Some(op_type) = device
            .attributes
            .get("lock_operation_type")
            .and_then(|v| v.as_f64())
        {
            let label = match op_type as u64 {
                1 => "Constant",
                2 => "Timed",
                _ => "Unknown",
            };
            lines.push(detail_row("Op Mode", vec![Span::raw(label)]));
        }
        // Timed mode timeout
        if let Some(timeout) = device
            .attributes
            .get("lock_timeout_secs")
            .and_then(|v| v.as_f64())
            && timeout > 0.0
        {
            lines.push(detail_row(
                "Timeout",
                vec![Span::raw(format!("{timeout:.0}s"))],
            ));
        }
        if let Some(relock) = device
            .attributes
            .get("lock_auto_relock_secs")
            .and_then(|v| v.as_f64())
            && relock > 0.0
        {
            lines.push(detail_row(
                "Auto-relock",
                vec![Span::raw(format!("{relock:.0}s"))],
            ));
        }
    }

    // Motion sensor
    if let Some(motion) = device.attributes.get("motion").and_then(|v| v.as_bool()) {
        let (s, c) = if motion {
            ("Motion", Color::Yellow)
        } else {
            ("Clear", Color::Green)
        };
        lines.push(detail_row(
            "Motion",
            vec![Span::styled(s, Style::default().fg(c))],
        ));
    }

    // Contact sensor
    if let Some(open) = device
        .attributes
        .get("contact_open")
        .and_then(|v| v.as_bool())
    {
        let (s, c) = if open {
            ("Open", Color::Red)
        } else {
            ("Closed", Color::Green)
        };
        lines.push(detail_row(
            "Contact",
            vec![Span::styled(s, Style::default().fg(c))],
        ));
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
        let mc = match mode {
            "heat" => Color::Red,
            "cool" => Color::Cyan,
            "off" => Color::DarkGray,
            _ => Color::White,
        };
        lines.push(detail_row(
            "Mode",
            vec![Span::styled(normalize_label(mode), Style::default().fg(mc))],
        ));
    }
    if let Some(action) = device
        .attributes
        .get("hvac_action")
        .and_then(|v| v.as_str())
    {
        lines.push(detail_row("HVAC", vec![Span::raw(normalize_label(action))]));
    }
    if let Some(setpoint) = device
        .attributes
        .get("target_temp")
        .and_then(|v| v.as_f64())
    {
        lines.push(detail_row(
            "Setpoint",
            vec![Span::styled(
                format!("{setpoint:.1}°F"),
                Style::default().fg(Color::Yellow),
            )],
        ));
    }

    // Energy monitoring
    let has_energy = ["power_w", "energy_kwh", "voltage", "current_a"]
        .iter()
        .any(|k| device.attributes.contains_key(*k));
    if has_energy {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─── Energy ───",
            Style::default().fg(Color::DarkGray),
        )));
    }
    if let Some(w) = device.attributes.get("power_w").and_then(|v| v.as_f64()) {
        lines.push(detail_row(
            "Power",
            vec![Span::styled(
                format!("{w:.1} W"),
                Style::default().fg(Color::Yellow),
            )],
        ));
    }
    if let Some(kwh) = device.attributes.get("energy_kwh").and_then(|v| v.as_f64()) {
        lines.push(detail_row(
            "Energy",
            vec![Span::raw(format!("{kwh:.3} kWh"))],
        ));
    }
    if let Some(v) = device.attributes.get("voltage").and_then(|v| v.as_f64()) {
        lines.push(detail_row("Voltage", vec![Span::raw(format!("{v:.1} V"))]));
    }
    if let Some(a) = device.attributes.get("current_a").and_then(|v| v.as_f64()) {
        lines.push(detail_row("Current", vec![Span::raw(format!("{a:.2} A"))]));
    }

    // Environmental extras
    if let Some(lux) = device
        .attributes
        .get("illuminance")
        .and_then(|v| v.as_f64())
    {
        lines.push(detail_row(
            "Illuminance",
            vec![Span::raw(format!("{lux:.0} lx"))],
        ));
    }
    if let Some(co2) = device.attributes.get("co2_ppm").and_then(|v| v.as_f64()) {
        lines.push(detail_row("CO₂", vec![Span::raw(format!("{co2:.0} ppm"))]));
    }

    // Alarm states (only show when active)
    for (key, label) in &[
        ("smoke", "Smoke"),
        ("co", "CO"),
        ("water_detected", "Water"),
        ("tamper", "Tamper"),
        ("vibration", "Vibration"),
    ] {
        if let Some(true) = device.attributes.get(*key).and_then(|v| v.as_bool()) {
            lines.push(detail_row(
                label,
                vec![Span::styled(
                    "ALARM",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )],
            ));
        }
    }

    // Other attributes — everything not already rendered above, excluding ZWave
    // internal noise properties that have no user-visible meaning.
    let shown = [
        "on",
        "state",
        "open",
        "online",
        "locked",
        "battery",
        "battery_level",
        "battery_percent",
        "battery_pct",
        "battery_state",
        "battery_low",
        "temperature",
        "temp",
        "humidity",
        "brightness",
        "motion",
        "contact_open",
        "position",
        "location",
        "mode",
        "hvac_action",
        "target_temp",
        "power_w",
        "energy_kwh",
        "voltage",
        "current_a",
        "illuminance",
        "co2_ppm",
        "pressure",
        "uv_index",
        "smoke",
        "co",
        "water_detected",
        "tamper",
        "vibration",
        "color_rgb",
        "color_temp",
        // Door lock physical sensors + config
        "bolt_status",
        "latch_status",
        "door_status",
        "door_open",
        "lock_operation_type",
        "lock_timeout_secs",
        "lock_auto_relock_secs",
        "last_alert",
        "auto_lock_secs",
        "sound_level",
        // Timer device attributes
        "duration_secs",
        "remaining_secs",
        "repeat",
        "started_at",
        "label",
    ];
    // ZWave internal / write-echo properties with no useful display value.
    // Also includes raw nodeInfo keys that survived field_map (shouldn't normally
    // appear, but guard against config mismatches).
    let zwave_noise = [
        "targetValue",
        "currentValue",
        "targetMode",
        "currentMode",
        "duration",
        "restorePrevious",
        "targetColor",
        "currentColor",
        "nodeName",
        "nodeLocation", // raw nodeInfo keys (mapped → name/location)
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
            let val_str = val
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| val.to_string());
            let display_key = normalize_label(key);
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<13}", display_key),
                    Style::default().fg(Color::DarkGray),
                ),
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
            let on = s
                .attributes
                .get("on")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let (dot, dot_color) = if on {
                ("●", Color::Green)
            } else {
                ("○", Color::DarkGray)
            };
            let label = if s.name != s.device_id {
                format!("  {}", s.name)
            } else {
                String::new()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {dot} "), Style::default().fg(dot_color)),
                Span::styled(
                    format!("{:<36}", s.device_id),
                    Style::default().fg(Color::White),
                ),
                Span::styled(label, Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Switches [{}]", app.switches.len())),
        )
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
            let state = t
                .attributes
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("idle");
            let state_color = match state {
                "running" => Color::Green,
                "finished" => Color::Yellow,
                "paused" => Color::Cyan,
                _ => Color::DarkGray,
            };
            let label = if t.name != t.device_id {
                format!("  {}", t.name)
            } else {
                String::new()
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {:<36}", t.device_id),
                    Style::default().fg(Color::White),
                ),
                Span::styled(format!("{:<10}", state), Style::default().fg(state_color)),
                Span::styled(label, Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Timers [{}]", app.timers.len())),
        )
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

    let canonical_style = if matches!(editor.field, DeviceEditField::CanonicalName) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let canonical = Paragraph::new(editor.canonical_name.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Canonical Name")
            .border_style(canonical_style),
    );
    frame.render_widget(canonical, layout[3]);

    let status = app
        .devices
        .iter()
        .find(|device| device.device_id == editor.device_id)
        .map(|device| app.device_status(device))
        .unwrap_or_else(|| "Unknown".to_string());
    let status_line = Paragraph::new(format!("Status: {status}")).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Device Status"),
    );
    frame.render_widget(status_line, layout[4]);

    let help = Paragraph::new("Tab switch field | Enter save | Esc cancel")
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(help, layout[5]);
}

fn draw_area_editor(frame: &mut Frame<'_>, app: &App, editor: &AreaEditor) {
    let title = if editor.id.is_none() {
        "New Area"
    } else {
        "Rename Area"
    };
    let popup = centered_rect(60, 30, frame.area());
    frame.render_widget(Clear, popup);

    let outer = Block::default().borders(Borders::ALL).title(title);
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(inner);

    let name_field = Paragraph::new(editor.name.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Name")
            .border_style(Style::default().fg(Color::Yellow)),
    );
    frame.render_widget(name_field, layout[0]);

    let help = Paragraph::new("Enter save | Esc cancel").alignment(Alignment::Center);
    frame.render_widget(help, layout[1]);

    let _ = app;
}

fn draw_user_editor(frame: &mut Frame<'_>, app: &App, editor: &UserEditor) {
    let title = match editor.mode {
        UserEditMode::Create => "New User",
        UserEditMode::EditRole => "Change Role",
        UserEditMode::ChangePassword => "Change Password",
    };
    let popup = centered_rect(64, 60, frame.area());
    frame.render_widget(Clear, popup);

    let outer = Block::default().borders(Borders::ALL).title(title);
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let focused = Style::default().fg(Color::Yellow);
    let normal = Style::default();

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
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Username")
                        .border_style(if editor.field == UserEditField::Username {
                            focused
                        } else {
                            normal
                        }),
                ),
                layout[0],
            );
            let pw_mask = "*".repeat(editor.password.len());
            frame.render_widget(
                Paragraph::new(pw_mask).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Password")
                        .border_style(if editor.field == UserEditField::Password {
                            focused
                        } else {
                            normal
                        }),
                ),
                layout[1],
            );
            let cpw_mask = "*".repeat(editor.confirm_password.len());
            frame.render_widget(
                Paragraph::new(cpw_mask).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Confirm Password")
                        .border_style(if editor.field == UserEditField::ConfirmPassword {
                            focused
                        } else {
                            normal
                        }),
                ),
                layout[2],
            );
            let role_str = format!("{:?}  (Space to cycle)", editor.role);
            frame.render_widget(
                Paragraph::new(role_str).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Role")
                        .border_style(if editor.field == UserEditField::Role {
                            focused
                        } else {
                            normal
                        }),
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
            let label = Paragraph::new(format!("User: {}", editor.username)).style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
            frame.render_widget(label, layout[0]);
            let role_str = format!("{:?}  (Space to cycle)", editor.role);
            frame.render_widget(
                Paragraph::new(role_str).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Role")
                        .border_style(focused),
                ),
                layout[1],
            );
            let help = Paragraph::new("Space cycle | Enter save | Esc cancel")
                .alignment(Alignment::Center);
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
            let label = Paragraph::new(format!("User: {}", editor.username)).style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
            frame.render_widget(label, layout[0]);
            let cpw_mask = "*".repeat(editor.current_password.len());
            frame.render_widget(
                Paragraph::new(cpw_mask).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Current Password")
                        .border_style(if editor.field == UserEditField::CurrentPassword {
                            focused
                        } else {
                            normal
                        }),
                ),
                layout[1],
            );
            let pw_mask = "*".repeat(editor.password.len());
            frame.render_widget(
                Paragraph::new(pw_mask).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("New Password")
                        .border_style(if editor.field == UserEditField::Password {
                            focused
                        } else {
                            normal
                        }),
                ),
                layout[2],
            );
            let confirm_mask = "*".repeat(editor.confirm_password.len());
            frame.render_widget(
                Paragraph::new(confirm_mask).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Confirm New Password")
                        .border_style(if editor.field == UserEditField::ConfirmPassword {
                            focused
                        } else {
                            normal
                        }),
                ),
                layout[3],
            );
            let help = Paragraph::new("Tab/↑↓ field | Enter save | Esc cancel")
                .alignment(Alignment::Center);
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
        let duration = device
            .attributes
            .get("duration_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
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
    device
        .attributes
        .get("remaining_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
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
        Tab::Rules => app.visible_rules().is_empty(),
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
        AdminSubPanel::Matter => 1,
        AdminSubPanel::Status => 2,
        AdminSubPanel::Users => 3,
        AdminSubPanel::Logs => 4,
        AdminSubPanel::Events => 5,
        AdminSubPanel::Audit => 6,
        AdminSubPanel::Backup => 7,
    };
    let sub_tabs = Tabs::new(vec![
        Line::from("Modes"),
        Line::from("Matter"),
        Line::from("Status"),
        Line::from("Users"),
        Line::from("Logs"),
        Line::from("Events"),
        Line::from("Audit"),
        Line::from("Backup"),
    ])
    .select(active_idx)
    .block(Block::default().borders(Borders::ALL).title("Manage"))
    .style(Style::default().fg(Color::Gray))
    .highlight_style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(sub_tabs, layout[0]);

    let highlight = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    if matches!(app.admin_sub, AdminSubPanel::Status) {
        draw_status_tab(frame, app, layout[1]);
        return;
    }

    if matches!(app.admin_sub, AdminSubPanel::Matter) {
        let matter_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Length(8)])
            .split(layout[1]);

        let items = app
            .matter_nodes
            .iter()
            .map(|n| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {:<30}", n.node_id),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("ep {:<4}", n.endpoint),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        format!("  clusters:{}", n.clusters.len()),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect::<Vec<_>>();

        let pending_badge = if app.matter_pending { " [pending]" } else { "" };

        let list_title = format!("Matter Nodes ({}){}", app.matter_nodes.len(), pending_badge);

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(list_title))
            .highlight_style(highlight)
            .highlight_symbol(">> ");
        let mut state = ratatui::widgets::ListState::default();
        if !app.matter_nodes.is_empty() {
            state.select(Some(app.selected.min(app.matter_nodes.len() - 1)));
        }

        frame.render_stateful_widget(list, matter_layout[0], &mut state);

        let mut activity_lines = vec![Line::from(vec![
            Span::styled("Last: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                app.matter_last_action.as_str(),
                Style::default().fg(Color::White),
            ),
        ])];

        activity_lines.push(Line::from(vec![
            Span::styled("Metric: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                app.matter_last_metric
                    .as_deref()
                    .unwrap_or("No Matter plugin_metrics event received yet"),
                Style::default().fg(Color::LightBlue),
            ),
        ]));

        if let Some(reason) = app.matter_blocked_reason.as_deref() {
            activity_lines.push(Line::from(vec![
                Span::styled("Blocked: ", Style::default().fg(Color::Red)),
                Span::styled(reason, Style::default().fg(Color::LightRed)),
            ]));

            if app.matter_blocked_suggestions.is_empty() {
                activity_lines.push(Line::from(Span::styled(
                    "Retry with device in pairing mode and confirm mDNS/LAN reachability",
                    Style::default().fg(Color::Yellow),
                )));
            } else {
                for suggestion in app.matter_blocked_suggestions.iter().take(2) {
                    activity_lines.push(Line::from(vec![
                        Span::styled("- ", Style::default().fg(Color::DarkGray)),
                        Span::styled(suggestion.as_str(), Style::default().fg(Color::Yellow)),
                    ]));
                }
            }
        }

        if app.matter_activity.is_empty() {
            activity_lines.push(Line::from(Span::styled(
                "No recent Matter activity",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for line in app.matter_activity.iter().take(3) {
                activity_lines.push(Line::from(Span::styled(
                    line.as_str(),
                    Style::default().fg(Color::Gray),
                )));
            }
        }

        let activity = Paragraph::new(activity_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Matter Activity"),
            )
            .wrap(Wrap { trim: true });
        frame.render_widget(activity, matter_layout[1]);
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

    if matches!(app.admin_sub, AdminSubPanel::Audit) {
        draw_audit_tab(frame, app, layout[1]);
        return;
    }

    if matches!(app.admin_sub, AdminSubPanel::Backup) {
        draw_backup_tab(frame, app, layout[1]);
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
            let items = app
                .modes
                .iter()
                .map(|m| {
                    let on = m
                        .state
                        .as_ref()
                        .and_then(|s| s.attributes.get("on"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let (dot, dot_color) = if on {
                        ("●", Color::Green)
                    } else {
                        ("○", Color::DarkGray)
                    };
                    let kind_color = match m.config.kind.as_str() {
                        "solar" => Color::Yellow,
                        "manual" => Color::Cyan,
                        _ => Color::White,
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("  {dot} "), Style::default().fg(dot_color)),
                        Span::styled(
                            format!("{:<28}", m.config.id),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            format!("  {:<8}", m.config.kind),
                            Style::default().fg(kind_color),
                        ),
                        Span::styled(
                            format!("  {}", m.config.name),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                })
                .collect();
            let len = app.modes.len();
            (items, "Modes", len)
        }
        AdminSubPanel::Matter => (Vec::new(), "Matter", 0),
        AdminSubPanel::Status => (Vec::new(), "Status", 0),
        AdminSubPanel::Users => (Vec::new(), "Users", 0),
        AdminSubPanel::Logs => (Vec::new(), "Logs", 0),
        AdminSubPanel::Events => (Vec::new(), "Events", 0),
        AdminSubPanel::Audit => (Vec::new(), "Audit", 0),
        AdminSubPanel::Backup => (Vec::new(), "Backup", 0),
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
            let is_self = app
                .current_user
                .as_ref()
                .map(|me| me.id == u.id)
                .unwrap_or(false);
            let role_str = format!("{:?}", u.role);
            let role_color = match u.role {
                crate::api::Role::Admin => Color::Yellow,
                crate::api::Role::User => Color::White,
                crate::api::Role::ReadOnly => Color::DarkGray,
                crate::api::Role::Observer => Color::Gray,
                crate::api::Role::DeviceOperator => Color::Cyan,
                crate::api::Role::RuleEditor => Color::Magenta,
                crate::api::Role::ServiceOperator => Color::Blue,
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
    let normal = Style::default();

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(editor.id.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title("ID  (switch_ prefix added automatically)")
                .border_style(if editor.field == SwitchEditField::Id {
                    focused
                } else {
                    normal
                }),
        ),
        layout[0],
    );
    frame.render_widget(
        Paragraph::new(editor.label.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Label  (optional)")
                .border_style(if editor.field == SwitchEditField::Label {
                    focused
                } else {
                    normal
                }),
        ),
        layout[1],
    );
    frame.render_widget(
        Paragraph::new("Tab field  |  Enter create  |  Esc cancel").alignment(Alignment::Center),
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
    let normal = Style::default();

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(editor.id.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title("ID  (timer_ prefix added automatically)")
                .border_style(if editor.field == TimerEditField::Id {
                    focused
                } else {
                    normal
                }),
        ),
        layout[0],
    );
    frame.render_widget(
        Paragraph::new(editor.label.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Label  (optional)")
                .border_style(if editor.field == TimerEditField::Label {
                    focused
                } else {
                    normal
                }),
        ),
        layout[1],
    );
    frame.render_widget(
        Paragraph::new("Tab field  |  Enter create  |  Esc cancel").alignment(Alignment::Center),
        layout[2],
    );
}

// ── Glue creator modal ────────────────────────────────────────────────────────
//
// Centered overlay. Shows the type cycler at the top, then common fields,
// then any type-specific fields per GlueCreator::fields_for_type. Highlights
// the current cursor field and surfaces save errors at the bottom.

fn draw_glue_creator(frame: &mut Frame<'_>, creator: &GlueCreator) {
    let area = centered_rect(64, 60, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title("Create Glue Device")
        .style(Style::default().fg(Color::White));
    frame.render_widget(block, area);

    let inner = Rect {
        x: area.x + 2,
        y: area.y + 2,
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(4),
    };

    let active = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let inactive = Style::default().fg(Color::DarkGray);

    let row_style = |field: GlueEditField| {
        if creator.field == field {
            active
        } else {
            inactive
        }
    };

    // Visible fields drive layout — only render the ones relevant to type.
    let visible = GlueCreator::fields_for_type(creator.glue_type);
    let mut lines: Vec<Line> = Vec::new();

    // Type row — always present.
    lines.push(Line::from(vec![
        Span::styled(format!("{:<14}", "type"), row_style(GlueEditField::Type)),
        Span::raw("  "),
        Span::styled(
            format!("{}  (Space to cycle)", creator.glue_type.as_str()),
            Style::default().fg(Color::Cyan),
        ),
    ]));
    lines.push(Line::from(""));

    // ID row.
    if visible.contains(&GlueEditField::Id) {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<14}", "id"), row_style(GlueEditField::Id)),
            Span::raw("  "),
            Span::styled(creator.id.clone(), Style::default().fg(Color::White)),
            Span::styled(
                if creator.field == GlueEditField::Id {
                    "_"
                } else {
                    ""
                },
                Style::default().fg(Color::Cyan),
            ),
        ]));
    }

    // Name row.
    if visible.contains(&GlueEditField::Name) {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<14}", "name"), row_style(GlueEditField::Name)),
            Span::raw("  "),
            Span::styled(creator.name.clone(), Style::default().fg(Color::White)),
            Span::styled(
                if creator.field == GlueEditField::Name {
                    "_"
                } else {
                    ""
                },
                Style::default().fg(Color::Cyan),
            ),
        ]));
    }

    // Type-specific fields.
    if visible.contains(&GlueEditField::Options) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "select options (comma-separated, e.g. \"red,green,blue\"):",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<14}", "options"),
                row_style(GlueEditField::Options),
            ),
            Span::raw("  "),
            Span::styled(creator.options.clone(), Style::default().fg(Color::White)),
            Span::styled(
                if creator.field == GlueEditField::Options {
                    "_"
                } else {
                    ""
                },
                Style::default().fg(Color::Cyan),
            ),
        ]));
    }
    if visible.contains(&GlueEditField::Members) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "group members (comma-separated device IDs):",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<14}", "members"),
                row_style(GlueEditField::Members),
            ),
            Span::raw("  "),
            Span::styled(creator.members.clone(), Style::default().fg(Color::White)),
            Span::styled(
                if creator.field == GlueEditField::Members {
                    "_"
                } else {
                    ""
                },
                Style::default().fg(Color::Cyan),
            ),
        ]));
    }
    if visible.contains(&GlueEditField::SourceDeviceId) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "threshold tracks one numeric attribute on a source device:",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<14}", "source dev"),
                row_style(GlueEditField::SourceDeviceId),
            ),
            Span::raw("  "),
            Span::styled(
                creator.source_device_id.clone(),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                if creator.field == GlueEditField::SourceDeviceId {
                    "_"
                } else {
                    ""
                },
                Style::default().fg(Color::Cyan),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<14}", "source attr"),
                row_style(GlueEditField::SourceAttribute),
            ),
            Span::raw("  "),
            Span::styled(
                creator.source_attribute.clone(),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                if creator.field == GlueEditField::SourceAttribute {
                    "_"
                } else {
                    ""
                },
                Style::default().fg(Color::Cyan),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<14}", "threshold"),
                row_style(GlueEditField::Threshold),
            ),
            Span::raw("  "),
            Span::styled(creator.threshold.clone(), Style::default().fg(Color::White)),
            Span::styled(
                if creator.field == GlueEditField::Threshold {
                    "_"
                } else {
                    ""
                },
                Style::default().fg(Color::Cyan),
            ),
        ]));
    }

    // Error row, if any.
    if let Some(err) = &creator.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("Error: {err}"),
            Style::default().fg(Color::Red),
        )));
    }

    let para = Paragraph::new(lines)
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, inner);
}

fn draw_mode_editor(frame: &mut Frame<'_>, editor: &ModeEditor) {
    let popup = centered_rect(64, 50, frame.area());
    frame.render_widget(Clear, popup);
    let outer = Block::default().borders(Borders::ALL).title("New Mode");
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let focused = Style::default().fg(Color::Yellow);
    let normal = Style::default();

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
            Block::default()
                .borders(Borders::ALL)
                .title("ID  (must start with mode_)")
                .border_style(if editor.field == ModeEditField::Id {
                    focused
                } else {
                    normal
                }),
        ),
        layout[0],
    );
    frame.render_widget(
        Paragraph::new(editor.name.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Name")
                .border_style(if editor.field == ModeEditField::Name {
                    focused
                } else {
                    normal
                }),
        ),
        layout[1],
    );
    let kind_color = match editor.kind {
        ModeKind::Solar => Color::Yellow,
        ModeKind::Manual => Color::Cyan,
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!("{}  (Space to toggle)", editor.kind.as_str()),
            Style::default().fg(kind_color),
        ))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Kind")
                .border_style(if editor.field == ModeEditField::Kind {
                    focused
                } else {
                    normal
                }),
        ),
        layout[2],
    );
    frame.render_widget(
        Paragraph::new("Tab field  |  Space cycle kind  |  Enter create  |  Esc cancel")
            .alignment(Alignment::Center),
        layout[3],
    );
}

fn draw_matter_commission_editor(frame: &mut Frame<'_>, editor: &MatterCommissionEditor) {
    let popup = centered_rect(72, 68, frame.area());
    frame.render_widget(Clear, popup);
    let outer = Block::default()
        .borders(Borders::ALL)
        .title("Matter Commission");
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let focused = Style::default().fg(Color::Yellow);
    let normal = Style::default();

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(editor.pairing_code.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Pairing Code (manual code)")
                .border_style(if editor.field == MatterCommissionField::PairingCode {
                    focused
                } else {
                    normal
                }),
        ),
        layout[0],
    );
    frame.render_widget(
        Paragraph::new(editor.name.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Name (optional)")
                .border_style(if editor.field == MatterCommissionField::Name {
                    focused
                } else {
                    normal
                }),
        ),
        layout[1],
    );
    frame.render_widget(
        Paragraph::new(editor.room.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Room (optional)")
                .border_style(if editor.field == MatterCommissionField::Room {
                    focused
                } else {
                    normal
                }),
        ),
        layout[2],
    );
    frame.render_widget(
        Paragraph::new(editor.discriminator.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Discriminator (optional)")
                .border_style(if editor.field == MatterCommissionField::Discriminator {
                    focused
                } else {
                    normal
                }),
        ),
        layout[3],
    );
    frame.render_widget(
        Paragraph::new(editor.passcode.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Passcode (optional)")
                .border_style(if editor.field == MatterCommissionField::Passcode {
                    focused
                } else {
                    normal
                }),
        ),
        layout[4],
    );
    frame.render_widget(
        Paragraph::new("Tab field  |  Enter commission  |  Esc cancel")
            .alignment(Alignment::Center),
        layout[5],
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
