//! Overlays: editors, the confirmation dialog, the streaming-action
//! modal, the rule groups overlay and small input bars.

use super::widgets::{centered_rect, highlight, input, mask};
use crate::app::editors::{
    DeviceEditField, MatterCommissionField, ModeEditField, ModeKind, RuleFilterField,
    UserEditField, UserEditMode,
};
use crate::app::plugins::progress_percent;
use crate::app::{App, Load, StreamingAction, StreamingStage};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap},
};
use serde_json::Value;

pub fn draw_overlays(frame: &mut Frame<'_>, app: &App) {
    draw_device_editor(frame, app);
    draw_area_editor(frame, app);
    draw_user_editor(frame, app);
    draw_mode_editor(frame, app);
    draw_matter_editor(frame, app);
    if app.plugins_tab.streaming.is_some() {
        draw_streaming_modal(frame, app);
    }
    super::plugins::draw_plugin_overlays(frame, app);
    if app.rules_tab.groups.is_some() {
        draw_groups(frame, app);
    }
    draw_filter_bar(frame, app);
    super::settings::draw_modal(frame, app);
    super::home::draw_picker(frame, app);
    super::home::draw_heading_editor(frame, app);
    super::helpers::draw_helper_overlays(frame, app);
    super::rules::draw_rule_overlays(frame, app);
    super::scenes::draw_overlays(frame, app);
    super::help::draw_help(frame, app);
    // The question goes over everything.
    draw_confirm(frame, app);
}

fn modal(frame: &mut Frame<'_>, px: u16, py: u16, title: &str) -> Rect {
    let popup = centered_rect(px, py, frame.area());
    frame.render_widget(Clear, popup);
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title.to_string());
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);
    inner
}

fn rows(area: Rect, n: usize) -> std::rc::Rc<[Rect]> {
    let mut c = vec![Constraint::Length(3); n];
    c.push(Constraint::Min(1));
    Layout::default()
        .direction(Direction::Vertical)
        .constraints(c)
        .split(area)
}

fn help(frame: &mut Frame<'_>, text: &str, area: Rect) {
    frame.render_widget(
        Paragraph::new(text.to_string()).alignment(Alignment::Center),
        area,
    );
}

fn draw_confirm(frame: &mut Frame<'_>, app: &App) {
    let Some(c) = &app.confirm else {
        return;
    };
    let popup = centered_rect(60, 25, frame.area());
    frame.render_widget(Clear, popup);
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(c.title.clone())
        .border_style(Style::default().fg(Color::Red));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(c.message.clone())
            .style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        layout[0],
    );
    if let Some(d) = &c.detail {
        frame.render_widget(
            Paragraph::new(d.clone())
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            layout[1],
        );
    }
    frame.render_widget(
        Paragraph::new("No (n or Esc)     y  Yes")
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center),
        layout[2],
    );
}

fn draw_device_editor(frame: &mut Frame<'_>, app: &App) {
    let Some(ed) = &app.devices_tab.editor else {
        return;
    };
    let inner = modal(frame, 72, 70, "Edit Device");
    let r = rows(inner, 5);
    frame.render_widget(Paragraph::new(format!("Device: {}", ed.device_id)), r[0]);
    frame.render_widget(
        input(
            &ed.name,
            "Name (empty restores the plugin's name)",
            ed.field == DeviceEditField::Name,
        ),
        r[1],
    );
    let room = if ed.area.trim().is_empty() {
        "no room".to_string()
    } else {
        ed.area.clone()
    };
    frame.render_widget(
        input(
            &room,
            "Room (type one, or ←/→ through the rooms)",
            ed.field == DeviceEditField::Area,
        ),
        r[2],
    );
    let security = format!(
        "‹ {} ›",
        crate::app::editors::DeviceEditor::security_label(ed.security)
    );
    frame.render_widget(
        input(
            &security,
            "Security (←/→)",
            ed.field == DeviceEditField::Security,
        ),
        r[3],
    );
    frame.render_widget(
        input(
            &ed.canonical_name,
            "Name in automations",
            ed.field == DeviceEditField::CanonicalName,
        ),
        r[4],
    );
    help(frame, "Tab field | Enter save | Esc cancel", r[5]);
}

fn draw_area_editor(frame: &mut Frame<'_>, app: &App) {
    let Some(ed) = &app.rooms.editor else {
        return;
    };
    let title = if ed.id.is_none() {
        "New room"
    } else {
        "Rename room"
    };
    let inner = modal(frame, 60, 30, title);
    let r = rows(inner, 1);
    frame.render_widget(input(&ed.name, "Name", true), r[0]);
    help(frame, "Enter save | Esc cancel", r[1]);
}

fn draw_user_editor(frame: &mut Frame<'_>, app: &App) {
    let Some(ed) = &app.manage.user_editor else {
        return;
    };
    let title = match ed.mode {
        UserEditMode::Create => "New User",
        UserEditMode::EditRole => "Change Role",
        UserEditMode::ChangePassword => "Change Password",
        UserEditMode::SetPassword => "Set Their Password",
    };
    let inner = modal(frame, 64, 60, title);
    let f = |x: UserEditField| ed.field == x;
    let role = format!("{}  (Space for the next)", ed.role.label());
    match ed.mode {
        UserEditMode::Create => {
            let r = rows(inner, 4);
            frame.render_widget(
                input(&ed.username, "Username", f(UserEditField::Username)),
                r[0],
            );
            let pw = mask(&ed.password, "*");
            frame.render_widget(input(&pw, "Password", f(UserEditField::Password)), r[1]);
            let cpw = mask(&ed.confirm_password, "*");
            frame.render_widget(
                input(&cpw, "Confirm Password", f(UserEditField::ConfirmPassword)),
                r[2],
            );
            frame.render_widget(input(&role, "Role", f(UserEditField::Role)), r[3]);
            help(
                frame,
                "Tab/↑↓ field | Space cycle role | Enter save | Esc cancel",
                r[4],
            );
        }
        UserEditMode::EditRole => {
            let r = rows(inner, 2);
            frame.render_widget(Paragraph::new(format!("User: {}", ed.username)), r[0]);
            frame.render_widget(input(&role, "Role", true), r[1]);
            help(frame, "Space cycle | Enter save | Esc cancel", r[2]);
        }
        UserEditMode::SetPassword => {
            let r = rows(inner, 3);
            frame.render_widget(Paragraph::new(format!("User: {}", ed.username)), r[0]);
            let pw = mask(&ed.password, "*");
            frame.render_widget(input(&pw, "New Password", f(UserEditField::Password)), r[1]);
            let cpw = mask(&ed.confirm_password, "*");
            frame.render_widget(
                input(
                    &cpw,
                    "Confirm New Password",
                    f(UserEditField::ConfirmPassword),
                ),
                r[2],
            );
            help(
                frame,
                "Tab field | Enter save (signs them out everywhere) | Esc cancel",
                r[3],
            );
        }
        UserEditMode::ChangePassword => {
            let r = rows(inner, 4);
            frame.render_widget(Paragraph::new(format!("User: {}", ed.username)), r[0]);
            let cur = mask(&ed.current_password, "*");
            frame.render_widget(
                input(&cur, "Current Password", f(UserEditField::CurrentPassword)),
                r[1],
            );
            let pw = mask(&ed.password, "*");
            frame.render_widget(input(&pw, "New Password", f(UserEditField::Password)), r[2]);
            let cpw = mask(&ed.confirm_password, "*");
            frame.render_widget(
                input(
                    &cpw,
                    "Confirm New Password",
                    f(UserEditField::ConfirmPassword),
                ),
                r[3],
            );
            help(
                frame,
                "Tab/↑↓ field | Enter save (signs you out) | Esc cancel",
                r[4],
            );
        }
    }
}

fn draw_mode_editor(frame: &mut Frame<'_>, app: &App) {
    let Some(ed) = &app.manage.mode_editor else {
        return;
    };
    use crate::app::editors::sun_event_words;
    let title = match &ed.editing {
        Some(m) => format!("Edit mode {}", m.id),
        None => "New mode".to_string(),
    };
    let fields = ed.fields();
    let inner = modal(frame, 64, 80, &title);
    let r = rows(inner, 6);
    let offset = |o: i32| match o {
        0 => "at the time".to_string(),
        o if o > 0 => format!("{o} minutes after"),
        o => format!("{} minutes before", -o),
    };
    let kc = match ed.kind {
        ModeKind::Solar => Color::Yellow,
        ModeKind::Manual => Color::Cyan,
    };
    let mut i = 0;
    for f in [
        ModeEditField::Name,
        ModeEditField::Kind,
        ModeEditField::OnEvent,
        ModeEditField::OnOffset,
        ModeEditField::OffEvent,
        ModeEditField::OffOffset,
    ] {
        let (label, value) = match f {
            ModeEditField::Name => ("Name", ed.name.clone()),
            ModeEditField::Kind => (
                "Kind (←/→): manual you switch, solar follows the sun",
                format!("‹ {} ›", ed.kind.as_str()),
            ),
            ModeEditField::OnEvent => (
                "Turns on at",
                format!("‹ {} ›", sun_event_words(&ed.on_event)),
            ),
            ModeEditField::OnOffset => ("On offset", format!("‹ {} ›", offset(ed.on_offset))),
            ModeEditField::OffEvent => (
                "Turns off at",
                format!("‹ {} ›", sun_event_words(&ed.off_event)),
            ),
            ModeEditField::OffOffset => ("Off offset", format!("‹ {} ›", offset(ed.off_offset))),
        };
        if f == ModeEditField::Kind && ed.editing.is_some() {
            continue;
        }
        if !fields.contains(&f) {
            continue;
        }
        let mut p = input(&value, label, ed.field == f);
        if f == ModeEditField::Kind {
            p = p.style(Style::default().fg(kc));
        }
        frame.render_widget(p, r[i]);
        i += 1;
    }
    let verb = if ed.editing.is_some() {
        "save"
    } else {
        "create"
    };
    help(
        frame,
        &format!("Tab field  |  ←/→ change  |  Enter {verb}  |  Esc cancel"),
        r[i],
    );
}

fn draw_matter_editor(frame: &mut Frame<'_>, app: &App) {
    let Some(ed) = &app.manage.matter.editor else {
        return;
    };
    let inner = modal(frame, 72, 68, "Matter Commission");
    let r = rows(inner, 5);
    use MatterCommissionField as F;
    frame.render_widget(
        input(
            &ed.pairing_code,
            "Pairing Code (manual code)",
            ed.field == F::PairingCode,
        ),
        r[0],
    );
    frame.render_widget(
        input(&ed.name, "Name (optional)", ed.field == F::Name),
        r[1],
    );
    frame.render_widget(
        input(&ed.room, "Room (optional)", ed.field == F::Room),
        r[2],
    );
    frame.render_widget(
        input(
            &ed.discriminator,
            "Discriminator (optional)",
            ed.field == F::Discriminator,
        ),
        r[3],
    );
    frame.render_widget(
        input(&ed.passcode, "Passcode (optional)", ed.field == F::Passcode),
        r[4],
    );
    help(frame, "Tab field  |  Enter commission  |  Esc cancel", r[5]);
}

fn draw_groups(frame: &mut Frame<'_>, app: &App) {
    let Some(g) = &app.rules_tab.groups else {
        return;
    };
    let popup = centered_rect(60, 60, frame.area());
    frame.render_widget(Clear, popup);
    let outer = Block::default()
        .borders(Borders::ALL)
        .title("Automation groups")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);
    match &g.items {
        Load::Ready(items) if !items.is_empty() => {
            let rows: Vec<ListItem> = items
                .iter()
                .map(|grp| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("  {:<28}", crate::text::take_chars(&grp.name, 28)),
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{} automation(s)", grp.rule_ids.len()),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                })
                .collect();
            let list = List::new(rows)
                .highlight_style(highlight())
                .highlight_symbol(">> ");
            let mut state = ListState::default();
            state.select(Some(g.sel.index()));
            frame.render_stateful_widget(list, layout[0], &mut state);
        }
        other => {
            let msg = match other {
                Load::Failed(e) => format!("Failed to load groups: {e}"),
                Load::Ready(_) => "No groups defined.".to_string(),
                _ => "loading...".to_string(),
            };
            frame.render_widget(
                Paragraph::new(msg)
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::DarkGray)),
                layout[0],
            );
        }
    }
    frame.render_widget(
        Paragraph::new("+ on  |  - off  |  x delete  |  Esc close")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center),
        layout[1],
    );
}

fn draw_filter_bar(frame: &mut Frame<'_>, app: &App) {
    let Some(bar) = &app.rules_tab.filter_bar else {
        return;
    };
    let area = frame.area();
    let bar_area = Rect {
        x: area.x,
        y: area.height.saturating_sub(6).max(area.y),
        width: area.width,
        height: 3.min(area.height),
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
    let style = |f: RuleFilterField| {
        if bar.active_field == f {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Gray)
        }
    };
    frame.render_widget(
        Paragraph::new(bar.tag.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Tag filter")
                .border_style(style(RuleFilterField::Tag)),
        ),
        layout[0],
    );
    frame.render_widget(
        Paragraph::new(bar.trigger.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Trigger filter (e.g. DeviceStateChanged)")
                .border_style(style(RuleFilterField::Trigger)),
        ),
        layout[1],
    );
    let (s, c) = if bar.stale {
        ("ON ", Color::Red)
    } else {
        ("off", Color::DarkGray)
    };
    frame.render_widget(
        Paragraph::new(Span::styled(s, Style::default().fg(c)))
            .block(Block::default().borders(Borders::ALL).title("Stale only")),
        layout[2],
    );
}

// ── Streaming action modal ──────────────────────────────────────────────────

fn draw_streaming_modal(frame: &mut Frame<'_>, app: &App) {
    let Some(s) = &app.plugins_tab.streaming else {
        return;
    };
    let popup = centered_rect(80, 70, frame.area());
    frame.render_widget(Clear, popup);
    let color = match s.stage {
        StreamingStage::Starting => Color::Gray,
        StreamingStage::Running => Color::Cyan,
        StreamingStage::AwaitingUser => Color::Yellow,
        StreamingStage::Complete => Color::Green,
        StreamingStage::Error | StreamingStage::Timeout => Color::Red,
        StreamingStage::Canceled => Color::Magenta,
    };
    let spin = if s.stage.is_terminal() {
        String::new()
    } else {
        format!("{} ", app.spinner())
    };
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {spin}{}: {} ", s.label, s.stage.label()))
        .border_style(Style::default().fg(color));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let has = [
        s.last_progress.is_some(),
        !s.items.is_empty(),
        s.pending_prompt.is_some(),
        !s.warnings.is_empty(),
        s.terminal.is_some(),
    ];
    let mut constraints = Vec::new();
    if !has.iter().any(|h| *h) {
        constraints.push(Constraint::Min(1));
    }
    for (on, c) in has.iter().zip([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(7),
        Constraint::Length(4),
        Constraint::Length(5),
    ]) {
        if *on {
            constraints.push(c);
        }
    }
    constraints.push(Constraint::Length(2));
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);
    let mut i = 0;
    if !has.iter().any(|h| *h) {
        frame.render_widget(
            Paragraph::new(s.footer.as_str())
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            layout[i],
        );
        i += 1;
    }
    if has[0] {
        draw_progress(frame, layout[i], s);
        i += 1;
    }
    if has[1] {
        draw_items(frame, layout[i], s);
        i += 1;
    }
    if has[2] {
        draw_prompt(frame, layout[i], s);
        i += 1;
    }
    if has[3] {
        draw_warnings(frame, layout[i], s);
        i += 1;
    }
    if has[4] {
        draw_terminal(frame, layout[i], s);
        i += 1;
    }
    frame.render_widget(
        Paragraph::new(s.footer.as_str()).style(Style::default().fg(Color::Yellow)),
        layout[i],
    );
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn draw_progress(frame: &mut Frame<'_>, area: Rect, s: &StreamingAction) {
    let Some(p) = &s.last_progress else {
        return;
    };
    let msg = p.get("message").and_then(Value::as_str).unwrap_or("");
    let block = Block::default().borders(Borders::ALL).title("Progress");
    match progress_percent(p) {
        Some(pct) => {
            let label = if msg.is_empty() {
                format!("{pct:>3.0}%")
            } else {
                format!("{pct:>3.0}%: {msg}")
            };
            frame.render_widget(
                Gauge::default()
                    .block(block)
                    .gauge_style(Style::default().fg(Color::Cyan))
                    .ratio(pct / 100.0)
                    .label(label),
                area,
            );
        }
        None => frame.render_widget(
            Paragraph::new(msg.to_string())
                .block(block)
                .style(Style::default().fg(Color::Cyan)),
            area,
        ),
    }
}

fn draw_items(frame: &mut Frame<'_>, area: Rect, s: &StreamingAction) {
    let take = area.height.saturating_sub(2) as usize;
    let start = s.items.len().saturating_sub(take);
    let items: Vec<ListItem> = s.items[start..]
        .iter()
        .map(|v| {
            let line = match v {
                Value::Object(m) => m
                    .get("name")
                    .or_else(|| m.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| v.to_string()),
                other => value_text(other),
            };
            ListItem::new(crate::text::truncate(&line, 200))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Items ({})", s.items.len())),
        ),
        area,
    );
}

fn draw_prompt(frame: &mut Frame<'_>, area: Rect, s: &StreamingAction) {
    let Some(p) = &s.pending_prompt else {
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
    let text = p
        .get("prompt")
        .map(value_text)
        .or_else(|| p.get("schema").map(Value::to_string))
        .unwrap_or_default();
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), layout[0]);
    frame.render_widget(
        Paragraph::new("Response:").style(Style::default().fg(Color::DarkGray)),
        layout[1],
    );
    frame.render_widget(
        Paragraph::new(s.response_input.as_str()).style(Style::default().fg(Color::Yellow)),
        layout[2],
    );
}

fn draw_warnings(frame: &mut Frame<'_>, area: Rect, s: &StreamingAction) {
    let take = area.height.saturating_sub(2) as usize;
    let start = s.warnings.len().saturating_sub(take);
    let items: Vec<ListItem> = s.warnings[start..]
        .iter()
        .map(|v| {
            let msg = v
                .get("message")
                .map(value_text)
                .unwrap_or_else(|| v.to_string());
            ListItem::new(Line::from(Span::styled(
                msg,
                Style::default().fg(Color::Yellow),
            )))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Warnings ({})", s.warnings.len())),
        ),
        area,
    );
}

fn draw_terminal(frame: &mut Frame<'_>, area: Rect, s: &StreamingAction) {
    let Some(t) = &s.terminal else {
        return;
    };
    let stage = t.get("stage").and_then(Value::as_str).unwrap_or("");
    let detail = t
        .get("message")
        .or_else(|| t.get("error"))
        .or_else(|| t.get("data"))
        .or_else(|| t.get("result"))
        .filter(|v| !v.is_null())
        .map(value_text)
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
    frame.render_widget(
        Paragraph::new(body)
            .block(Block::default().borders(Borders::ALL).title(" Result "))
            .style(Style::default().fg(color))
            .wrap(Wrap { trim: false }),
        area,
    );
}
