//! The Devices detail pane's controls and history line, the Removed
//! list, and the device picker.

use super::widgets::{clean_plugin_id, highlight, make_bar};
use crate::app::controls::{ControlKind, control_value, sparkline};
use crate::app::{App, Load, format_timestamp_utc};
use crate::text::{take_chars, truncate};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use serde_json::Value;

const LABEL_W: usize = 14;

/// Draw the selected device's controls and history at the top of
/// `area`; return what is left for the rest of the detail.
pub fn draw_controls(frame: &mut Frame<'_>, app: &App, area: Rect) -> Rect {
    let Some(d) = app.selected_device() else {
        return area;
    };
    let controls = app.device_controls(d);
    let loading = app
        .devices_tab
        .controls
        .schemas
        .get(&d.device_id)
        .is_none_or(Load::is_loading);
    let history = app.devices_tab.controls.history.get(&d.device_id);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let focus = app.devices_tab.controls.focus;
    let inner_w = area.width.saturating_sub(2) as usize;

    for (i, c) in controls.iter().enumerate() {
        let focused = focus.map(|f| f.min(controls.len() - 1)) == Some(i);
        let label_style = if focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let mut spans = vec![Span::styled(
            format!(
                "{}{:<LABEL_W$} ",
                if focused { "▸ " } else { "  " },
                take_chars(&c.label, LABEL_W)
            ),
            label_style,
        )];
        let value = control_value(d, c, app.temp_unit);
        let value_style = if focused {
            highlight()
        } else {
            Style::default().fg(Color::White)
        };
        match &c.kind {
            ControlKind::Actions(buttons) => {
                let at = app.devices_tab.controls.button.min(buttons.len() - 1);
                let room = inner_w.saturating_sub(LABEL_W + 3);
                let all: usize = buttons.iter().map(|b| b.label.chars().count() + 3).sum();
                if all > room {
                    // Too many to show: the highlighted one and where it is.
                    let b = &buttons[if focused { at } else { 0 }];
                    let s = if focused {
                        highlight()
                    } else {
                        Style::default().fg(Color::Cyan)
                    };
                    spans.push(Span::styled(
                        format!("[{}]", take_chars(&b.label, room.saturating_sub(10))),
                        s,
                    ));
                    spans.push(Span::styled(
                        format!(" {} of {}", if focused { at + 1 } else { 1 }, buttons.len()),
                        Style::default().fg(Color::DarkGray),
                    ));
                    lines.push(Line::from(spans));
                    continue;
                }
                for (j, b) in buttons.iter().enumerate() {
                    let s = if focused && j == at {
                        highlight()
                    } else {
                        Style::default().fg(Color::Cyan)
                    };
                    spans.push(Span::styled(format!("[{}]", b.label), s));
                    spans.push(Span::raw(" "));
                }
            }
            kind => {
                let shown = if focused {
                    format!("‹ {value} ›")
                } else {
                    format!("  {value}  ")
                };
                spans.push(Span::styled(shown, value_style));
                let rest = match kind {
                    ControlKind::Range { min, max, .. } => {
                        let now = d.attributes.get(&c.attr).and_then(Value::as_f64);
                        now.filter(|_| max > min)
                            .map(|v| format!(" {}", make_bar((v - min) / (max - min), 10)))
                            .unwrap_or_default()
                    }
                    ControlKind::Choice { options, .. } if focused => {
                        let all: Vec<&str> = options.iter().map(|o| o.label()).collect();
                        format!("  {}", all.join(" · "))
                    }
                    _ => String::new(),
                };
                spans.push(Span::styled(rest, Style::default().fg(Color::DarkGray)));
            }
        }
        lines.push(Line::from(spans));
    }
    if loading && controls.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Loading controls...",
            Style::default().fg(Color::DarkGray),
        )));
    }
    if let Some(h) = history {
        let label = format!("  {:<LABEL_W$} ", "History");
        let text = match &h.values {
            Load::Ready(v) if v.is_empty() => "nothing in the last 24h".to_string(),
            Load::Ready(v) => {
                let w = inner_w.saturating_sub(LABEL_W + 3 + 24).clamp(4, 40);
                let lo = v.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                format!(
                    "{}  last 24h, {} {}..{}",
                    sparkline(v, w),
                    crate::text::normalize_label(&h.attr).to_lowercase(),
                    short_num(lo),
                    short_num(hi)
                )
            }
            Load::Failed(e) => format!("not available ({})", truncate(e, 40)),
            _ => "loading...".to_string(),
        };
        lines.push(Line::from(vec![
            Span::styled(label, Style::default().fg(Color::DarkGray)),
            Span::styled(text, Style::default().fg(Color::Cyan)),
        ]));
    }
    if lines.is_empty() {
        return area;
    }
    let want = lines.len() as u16 + 2;
    let h = want.min(area.height.saturating_sub(3).max(area.height / 2));
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(h), Constraint::Min(0)])
        .split(area);
    let title = if focus.is_some() {
        " Controls  ←/→ change  Tab next "
    } else {
        " Controls  Tab to set "
    };
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title)),
        parts[0],
    );
    parts[1]
}

fn short_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// `Nov 11 18:25`, local or UTC.
fn removed_when(ts: &str, utc: bool) -> String {
    use chrono::{DateTime, Local, Utc};
    match DateTime::parse_from_rfc3339(ts) {
        Ok(dt) if utc => dt.with_timezone(&Utc).format("%b %d %H:%M UTC").to_string(),
        Ok(dt) => dt.with_timezone(&Local).format("%b %d %H:%M").to_string(),
        Err(_) => format_timestamp_utc(ts, utc),
    }
}

/// Removed devices: what homeCore was told to forget.
pub fn draw_removed(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Removed devices  Space brings one back ");
    let lines: Vec<Line> = match &app.devices_tab.controls.removed {
        Load::Ready(list) if list.is_empty() => vec![Line::from(Span::styled(
            "  Nothing has been removed.",
            Style::default().fg(Color::DarkGray),
        ))],
        Load::Ready(list) => {
            let sel = app.selected_removed().map(|r| r.device_id.clone());
            list.iter()
                .map(|r| {
                    let is_sel = sel.as_deref() == Some(r.device_id.as_str());
                    let name = if r.name.is_empty() {
                        &r.device_id
                    } else {
                        &r.name
                    };
                    let text = format!(
                        "{}{:<30} {:<14} removed {}",
                        if is_sel { "▸ " } else { "  " },
                        take_chars(name, 30),
                        take_chars(&clean_plugin_id(&r.plugin_id), 14),
                        removed_when(&r.removed_at, app.time_utc)
                    );
                    if is_sel {
                        Line::from(Span::styled(text, highlight()))
                    } else {
                        Line::from(text)
                    }
                })
                .collect()
        }
        Load::Failed(e) => vec![Line::from(Span::styled(
            format!("  Could not load: {e}"),
            Style::default().fg(Color::Red),
        ))],
        _ => vec![Line::from("  Loading...")],
    };
    let h = area.height.saturating_sub(2) as usize;
    let sel = app.devices_tab.controls.removed_sel.index();
    let skip = sel.saturating_sub(h.saturating_sub(1));
    let lines: Vec<Line> = lines.into_iter().skip(skip).collect();
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// The device picker over everything else.
pub fn draw_picker(frame: &mut Frame<'_>, app: &App) {
    let Some(p) = &app.devices_tab.controls.picker else {
        return;
    };
    let area = frame.area();
    let w = 70.min(area.width);
    let h = 22.min(area.height);
    let r = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };
    frame.render_widget(Clear, r);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", p.title));
    let inner = block.inner(r);
    frame.render_widget(block, r);
    if inner.height < 3 {
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(format!("/{}_", p.query)).style(Style::default().fg(Color::Yellow)),
        rows[0],
    );
    let list = app.picker_devices();
    let ids: Vec<&str> = list.iter().map(|d| d.device_id.as_str()).collect();
    let sel = p
        .sel
        .id()
        .and_then(|id| ids.iter().position(|x| *x == id))
        .unwrap_or_else(|| p.sel.index().min(ids.len().saturating_sub(1)));
    let height = rows[1].height as usize;
    let start = sel.saturating_sub(height.saturating_sub(1));
    let name_w = (inner.width as usize).saturating_sub(4 + 14 + 16);
    let lines: Vec<Line> = list
        .iter()
        .enumerate()
        .skip(start)
        .take(height)
        .map(|(i, d)| {
            let marked = p.marked.contains(&d.device_id);
            let style = if i == sel {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(
                    if marked { "[x] " } else { "[ ] " },
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!("{:<name_w$}", take_chars(d.display_name(), name_w)),
                    style,
                ),
                Span::styled(
                    format!(" {:<13}", take_chars(d.device_type(), 13)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!(
                        " {}",
                        take_chars(&app.room_label(d).unwrap_or_default(), 15)
                    ),
                    Style::default().fg(Color::Gray),
                ),
            ])
        })
        .collect();
    let body = if lines.is_empty() {
        Paragraph::new("  Nothing matches").style(Style::default().fg(Color::DarkGray))
    } else {
        Paragraph::new(lines)
    };
    frame.render_widget(body, rows[1]);
    frame.render_widget(
        Paragraph::new(format!("{} marked", p.marked.len()))
            .style(Style::default().fg(Color::DarkGray)),
        rows[2],
    );
}

#[cfg(test)]
mod tests {
    use crate::app::test_support::*;
    use crate::app::{DeviceSubPanel, Load};
    use crate::model::Role;
    use crate::screens::ScreenId;
    use ratatui::{Terminal, backend::TestBackend};
    use serde_json::json;

    fn render(app: &mut crate::app::App, w: u16, h: u16) -> String {
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
    fn controls_picker_and_scene_editor_draw_at_any_size() {
        let mut a = signed_in(Role::Admin);
        let mut t = device("t", "Hall Thermostat");
        t.device_type = Some("thermostat".into());
        t.attributes = json!({"setpoint": 20.0, "hvac_mode": "heat", "temperature": 19.0})
            .as_object()
            .cloned()
            .unwrap();
        a.devices = vec![t];
        let schema = serde_json::from_value(json!({
            "attributes": {
                "setpoint": {"kind": "float", "min": 5.0, "max": 30.0, "unit": "°C",
                             "display_name": "Target"},
                "hvac_mode": {"kind": "enum", "options": ["off", "heat"]}
            },
            "actions": [{"id": "resume", "label": "Back to the usual target"},
                        {"id": "boost", "label": "Boost for an hour"}],
            "primary": ["setpoint"]
        }))
        .unwrap();
        a.devices_tab
            .controls
            .schemas
            .insert("t".into(), Load::Ready(Some(schema)));
        a.temp_unit = crate::model::TempUnit::Fahrenheit;
        a.screen = ScreenId::Devices;
        a.devices_tab.sub = DeviceSubPanel::Climate;
        a.devices_tab.controls.focus = Some(0);
        let screen = render(&mut a, 160, 40);
        assert!(screen.contains("‹ 68°F ›"), "{screen}");
        assert!(screen.contains("Offline (0)"), "{screen}");
        a.devices_tab.controls.focus = Some(2);
        for (w, h) in [(20u16, 5u16), (60, 12), (200, 60)] {
            let _ = render(&mut a, w, h);
        }
        a.screen = ScreenId::Scenes;
        a.new_scene();
        a.add_scene_members(vec!["t".into()]);
        let screen = render(&mut a, 120, 40);
        assert!(screen.contains("Hall Thermostat"), "{screen}");
        a.devices_tab.controls.picker = Some(crate::app::picker::DevicePicker::new(
            crate::app::picker::PickFor::Scene,
            "Add to the scene",
            vec![],
        ));
        for (w, h) in [(20u16, 5u16), (100, 30)] {
            let _ = render(&mut a, w, h);
        }
        a.devices_tab.controls.picker = None;
        a.scenes_tab.editor = None;
        a.screen = ScreenId::Devices;
        a.devices_tab.sub = DeviceSubPanel::Removed;
        a.devices_tab.controls.removed = Load::Ready(
            serde_json::from_value(json!([
                {"device_id": "x", "plugin_id": "plugin.lutron", "name": "Office Fan",
                 "removed_at": "2026-11-11T23:25:31Z"}
            ]))
            .unwrap(),
        );
        let screen = render(&mut a, 120, 20);
        assert!(screen.contains("Office Fan"), "{screen}");
    }
}
