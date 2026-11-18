//! Automations: the list, the selected one's When / If / Then and Recent
//! runs, the read-only RON, the dry run and the device picker.

use super::widgets::{centered_rect, centered_rect_min, heading, highlight};
use crate::app::automations::{Tone, run_line, test_lines};
use crate::app::rule_words::{action_words, condition_words, trigger_words};
use crate::app::{App, Load};
use crate::model::{Rule, RuleFiring};
use crate::text::take_chars;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, TableState, Wrap,
    },
};

pub fn draw_rules_tab(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.rules_tab.detail.is_some() {
        draw_rule_detail(frame, app, area);
        return;
    }
    let pane = if area.height >= 18 && app.selected_rule().is_some() {
        (area.height / 2).clamp(8, 14)
    } else {
        0
    };
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(pane)])
        .split(area);
    draw_rules_list(frame, app, parts[0]);
    if pane > 0
        && let Some(r) = app.selected_rule()
    {
        draw_summary(frame, app, r, parts[1]);
    }
}

fn tone_style(t: Tone) -> Style {
    match t {
        Tone::Good => Style::default().fg(Color::Green),
        Tone::Bad => Style::default().fg(Color::Red),
        Tone::Quiet => Style::default().fg(Color::Gray),
    }
}

fn run_items(
    app: &App,
    rule: Option<&Rule>,
    load: Option<&[RuleFiring]>,
    take: usize,
) -> Vec<ListItem<'static>> {
    let dim = Style::default().fg(Color::DarkGray);
    let Some(runs) = load else {
        let text = if app.rules_tab.extra.runs_loaded {
            "no recent runs"
        } else {
            "loading..."
        };
        return vec![ListItem::new(Line::from(Span::styled(text, dim)))];
    };
    if runs.is_empty() {
        return vec![ListItem::new(Line::from(Span::styled(
            "no recent runs since homeCore started",
            dim,
        )))];
    }
    let name_of = |id: &str| app.rule_device_name(id);
    runs.iter()
        .take(take)
        .map(|f| {
            let (s, t) = run_line(f, rule, &name_of, app.time_utc);
            ListItem::new(Line::from(Span::styled(format!("  {s}"), tone_style(t))))
        })
        .collect()
}

/// When / If / Then for the selected rule, then its last runs.
fn draw_summary(frame: &mut Frame<'_>, app: &App, r: &Rule, area: Rect) {
    let name_of = |id: &str| app.rule_device_name(id);
    let when: Vec<String> = r
        .trigger
        .iter()
        .chain(r.triggers.iter())
        .map(|t| trigger_words(t, &name_of))
        .collect();
    let checks: Vec<String> = r
        .conditions
        .iter()
        .map(|c| condition_words(c, &name_of))
        .collect();
    let steps: Vec<String> = r
        .actions
        .iter()
        .map(|a| action_words(a, &name_of))
        .collect();
    let label = |s: &'static str| Span::styled(format!("{s:<6}"), heading());
    let join = |v: &[String], sep: &str, none: &str| {
        if v.is_empty() {
            none.to_string()
        } else {
            v.join(sep)
        }
    };
    let mut lines = vec![
        Line::from(vec![label("When"), Span::raw(join(&when, " or ", "never"))]),
        Line::from(vec![
            label("If"),
            Span::raw(join(&checks, " and ", "always")),
        ]),
        Line::from(vec![
            label("Then"),
            Span::raw(join(&steps, ", ", "nothing")),
        ]),
    ];
    if let Some(e) = &r.error {
        lines.push(Line::from(Span::styled(
            format!("homeCore can't run it: {e}"),
            Style::default().fg(Color::Red),
        )));
    }
    if let Some(m) = app.rules_tab.extra.missing.get(&r.id) {
        let names: Vec<String> = m
            .iter()
            .map(|id| {
                let n = app.rule_device_name(id);
                n.strip_prefix("missing: ").map(str::to_string).unwrap_or(n)
            })
            .collect();
        lines.push(Line::from(Span::styled(
            format!(
                "It names devices homeCore has no device for: {}. F sends it to another.",
                names.join(", ")
            ),
            Style::default().fg(Color::Red),
        )));
    }
    if app.rule_paused(&r.id) {
        lines.push(Line::from(Span::styled(
            "Another rule paused it: its moment does not run it until it is resumed (u).",
            Style::default().fg(Color::Yellow),
        )));
    }
    let head = lines.len() as u16 + 1;
    let block = Block::default()
        .borders(Borders::TOP)
        .title(format!(" {} ", r.name));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(head), Constraint::Min(1)])
        .split(inner);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), parts[0]);
    let runs = app.rules_tab.extra.runs.get(&r.id).map(Vec::as_slice);
    let take = parts[1].height.saturating_sub(1) as usize;
    let mut items = vec![ListItem::new(Line::from(Span::styled(
        "Recent runs",
        heading(),
    )))];
    items.extend(run_items(app, Some(r), runs, take.max(1)));
    frame.render_widget(List::new(items), parts[1]);
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
            Constraint::Length(12),
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
                if app.rule_paused(&r.id) {
                    Span::styled("‖ paused", Style::default().fg(Color::Yellow))
                } else if r.enabled {
                    Span::styled("● on", Style::default().fg(Color::Green))
                } else {
                    Span::styled("○ off", Style::default().fg(Color::DarkGray))
                },
                Span::raw(format!("  · priority {}", r.priority)),
            ];
            if !r.tags.is_empty() {
                spans.push(Span::raw(format!("  · tags {}", r.tags.join(", "))));
            }
            let mut lines = vec![
                Line::from(spans),
                Line::from(Span::styled(
                    format!("When {}", app.rule_trigger_text(r)),
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            if let Some(err) = &r.error {
                lines.push(Line::from(Span::styled(
                    format!("homeCore can't run it: {err}"),
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
                    .title(" RON, read only: edit in the app or the file "),
            )
            .style(ron_style)
            .scroll((d.scroll, 0))
            .wrap(Wrap { trim: false }),
        layout[1],
    );

    let take = (layout[2].height.saturating_sub(2) as usize).max(1);
    let (title, items) = match &d.history {
        Load::Ready(h) => (
            format!(" Recent runs ({}) ", h.len()),
            run_items(app, rule, Some(h), take),
        ),
        Load::Failed(e) => (
            " Recent runs ".to_string(),
            vec![ListItem::new(Line::from(Span::styled(
                format!("history unavailable: {e}"),
                Style::default().fg(Color::Red),
            )))],
        ),
        _ => (
            " Recent runs ".to_string(),
            vec![ListItem::new(Line::from(Span::styled(
                "loading...",
                Style::default().fg(Color::DarkGray),
            )))],
        ),
    };
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(title)),
        layout[2],
    );
}

/// The On column: on, off, paused or needs fixing.
fn mark(app: &App, r: &Rule) -> (&'static str, Color) {
    if app.rules_tab.extra.needs_fixing(r) {
        ("!", Color::Red)
    } else if app.rule_paused(&r.id) {
        ("‖", Color::Yellow)
    } else if r.enabled {
        ("●", Color::Green)
    } else {
        ("○", Color::DarkGray)
    }
}

/// `Automations  42 · 3 paused · 1 needs fixing`.
fn list_title(app: &App) -> String {
    let total = app.rules.len();
    let shown = app.views.rules.len();
    let mut parts = vec![if shown != total {
        format!("{shown} of {total}")
    } else {
        total.to_string()
    }];
    let paused = app.rules.iter().filter(|r| app.rule_paused(&r.id)).count();
    if paused > 0 {
        parts.push(format!("{paused} paused"));
    }
    let fixing = app
        .rules
        .iter()
        .filter(|r| app.rules_tab.extra.needs_fixing(r))
        .count();
    if fixing > 0 {
        parts.push(format!("{fixing} needs fixing"));
    }
    let marked = app.rules_tab.marked.len();
    if marked > 0 {
        parts.push(format!("{marked} marked"));
    }
    format!(" Automations  {} ", parts.join(" · "))
}

fn draw_rules_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let tab = &app.rules_tab;
    let title = list_title(app);
    let header = Row::new(
        ["", "On", "Name", "Trigger", "Last run", "Group"]
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
            let marked = tab.marked.contains(&r.id);
            let (dot, dot_color) = mark(app, r);
            let fixing = app.rules_tab.extra.needs_fixing(r);
            let name = take_chars(&format!("{}{}", if marked { "✓ " } else { "" }, r.name), 40);
            let base = if fixing {
                Style::default().fg(Color::Red)
            } else if r.enabled {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let name_style = if marked {
                base.add_modifier(Modifier::BOLD).fg(Color::Yellow)
            } else {
                base
            };
            Row::new(vec![
                Cell::from(if is_sel { "▸" } else { " " }),
                Cell::from(format!(" {dot}")).style(Style::default().fg(dot_color)),
                Cell::from(name).style(name_style),
                Cell::from(take_chars(&app.rule_trigger_text(r), 28)).style(if fixing {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Gray)
                }),
                Cell::from(app.rule_last_run(r)).style(Style::default().fg(Color::DarkGray)),
                Cell::from(take_chars(&app.rule_group(r), 18))
                    .style(Style::default().fg(Color::Cyan)),
            ])
        })
        .collect();
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(if app.rules.is_empty() {
                "No automations. They are written in the app or as RON files."
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
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(24),
            Constraint::Length(28),
            Constraint::Length(12),
            Constraint::Length(18),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(title))
    .row_highlight_style(highlight());
    let mut state = TableState::default();
    state.select(sel_row);
    frame.render_stateful_widget(table, area, &mut state);
}

// ── Overlays ────────────────────────────────────────────────────────────────

/// The keys of an open dry run or device picker.
pub fn modal_hints(app: &App) -> Option<Vec<&'static str>> {
    if let Some(t) = &app.rules_tab.extra.test {
        return Some(if t.editing {
            vec![
                "type a time like 22:30, or nothing for now",
                "Enter test",
                "Esc close",
            ]
        } else {
            vec!["j/k scroll", "t another time", "Esc close"]
        });
    }
    if app.rules_tab.extra.fix.is_some() {
        return Some(vec![
            "type to search",
            "↑/↓ move",
            "Tab next missing device",
            "Enter choose",
            "Esc cancel",
        ]);
    }
    None
}

pub fn draw_rule_overlays(frame: &mut Frame<'_>, app: &App) {
    draw_test(frame, app);
    draw_fix(frame, app);
}

fn draw_test(frame: &mut Frame<'_>, app: &App) {
    let Some(t) = &app.rules_tab.extra.test else {
        return;
    };
    let area = centered_rect(80, 70, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Test {}: a dry run, nothing is sent ", t.name))
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let dim = Style::default().fg(Color::DarkGray);
    let mut lines = vec![Line::from(vec![
        Span::styled("At  ", heading()),
        if t.editing {
            Span::raw(format!("{}_", t.input))
        } else if t.asked.is_empty() {
            Span::raw("now")
        } else {
            Span::raw(t.asked.clone())
        },
    ])];
    if t.editing {
        lines.push(Line::from(Span::styled(
            "A time today (22:30), a date and time (2026-11-18T07:00), or nothing for now.",
            dim,
        )));
    }
    lines.push(Line::from(""));
    match &t.result {
        Load::Idle => {}
        Load::Loading => lines.push(Line::from(Span::styled("testing...", dim))),
        Load::Failed(e) => lines.push(Line::from(Span::styled(
            format!("Could not test: {e}"),
            Style::default().fg(Color::Red),
        ))),
        Load::Ready(res) => {
            let rule = app.rules.iter().find(|r| r.id == t.rule_id);
            let name_of = |id: &str| app.rule_device_name(id);
            for (i, (s, tone)) in test_lines(rule, res, &t.asked, &name_of)
                .into_iter()
                .enumerate()
            {
                let style = if i == 0 {
                    tone_style(tone).add_modifier(Modifier::BOLD)
                } else {
                    tone_style(tone)
                };
                lines.push(Line::from(Span::styled(s, style)));
            }
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((t.scroll, 0)),
        inner,
    );
}

fn draw_fix(frame: &mut Frame<'_>, app: &App) {
    let Some(p) = &app.rules_tab.extra.fix else {
        return;
    };
    let h = frame.area().height.saturating_sub(4).clamp(6, 24);
    let area = centered_rect_min(40, 90, h, frame.area());
    frame.render_widget(Clear, area);
    let from = p
        .missing
        .get(p.which)
        .map(|m| app.rule_device_name(m))
        .unwrap_or_default();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {}: send {from} to... ", p.name))
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(inner);
    let mut head = vec![Line::from(vec![
        Span::styled("Search ", heading()),
        Span::raw(format!("{}_", p.query)),
    ])];
    if p.missing.len() > 1 {
        head.push(Line::from(Span::styled(
            format!(
                "missing device {} of {} (Tab for the next)",
                p.which + 1,
                p.missing.len()
            ),
            Style::default().fg(Color::DarkGray),
        )));
    }
    frame.render_widget(Paragraph::new(head), parts[0]);
    let list = app.fix_candidates();
    let rows = parts[1].height as usize;
    let start = p.sel.saturating_sub(rows.saturating_sub(1));
    let items: Vec<ListItem> = list
        .iter()
        .enumerate()
        .skip(start)
        .take(rows)
        .map(|(i, (id, name))| {
            let line = format!("{name}  ({id})");
            if i == p.sel {
                ListItem::new(Line::from(Span::styled(line, highlight())))
            } else {
                ListItem::new(line)
            }
        })
        .collect();
    if items.is_empty() {
        frame.render_widget(
            Paragraph::new("No device matches.").style(Style::default().fg(Color::DarkGray)),
            parts[1],
        );
    } else {
        frame.render_widget(List::new(items), parts[1]);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::test_support::*;
    use crate::model::Role;
    use crate::screens::ScreenId;
    use ratatui::{Terminal, backend::TestBackend};
    use serde_json::json;

    fn render(a: &mut crate::app::App, w: u16, h: u16) -> String {
        a.refresh_views();
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| crate::ui::draw(f, a)).unwrap();
        let b = t.backend().buffer().clone();
        (0..b.area.height)
            .map(|y| {
                (0..b.area.width)
                    .map(|x| b[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn house() -> crate::app::App {
        let mut a = signed_in(Role::Admin);
        let mut timer = device("timer_fan", "Laundry Timer");
        timer.plugin_id = "core.glue".into();
        timer.device_type = Some("timer".into());
        let started = (chrono::Utc::now() - chrono::Duration::seconds(60)).to_rfc3339();
        timer.attributes = json!({"state": "running", "duration_secs": 900, "started_at": started})
            .as_object()
            .cloned()
            .unwrap();
        a.devices = vec![
            timer,
            device("porch", "Porch Light"),
            device("vac", "Vacation"),
        ];
        a.rules = vec![
            serde_json::from_value(json!({
                "id": "r1", "name": "Porch light at sunset", "enabled": true,
                "trigger": {"SunEvent": {"event": "Sunset", "offset_minutes": -15}},
                "conditions": [{"DeviceState": {"device_id": "vac", "attribute": "on", "op": "Eq", "value": false}}],
                "actions": [{"action": {"SetDeviceState": {"device_id": "porch", "state": {"on": true, "brightness_pct": 60}}}, "enabled": true}]
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "id": "r2", "name": "Office fan", "enabled": false,
                "trigger": "ManualTrigger",
                "actions": [{"action": {"SetDeviceState": {"device_id": "DELETED:fan", "state": {"on": true}}}, "enabled": true}]
            }))
            .unwrap(),
        ];
        a.rules_tab.extra.paused.insert("r1".into());
        a.rules_tab
            .extra
            .removed
            .insert("fan".into(), "Office Fan".into());
        a.alerts = vec![
            serde_json::from_value(json!({
                "id": "p", "kind": "plugin", "device_id": "porch", "message": "Porch firmware 0.15",
                "severity": "info", "raised_at": "2026-11-17T10:00:00Z", "plugin_id": "plugin.wled",
                "topic": "firmware_update", "action": {"label": "Close it", "id": "close"}
            }))
            .unwrap(),
        ];
        a
    }

    #[test]
    fn automations_show_triggers_marks_and_the_summary() {
        let mut a = house();
        a.screen = ScreenId::Automations;
        let s = render(&mut a, 140, 40);
        assert!(
            s.contains("Automations  2 · 1 paused · 1 needs fixing"),
            "{s}"
        );
        assert!(s.contains("sunset -15m"), "{s}");
        assert!(s.contains("missing: Office Fan"), "{s}");
        assert!(s.contains('‖'), "{s}");
        assert!(s.contains("When  sunset -15m"), "{s}");
        assert!(s.contains("If    Vacation is off"), "{s}");
        assert!(s.contains("Then  Porch Light on 60%"), "{s}");
        assert!(s.contains("u resume"), "{s}");
    }

    #[test]
    fn helpers_count_down_and_alerts_show_their_button() {
        let mut a = house();
        a.screen = ScreenId::Helpers;
        let s = render(&mut a, 120, 20);
        assert!(
            s.contains("running 14:00 left") || s.contains("running 13:59 left"),
            "{s}"
        );
        a.screen = ScreenId::Alerts;
        a.alerts_sel.select_id("p");
        let s = render(&mut a, 120, 30);
        assert!(s.contains("Open (1)"), "{s}");
        assert!(s.contains("From wled: firmware update"), "{s}");
        assert!(s.contains("p press \"Close it\""), "{s}");
        assert!(s.contains("p run its action"), "{s}");
    }

    #[test]
    fn the_dry_run_and_the_picker_render() {
        let mut a = house();
        a.screen = ScreenId::Automations;
        a.refresh_views();
        a.open_rule_test();
        let s = render(&mut a, 120, 30);
        assert!(s.contains("nothing is sent"), "{s}");
        a.rules_tab.extra.test = None;
        a.rules_tab.sel.select_id("r2");
        a.refresh_views();
        a.open_rule_fix();
        let s = render(&mut a, 120, 30);
        assert!(s.contains("send missing: Office Fan to"), "{s}");
        assert!(s.contains("Porch Light  (porch)"), "{s}");
    }
}
