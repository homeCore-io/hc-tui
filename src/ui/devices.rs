//! Devices (all and media players), Scenes and Areas tabs.

use super::widgets::{
    clean_name, clean_plugin_id, detail_row, format_duration_ms, highlight, make_bar, status_color,
};
use crate::app::devices::{
    device_battery, device_brightness, device_humidity, device_lock_state, device_temperature,
    is_timer, timer_remaining_secs, timer_state,
};
use crate::app::media::media_player_model;
use crate::app::{App, AreasPane, DeviceSubPanel, DeviceViewMode, format_timestamp_utc};
use crate::model::{DeviceState, SceneSource};
use crate::text::{normalize_label, take_chars};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState, Tabs,
        Wrap,
    },
};
use serde_json::Value;

pub fn draw_devices_tab(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    let idx = match app.devices_tab.sub {
        DeviceSubPanel::All => 0,
        DeviceSubPanel::MediaPlayers => 1,
    };
    let tabs = Tabs::new(vec![Line::from("All"), Line::from("Media Players")])
        .select(idx)
        .block(Block::default().borders(Borders::ALL).title("Devices"))
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, layout[0]);

    match app.devices_tab.sub {
        DeviceSubPanel::All => {
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(layout[1]);
            draw_device_list(frame, app, panes[0]);
            draw_device_detail(frame, app, panes[1]);
        }
        DeviceSubPanel::MediaPlayers => draw_media_players(frame, app, layout[1]),
    }
}

fn draw_device_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let tab = &app.devices_tab;
    let selected_id = app.selected_device().map(|d| d.device_id.clone());
    let mut items: Vec<ListItem<'static>> = Vec::new();
    let mut render_selected = None;

    let mut row = |d: &DeviceState, indent: bool, items: &mut Vec<ListItem<'static>>| {
        let is_sel = selected_id.as_deref() == Some(d.device_id.as_str());
        if is_sel {
            render_selected = Some(items.len());
        }
        items.push(device_list_row(app, d, is_sel, indent));
    };

    if tab.view_mode == DeviceViewMode::Grouped {
        let mut devices = app.visible_devices();
        for (label, count) in &app.views.device_groups {
            items.push(ListItem::new(Line::from(Span::styled(
                format!(" {label} "),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ))));
            for d in devices.by_ref().take(*count) {
                row(d, true, &mut items);
            }
        }
    } else {
        for d in app.visible_devices() {
            row(d, false, &mut items);
        }
    }

    let mode = if tab.view_mode == DeviceViewMode::Grouped {
        "Grouped"
    } else {
        "Flat"
    };
    let search = match tab.search_query.trim() {
        "" => "-".to_string(),
        q if tab.search_open => format!("{q}_"),
        q => q.to_string(),
    };
    let syncing = if app.syncing { " [syncing...]" } else { "" };
    let title = format!(
        "Devices ({mode}) [{}] f:{} s:{} q:{}{syncing}",
        app.views.devices.len(),
        tab.filter.title(),
        tab.sort.title(),
        search,
    );
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    let mut state = ListState::default();
    state.select(render_selected);
    frame.render_stateful_widget(list, area, &mut state);
}

fn device_list_row(
    app: &App,
    d: &DeviceState,
    is_selected: bool,
    indent: bool,
) -> ListItem<'static> {
    let status = app.device_status(d);
    let sc = status_color(&status, d.available);
    let prefix = if indent { "  " } else { "" };
    let name = take_chars(&clean_name(d.display_name()), 26);

    let mut suffix = String::new();
    let shows_temp = status.contains('°');
    let shows_humidity = status.ends_with("%rh");
    let shows_battery = status.ends_with('%') && !shows_humidity;
    if let Some(b) = device_battery(d)
        && !shows_battery
    {
        suffix.push_str(&format!(" {b}%🔋"));
    }
    if let Some(t) = device_temperature(d)
        && !shows_temp
    {
        suffix.push_str(&format!(" {}", app.temp_unit.format(t)));
    }
    if let Some(h) = device_humidity(d)
        && !shows_humidity
    {
        suffix.push_str(&format!(" {h:.0}%"));
    }
    if is_timer(d) && matches!(timer_state(d), "running" | "paused") {
        let icon = if timer_state(d) == "running" {
            "▶"
        } else {
            "⏸"
        };
        let remaining = timer_remaining_secs(d) * 1000;
        suffix.push_str(&format!(" {icon} {}", format_duration_ms(remaining)));
    }

    if is_selected {
        let s = highlight();
        return ListItem::new(Line::from(vec![
            Span::styled(prefix.to_string(), s),
            Span::styled("● ", s),
            Span::styled(format!("{name:<26}"), s),
            Span::styled(format!(" {status:<10}"), s),
            Span::styled(suffix, s),
        ]));
    }
    let (base, status_style) = if d.available {
        (Style::default(), Style::default().fg(sc))
    } else {
        let dim = Style::default().fg(Color::DarkGray);
        (dim, dim)
    };
    let dot = if d.available {
        Span::styled("● ", Style::default().fg(Color::Green))
    } else {
        Span::styled("○ ", Style::default().fg(Color::DarkGray))
    };
    ListItem::new(Line::from(vec![
        Span::styled(prefix.to_string(), base),
        dot,
        Span::styled(format!("{name:<26}"), base),
        Span::styled(format!(" {status:<10}"), status_style),
        Span::styled(suffix, Style::default().fg(Color::DarkGray)),
    ]))
}

/// Attributes the detail pane renders itself; the rest go under "Other".
const SHOWN_ATTRS: &[&str] = &[
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
    "humidity",
    "brightness_pct",
    "motion",
    "contact_open",
    "position",
    "mode",
    "hvac_mode",
    "hvac_action",
    "setpoint",
    "heat_setpoint",
    "cool_setpoint",
    "power_w",
    "energy_kwh",
    "voltage",
    "current_a",
    "illuminance",
    "co2_ppm",
    "smoke",
    "co",
    "water_detected",
    "tamper",
    "vibration",
    "bolt_status",
    "latch_status",
    "door_status",
    "door_open",
    "lock_operation_type",
    "lock_timeout_secs",
    "lock_auto_relock_secs",
    "last_alert",
    "auto_lock_secs",
    "duration_secs",
    "remaining_secs",
    "repeat",
    "started_at",
    "label",
];

fn draw_device_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Detail");
    let Some(d) = app.selected_device() else {
        frame.render_widget(
            Paragraph::new("No device selected")
                .block(block)
                .alignment(Alignment::Center),
            area,
        );
        return;
    };
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let a = &d.attributes;
    let f64_attr = |k: &str| a.get(k).and_then(Value::as_f64);
    let str_attr = |k: &str| a.get(k).and_then(Value::as_str);
    let bool_attr = |k: &str| a.get(k).and_then(Value::as_bool);

    let mut lines: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            clean_name(d.display_name()),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        detail_row("Device ID", vec![Span::raw(d.device_id.clone())]),
    ];
    if d.name_override.is_some() && d.display_name() != d.name {
        lines.push(detail_row(
            "Plugin name",
            vec![Span::styled(
                d.name.clone(),
                Style::default().fg(Color::DarkGray),
            )],
        ));
    }
    if let Some(cn) = d.canonical_name.as_ref().filter(|c| !c.is_empty()) {
        lines.push(detail_row(
            "Canonical",
            vec![Span::styled(cn.clone(), Style::default().fg(Color::Cyan))],
        ));
    }
    let status = app.device_status(d);
    let sc = status_color(&status, d.available);
    lines.push(detail_row(
        "Status",
        vec![Span::styled(
            status,
            Style::default().fg(sc).add_modifier(Modifier::BOLD),
        )],
    ));
    let (avail, avail_color) = if d.available {
        ("Online", Color::Green)
    } else {
        ("Offline", Color::Red)
    };
    lines.push(detail_row(
        "Available",
        vec![Span::styled(avail, Style::default().fg(avail_color))],
    ));
    if let Some(room) = app.room_label(d) {
        lines.push(detail_row("Area", vec![Span::raw(room)]));
    }
    lines.push(detail_row(
        "Plugin",
        vec![Span::raw(clean_plugin_id(&d.plugin_id))],
    ));
    if let Some(t) = d.device_type.as_ref() {
        lines.push(detail_row("Type", vec![Span::raw(normalize_label(t))]));
    }
    if let Some(parent) = d.parent_device_id.as_ref() {
        let label = app
            .device_by_id(parent)
            .map(|p| p.display_name().to_string())
            .unwrap_or_else(|| parent.clone());
        lines.push(detail_row("Parent", vec![Span::raw(label)]));
    }
    if !d.last_seen.is_empty() {
        lines.push(detail_row(
            "Last seen",
            vec![Span::styled(
                format_timestamp_utc(&d.last_seen, app.time_utc),
                Style::default().fg(Color::DarkGray),
            )],
        ));
    }
    lines.push(Line::from(""));

    if is_timer(d) {
        let state = timer_state(d).to_string();
        let duration_ms = a.get("duration_secs").and_then(Value::as_u64).unwrap_or(0) * 1000;
        let remaining_ms = timer_remaining_secs(d) * 1000;
        let color = match state.as_str() {
            "running" => Color::Green,
            "paused" => Color::Yellow,
            "finished" => Color::Cyan,
            _ => Color::DarkGray,
        };
        lines.push(detail_row(
            "Timer State",
            vec![Span::styled(
                normalize_label(&state),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )],
        ));
        if duration_ms > 0 {
            lines.push(detail_row(
                "Duration",
                vec![Span::raw(format_duration_ms(duration_ms))],
            ));
        }
        if matches!(state.as_str(), "running" | "paused") && duration_ms > 0 {
            let progress = 1.0 - (remaining_ms as f64 / duration_ms as f64).clamp(0.0, 1.0);
            lines.push(detail_row(
                "Remaining",
                vec![
                    Span::styled(
                        format!("{} ", format_duration_ms(remaining_ms)),
                        Style::default().fg(color),
                    ),
                    Span::styled(make_bar(progress, 10), Style::default().fg(color)),
                ],
            ));
        }
        if bool_attr("repeat") == Some(true) {
            lines.push(detail_row(
                "Repeat",
                vec![Span::styled("Yes", Style::default().fg(Color::Yellow))],
            ));
        }
    }

    if let Some(b) = device_battery(d) {
        let color = if b <= 20 {
            Color::Red
        } else if b <= 40 {
            Color::Yellow
        } else {
            Color::Green
        };
        lines.push(detail_row(
            "Battery",
            vec![
                Span::styled(format!("{b:3}% "), Style::default().fg(color)),
                Span::styled(make_bar(b as f64 / 100.0, 10), Style::default().fg(color)),
                Span::styled(
                    if b <= 20 { " LOW" } else { "" },
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ],
        ));
    }
    if let Some(t) = device_temperature(d) {
        lines.push(detail_row(
            "Temperature",
            vec![Span::styled(
                app.temp_unit.format(t),
                Style::default().fg(Color::Yellow),
            )],
        ));
    }
    if let Some(h) = device_humidity(d) {
        lines.push(detail_row(
            "Humidity",
            vec![Span::styled(
                format!("{h:.0}%"),
                Style::default().fg(Color::Cyan),
            )],
        ));
    }
    if let Some(b) = device_brightness(d) {
        lines.push(detail_row(
            "Brightness",
            vec![
                Span::styled(format!("{b:3}% "), Style::default().fg(Color::Yellow)),
                Span::styled(
                    make_bar(b as f64 / 100.0, 10),
                    Style::default().fg(Color::Yellow),
                ),
            ],
        ));
    }
    if let Some(locked) = device_lock_state(d) {
        let (s, c) = if locked {
            ("Locked", Color::Red)
        } else {
            ("Unlocked", Color::Green)
        };
        lines.push(detail_row(
            "Lock",
            vec![Span::styled(s, Style::default().fg(c))],
        ));
        let open_closed = |label: &'static str, closed: bool| {
            let (s, c) = if closed {
                ("Closed", Color::Green)
            } else {
                ("Open", Color::Yellow)
            };
            detail_row(label, vec![Span::styled(s, Style::default().fg(c))])
        };
        if let Some(bolt) = str_attr("bolt_status") {
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
        if let Some(latch) = str_attr("latch_status") {
            lines.push(open_closed("Latch", latch == "closed"));
        }
        if let Some(door) = str_attr("door_status") {
            lines.push(open_closed("Door", door == "closed"));
        }
        if let Some(open) = bool_attr("door_open") {
            lines.push(open_closed("Door", !open));
        }
        if let Some(alert) = str_attr("last_alert") {
            lines.push(detail_row(
                "Last Alert",
                vec![Span::styled(
                    alert.to_string(),
                    Style::default().fg(Color::Yellow),
                )],
            ));
        }
        if let Some(secs) = a
            .get("auto_lock_secs")
            .and_then(Value::as_u64)
            .filter(|s| *s > 0)
        {
            lines.push(detail_row("Auto-lock", vec![Span::raw(format!("{secs}s"))]));
        }
        if let Some(t) = f64_attr("lock_timeout_secs").filter(|t| *t > 0.0) {
            lines.push(detail_row("Timeout", vec![Span::raw(format!("{t:.0}s"))]));
        }
        if let Some(t) = f64_attr("lock_auto_relock_secs").filter(|t| *t > 0.0) {
            lines.push(detail_row(
                "Auto-relock",
                vec![Span::raw(format!("{t:.0}s"))],
            ));
        }
    }
    if let Some(motion) = bool_attr("motion") {
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
    if let Some(open) = bool_attr("contact_open").or_else(|| bool_attr("open")) {
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
    if let Some(pos) = f64_attr("position") {
        lines.push(detail_row(
            "Position",
            vec![
                Span::styled(format!("{pos:3.0}% "), Style::default().fg(Color::Cyan)),
                Span::styled(make_bar(pos / 100.0, 10), Style::default().fg(Color::Cyan)),
            ],
        ));
    }
    if let Some(mode) = str_attr("hvac_mode").or_else(|| str_attr("mode")) {
        let c = match mode {
            "heat" => Color::Red,
            "cool" => Color::Cyan,
            "off" => Color::DarkGray,
            _ => Color::White,
        };
        lines.push(detail_row(
            "Mode",
            vec![Span::styled(normalize_label(mode), Style::default().fg(c))],
        ));
    }
    if let Some(action) = str_attr("hvac_action") {
        lines.push(detail_row("HVAC", vec![Span::raw(normalize_label(action))]));
    }
    for (key, label) in [
        ("setpoint", "Setpoint"),
        ("heat_setpoint", "Heat to"),
        ("cool_setpoint", "Cool to"),
    ] {
        if let Some(sp) = f64_attr(key) {
            lines.push(detail_row(
                label,
                vec![Span::styled(
                    app.temp_unit.format(sp),
                    Style::default().fg(Color::Yellow),
                )],
            ));
        }
    }

    if ["power_w", "energy_kwh", "voltage", "current_a"]
        .iter()
        .any(|k| a.contains_key(*k))
    {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─── Energy ───",
            Style::default().fg(Color::DarkGray),
        )));
    }
    if let Some(w) = f64_attr("power_w") {
        lines.push(detail_row(
            "Power",
            vec![Span::styled(
                format!("{w:.1} W"),
                Style::default().fg(Color::Yellow),
            )],
        ));
    }
    if let Some(kwh) = f64_attr("energy_kwh") {
        lines.push(detail_row(
            "Energy",
            vec![Span::raw(format!("{kwh:.3} kWh"))],
        ));
    }
    if let Some(v) = f64_attr("voltage") {
        lines.push(detail_row("Voltage", vec![Span::raw(format!("{v:.1} V"))]));
    }
    if let Some(amp) = f64_attr("current_a") {
        lines.push(detail_row(
            "Current",
            vec![Span::raw(format!("{amp:.2} A"))],
        ));
    }
    if let Some(lux) = f64_attr("illuminance") {
        lines.push(detail_row(
            "Illuminance",
            vec![Span::raw(format!("{lux:.0} lx"))],
        ));
    }
    if let Some(co2) = f64_attr("co2_ppm") {
        lines.push(detail_row("CO₂", vec![Span::raw(format!("{co2:.0} ppm"))]));
    }
    for (key, label) in [
        ("smoke", "Smoke"),
        ("co", "CO"),
        ("water_detected", "Water"),
        ("tamper", "Tamper"),
        ("vibration", "Vibration"),
    ] {
        if bool_attr(key) == Some(true) {
            lines.push(detail_row(
                label,
                vec![Span::styled(
                    "ALARM",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )],
            ));
        }
    }

    let other: Vec<(&String, &Value)> = a
        .iter()
        .filter(|(k, _)| !SHOWN_ATTRS.contains(&k.as_str()))
        .collect();
    if !other.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─── Other ───",
            Style::default().fg(Color::DarkGray),
        )));
        for (k, v) in other {
            let val = v
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| v.to_string());
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<13}", normalize_label(k)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(crate::text::truncate(&val, 200)),
            ]));
        }
    }

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_media_players(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);
    let selected = app.selected_device().map(|d| d.device_id.clone());
    let mut render_selected = None;
    let items: Vec<ListItem> = app
        .views
        .media
        .iter()
        .filter_map(|&i| app.devices.get(i))
        .enumerate()
        .map(|(row, d)| {
            let model = media_player_model(d);
            let state = model
                .as_ref()
                .map(|m| normalize_label(&m.playback_state))
                .unwrap_or_else(|| app.device_status(d));
            let summary = model
                .as_ref()
                .and_then(|m| m.title.as_deref())
                .map(|t| take_chars(t, 18))
                .unwrap_or_default();
            let is_sel = selected.as_deref() == Some(d.device_id.as_str());
            if is_sel {
                render_selected = Some(row);
            }
            let style = if is_sel {
                highlight()
            } else if d.available {
                Style::default()
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let name = take_chars(&clean_name(d.display_name()), 22);
            ListItem::new(Line::from(vec![
                Span::styled(format!("{name:<22}"), style),
                Span::styled(format!(" {state:<10}"), style),
                Span::styled(
                    if summary.is_empty() {
                        String::new()
                    } else {
                        format!(" {summary}")
                    },
                    style,
                ),
            ]))
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Media Players [{}]", app.views.media.len())),
    );
    let mut state = ListState::default();
    state.select(render_selected);
    frame.render_stateful_widget(list, panes[0], &mut state);
    draw_media_detail(frame, app, panes[1]);
}

fn draw_media_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Media Player Detail");
    let Some(m) = app.selected_media_player_model() else {
        frame.render_widget(
            Paragraph::new("No media player selected")
                .block(block)
                .alignment(Alignment::Center),
            area,
        );
        return;
    };
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            clean_name(&m.display_name),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        detail_row("Device ID", vec![Span::raw(m.device_id.clone())]),
    ];
    if let Some(cn) = &m.canonical_name {
        lines.push(detail_row(
            "Canonical",
            vec![Span::styled(cn.clone(), Style::default().fg(Color::Cyan))],
        ));
    }
    lines.push(detail_row(
        "Plugin",
        vec![Span::raw(clean_plugin_id(&m.plugin_id))],
    ));
    let pc = match m.playback_state.as_str() {
        "playing" => Color::Green,
        "paused" => Color::Yellow,
        "stopped" => Color::DarkGray,
        "buffering" => Color::Cyan,
        _ => Color::Gray,
    };
    lines.push(detail_row(
        "State",
        vec![Span::styled(
            normalize_label(&m.playback_state),
            Style::default().fg(pc).add_modifier(Modifier::BOLD),
        )],
    ));
    for (label, v) in [
        ("Title", &m.title),
        ("Artist", &m.artist),
        ("Album", &m.album),
        ("Source", &m.source),
    ] {
        if let Some(v) = v {
            lines.push(detail_row(label, vec![Span::raw(v.clone())]));
        }
    }
    if let Some(v) = m.volume {
        lines.push(detail_row(
            "Volume",
            vec![
                Span::styled(format!("{v:3}% "), Style::default().fg(Color::Yellow)),
                Span::styled(
                    make_bar(v as f64 / 100.0, 10),
                    Style::default().fg(Color::Yellow),
                ),
            ],
        ));
    }
    if let Some(muted) = m.muted {
        lines.push(detail_row(
            "Mute",
            vec![Span::styled(
                if muted { "Muted" } else { "Unmuted" },
                Style::default().fg(if muted { Color::Yellow } else { Color::Green }),
            )],
        ));
    }
    if let (Some(pos), Some(dur)) = (m.position_secs, m.duration_secs) {
        let progress = if dur == 0 {
            0.0
        } else {
            pos as f64 / dur as f64
        };
        lines.push(detail_row(
            "Progress",
            vec![
                Span::raw(format!(
                    "{} / {} ",
                    format_duration_ms(pos * 1000),
                    format_duration_ms(dur * 1000)
                )),
                Span::styled(make_bar(progress, 10), Style::default().fg(Color::Cyan)),
            ],
        ));
    }
    lines.push(Line::from(""));
    let c = &m.capabilities;
    let cmds: Vec<&str> = [
        (c.can_play, "play"),
        (c.can_pause, "pause"),
        (c.can_stop, "stop"),
        (c.can_previous, "previous"),
        (c.can_next, "next"),
        (c.can_set_volume, "volume"),
        (c.can_mute, "mute"),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, n)| *n)
    .collect();
    lines.push(detail_row(
        "Controls",
        vec![Span::styled(
            if cmds.is_empty() {
                "none".to_string()
            } else {
                cmds.join(", ")
            },
            Style::default().fg(Color::DarkGray),
        )],
    ));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

// ── Scenes ──────────────────────────────────────────────────────────────────

pub fn draw_scenes_table(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let hl = highlight();
    let header = Row::new(
        ["  Scene", "Source", "Area / Room", "Status"]
            .iter()
            .map(|h| Cell::from(*h).style(super::widgets::heading())),
    );
    let selected = app.selected_scene().map(|s| s.id.clone());
    let mut sel_row = None;
    let rows: Vec<Row> = app
        .scenes
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_sel = selected.as_deref() == Some(s.id.as_str());
            if is_sel {
                sel_row = Some(i);
            }
            let source = match (s.source, s.plugin_id.as_deref()) {
                (SceneSource::Native, _) => "homeCore".to_string(),
                (SceneSource::Device, Some(p)) => p.trim_start_matches("plugin.").to_string(),
                (SceneSource::Device, None) => "-".to_string(),
            };
            let room = s.room.clone().unwrap_or_else(|| "-".to_string());
            let (st, color) = match s.active {
                Some(true) => ("● Active", Color::Green),
                Some(false) => ("○ Inactive", Color::DarkGray),
                None => ("-", Color::DarkGray),
            };
            if is_sel {
                Row::new(vec![
                    Cell::from(format!("  {}", s.name)).style(hl),
                    Cell::from(source).style(hl),
                    Cell::from(room).style(hl),
                    Cell::from(st).style(hl),
                ])
            } else {
                Row::new(vec![
                    Cell::from(format!("  {}", s.name)).style(Style::default().fg(Color::White)),
                    Cell::from(source).style(Style::default().fg(Color::DarkGray)),
                    Cell::from(room).style(Style::default().fg(Color::Gray)),
                    Cell::from(st).style(Style::default().fg(color)),
                ])
            }
        })
        .collect();
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
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Scenes [{}]", app.scenes.len())),
    )
    .row_highlight_style(hl);
    let mut state = TableState::default();
    state.select(sel_row);
    frame.render_stateful_widget(table, area, &mut state);
}

// ── Areas ───────────────────────────────────────────────────────────────────

pub fn draw_areas_pane(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);
    draw_areas_list(frame, app, panes[0]);
    draw_area_devices(frame, app, panes[1]);
}

fn pane_border(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn draw_areas_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let focused = app.areas_tab.pane == AreasPane::AreasList;
    let items: Vec<ListItem> = app
        .areas
        .iter()
        .map(|a| {
            let n = a.device_ids.len();
            let label = if n == 1 { "device" } else { "devices" };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {:<24}", take_chars(&a.name, 24)),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("[{n} {label}]"),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();
    let mut list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Areas")
                .border_style(pane_border(focused)),
        )
        .highlight_symbol(">> ");
    if focused {
        list = list.highlight_style(highlight());
    }
    let mut state = ListState::default();
    if !app.areas.is_empty() {
        state.select(Some(app.areas_tab.list_sel.index()));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_area_devices(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let focused = app.areas_tab.pane == AreasPane::DeviceList;
    let Some(sel) = app.selected_area() else {
        frame.render_widget(
            Paragraph::new("← Select an area to view devices")
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Select an area")
                        .border_style(pane_border(focused)),
                )
                .style(Style::default().fg(Color::DarkGray)),
            area,
        );
        return;
    };
    let items: Vec<ListItem> = app
        .views
        .area_devices
        .iter()
        .filter_map(|&i| app.devices.get(i))
        .map(|d| {
            let mark = if app.areas_tab.marked.contains(&d.device_id) {
                "[✓]"
            } else {
                "[ ]"
            };
            let (dot, c) = if d.available {
                ("●", Color::Green)
            } else {
                ("○", Color::Red)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{mark} "), Style::default().fg(Color::Yellow)),
                Span::styled(format!("{dot} "), Style::default().fg(c)),
                Span::styled(
                    format!("{:<20}", take_chars(d.display_name(), 20)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!(" [{}]", app.device_status(d)),
                    Style::default().fg(Color::Gray),
                ),
            ]))
        })
        .collect();
    let n = items.len();
    let mut list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Devices in \"{}\"", sel.name))
                .border_style(pane_border(focused)),
        )
        .highlight_symbol(">> ");
    if focused {
        list = list.highlight_style(highlight());
    }
    let mut state = ListState::default();
    if focused && n > 0 {
        state.select(Some(app.areas_tab.devices_sel.index().min(n - 1)));
    }
    frame.render_stateful_widget(list, area, &mut state);
}
