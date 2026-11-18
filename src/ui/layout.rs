//! The frame: left menu, tab body, and the status bar with key hints.

use super::{devices, manage, plugins, rules};
use crate::app::{AdminSubPanel, App, DeviceSubPanel, PluginDetailPanel, Tab};
use crate::text::{char_len, truncate};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn draw_main(frame: &mut Frame<'_>, app: &App) {
    let footer_height = compute_footer_height(app, frame.area().width);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(footer_height)])
        .split(frame.area());
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(20), Constraint::Min(10)])
        .split(rows[0]);

    draw_menu_sidebar(frame, app, cols[0]);
    draw_tab_body(frame, app, cols[1]);
    draw_status_bar(frame, app, rows[1]);
}

fn draw_menu_sidebar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let active = app.active_tab();
    let items: Vec<ListItem> = app
        .tabs()
        .iter()
        .enumerate()
        .map(|(idx, tab)| {
            let on = *tab == active;
            let prefix = if on { "► " } else { "  " };
            let style = if on {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(Span::styled(
                format!("{prefix}{}) {}", idx + 1, tab.title()),
                style,
            ))
        })
        .collect();
    let list = List::new(items).block(Block::default().borders(Borders::RIGHT).title("Menu"));
    frame.render_widget(list, area);
}

fn draw_tab_body(frame: &mut Frame<'_>, app: &App, area: Rect) {
    match app.active_tab() {
        Tab::Devices => devices::draw_devices_tab(frame, app, area),
        Tab::Scenes => devices::draw_scenes_table(frame, app, area),
        Tab::Areas => devices::draw_areas_pane(frame, app, area),
        Tab::Rules => rules::draw_rules_tab(frame, app, area),
        Tab::Plugins => plugins::draw_plugins_tab(frame, app, area),
        Tab::Manage => manage::draw_manage_tab(frame, app, area),
    }
}

fn draw_status_bar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let role = app
        .current_user
        .as_ref()
        .map(|u| u.role.as_str())
        .unwrap_or("unknown");
    let user = app
        .current_user
        .as_ref()
        .map(|u| u.username.as_str())
        .unwrap_or("n/a");
    let live = if app.ws_connected { "●" } else { "○" };
    let busy = if app.busy() {
        format!("{} ", app.spinner())
    } else {
        String::new()
    };

    let text = match &app.error {
        Some(err) => format!("ERROR: {err}"),
        None => app.status.clone(),
    };
    let inner_width = area.width.saturating_sub(2) as usize;
    let status_line = fit_single_line(
        &format!("{busy}{text} | {user} ({role}) {live}"),
        inner_width,
    );
    let style = if app.error.is_some() {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    };

    let mut lines = vec![Line::from(Span::styled(status_line, style))];
    if let Some(line) = sync_error_line(app, inner_width) {
        lines.push(Line::from(Span::styled(
            line,
            Style::default().fg(Color::Red),
        )));
    }
    for (idx, text) in fit_hints_lines(&status_hints(app), inner_width)
        .iter()
        .enumerate()
    {
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

/// `"sync failed: rules (500: ...), users (403: ...)"` when the last sync
/// had failures.
fn sync_error_line(app: &App, width: usize) -> Option<String> {
    if app.sync_errors.is_empty() {
        return None;
    }
    let parts: Vec<String> = app
        .sync_errors
        .iter()
        .map(|(section, err)| format!("{section} ({})", truncate(err, 60)))
        .collect();
    Some(fit_single_line(
        &format!("sync failed: {}", parts.join(", ")),
        width,
    ))
}

fn compute_footer_height(app: &App, width: u16) -> u16 {
    let inner = width.saturating_sub(2) as usize;
    let hints = fit_hints_lines(&status_hints(app), inner).len();
    let sync = usize::from(!app.sync_errors.is_empty());
    (1 + sync + hints + 2) as u16
}

fn status_hints(app: &App) -> Vec<&'static str> {
    if app.confirm.is_some() {
        return vec!["y confirm", "n/Esc cancel"];
    }
    if let Some(s) = &app.plugins_tab.streaming {
        if s.stage.is_terminal() {
            return vec!["Esc close"];
        }
        if s.pending_prompt.is_some() {
            return vec![
                "type response",
                "Enter send",
                "Ctrl+X cancel run",
                "Esc close",
            ];
        }
        return vec!["c cancel", "Esc close"];
    }
    if app.rules_tab.detail.is_some() {
        return vec!["j/k scroll", "PgUp/PgDn", "r refresh", "Esc back", "q quit"];
    }
    if app.active_tab() == Tab::Plugins
        && let Some(d) = &app.plugins_tab.detail
    {
        let mut h = vec![
            "1/2/3 panel",
            "◄/► panel",
            "r refresh",
            "Esc close",
            "q quit",
        ];
        if d.panel == PluginDetailPanel::Actions {
            h.insert(0, "Enter run");
            h.insert(0, "j/k move");
        }
        return h;
    }
    if app.devices_tab.search_open {
        return vec!["type to search", "Enter apply", "Esc close"];
    }
    if app.rules_tab.filter_bar.is_some() {
        return vec!["type filter", "Tab field", "Enter apply", "Esc cancel"];
    }
    if app.manage.logs.input_open {
        return vec!["type filter", "Enter apply", "Esc cancel"];
    }
    if app.rules_tab.groups.is_some() {
        return vec!["j/k move", "e enable", "d disable", "x delete", "Esc close"];
    }
    if app.devices_tab.editor.is_some() {
        return vec!["Tab field", "Enter save", "Esc cancel"];
    }
    if app.areas_tab.editor.is_some() {
        return vec!["Enter save", "Esc cancel"];
    }
    if app.manage.user_editor.is_some() {
        return vec!["Tab field", "Space cycle role", "Enter save", "Esc cancel"];
    }
    if app.manage.glue_creator.is_some() {
        return vec!["Tab field", "Space type", "Enter create", "Esc cancel"];
    }
    if app.manage.mode_editor.is_some() {
        return vec!["Tab field", "Space kind", "Enter create", "Esc cancel"];
    }
    if app.manage.matter.editor.is_some() {
        return vec!["Tab field", "Enter commission", "Esc cancel"];
    }

    let mut hints = vec![
        "Tab/Shift+Tab menu",
        "1-9 jump tab",
        "j/k move",
        "r refresh",
        "q quit",
        "T time",
        "L logout",
    ];
    match app.active_tab() {
        Tab::Devices => {
            hints.push("◄/► panel");
            match app.devices_tab.sub {
                DeviceSubPanel::All => hints.extend([
                    "Spc toggle",
                    "t toggle",
                    "+/- brightness",
                    "l/u lock",
                    "v grouped/flat",
                    "f filter",
                    "s sort",
                    "/ search",
                    "Enter edit",
                    "d delete",
                ]),
                DeviceSubPanel::MediaPlayers => hints.extend([
                    "Spc/t play-stop",
                    "p play/pause",
                    "x stop",
                    "n next",
                    "b previous",
                    "+/- volume",
                    "m mute",
                    "Enter edit",
                ]),
            }
        }
        Tab::Scenes => hints.push("a activate"),
        Tab::Areas => match app.areas_tab.pane {
            crate::app::AreasPane::AreasList => {
                hints.extend(["n new area", "Enter/► devices", "d delete area"])
            }
            crate::app::AreasPane::DeviceList => hints.extend([
                "◄ areas",
                "Enter rename area",
                "Spc mark device",
                "-/d remove marked",
                "n new area",
            ]),
        },
        Tab::Plugins => {
            hints.push("Enter detail");
            if app.is_admin() {
                hints.push("d uninstall");
            }
        }
        Tab::Manage => {
            hints.push("◄/► panel");
            hints.push("g helper");
            match app.manage.sub {
                AdminSubPanel::Matter => {
                    hints.extend(["c commission", "i reinterview", "d remove"])
                }
                AdminSubPanel::Status => {}
                AdminSubPanel::Users => {
                    hints.extend(["n new", "Enter role", "p password", "d delete"])
                }
                AdminSubPanel::Logs => {
                    hints.extend(["p/Spc pause", "e/w/i/d level", "/ filter", "c clear"])
                }
                AdminSubPanel::Events => hints.push("f type filter"),
                AdminSubPanel::Audit => {
                    hints.extend(["n next page", "p prev page", "Enter detail"])
                }
                AdminSubPanel::Backup => hints.push("Enter run"),
                AdminSubPanel::Modes => hints.extend(["n new", "d delete"]),
            }
        }
        Tab::Rules => hints.extend([
            "Enter detail",
            "e enable",
            "d disable",
            "c clone",
            "x delete",
            "h history",
            "f filter",
            "s stale",
            "g groups",
            "Spc select",
        ]),
    }
    hints
}

pub fn fit_single_line(input: &str, max_chars: usize) -> String {
    truncate(input, max_chars)
}

pub fn fit_hints_lines(hints: &[&str], max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return vec![String::new()];
    }
    let body_width = max_chars.saturating_sub(6).max(1);
    let sep = " | ";
    let sep_len = char_len(sep);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut used = 0usize;
    for hint in hints {
        let hint_len = char_len(hint);
        let join = if current.is_empty() { 0 } else { sep_len };
        if used + join + hint_len > body_width {
            if current.is_empty() {
                lines.push(fit_single_line(hint, body_width));
                continue;
            }
            lines.push(std::mem::take(&mut current));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_wrap_by_characters() {
        let lines = fit_hints_lines(&["◄/► panel", "a", "b"], 20);
        assert!(lines.iter().all(|l| char_len(l) <= 14));
        assert_eq!(fit_hints_lines(&[], 10), vec![String::new()]);
    }
}
