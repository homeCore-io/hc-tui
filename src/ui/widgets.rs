//! Small drawing helpers shared by every screen.

use crate::text::normalize_label;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn highlight() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn heading() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn focus_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    }
}

/// A bordered single-line input with a title.
pub fn input<'a>(value: &'a str, title: &'a str, focused: bool) -> Paragraph<'a> {
    Paragraph::new(value).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(focus_style(focused)),
    )
}

/// Mask a secret by character count, not byte length.
pub fn mask(secret: &str, glyph: &str) -> String {
    glyph.repeat(crate::text::char_len(secret))
}

pub fn status_color(status: &str, available: bool) -> Color {
    if !available {
        return Color::DarkGray;
    }
    match status {
        "On" | "Open" | "Unlocked" | "Online" | "Occupied" => Color::Green,
        "Off" | "Closed" | "Locked" | "Offline" | "Vacant" => Color::Red,
        "Unknown" | "-" => Color::DarkGray,
        _ => Color::White,
    }
}

/// `"Label      : "` followed by `spans`.
pub fn detail_row(label: &str, spans: Vec<Span<'static>>) -> Line<'static> {
    let mut all = vec![Span::styled(
        format!("{label:<11}: "),
        Style::default().fg(Color::DarkGray),
    )];
    all.extend(spans);
    Line::from(all)
}

pub fn status_detail_row(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {label:<16}"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(value.to_string(), Style::default().fg(Color::White)),
    ])
}

pub fn make_bar(ratio: f64, width: usize) -> String {
    let filled = (ratio.clamp(0.0, 1.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

pub fn format_duration_ms(ms: u64) -> String {
    let total = ms / 1000;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}h {m:02}m {s:02}s")
    } else if m > 0 {
        format!("{m}:{s:02}")
    } else {
        format!("0:{s:02}")
    }
}

pub fn format_uptime(secs: u64) -> String {
    let (d, h, m) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

pub fn clean_plugin_id(plugin_id: &str) -> String {
    let mut value = plugin_id;
    for prefix in ["plugin.", "plugin_", "hc-"] {
        if let Some(stripped) = value.strip_prefix(prefix) {
            value = stripped;
        }
    }
    let last = value.rsplit('.').next().unwrap_or(value);
    normalize_label(last)
}

pub fn clean_name(name: &str) -> String {
    match name.trim() {
        "" => "-".to_string(),
        t => t.to_string(),
    }
}

/// `"YYYY-MM-DD HH:MM:SS"` from an RFC 3339 string, char-safe.
pub fn short_ts(ts: &str) -> String {
    crate::text::take_chars(ts, 19).replace('T', " ")
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vertical = Layout::default()
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
        .split(vertical[1])[1]
}

/// A rectangle of `height` rows and a width clamped to `[min_w, max_w]`,
/// centered in `area` and never larger than it.
pub fn centered_rect_min(min_w: u16, max_w: u16, height: u16, area: Rect) -> Rect {
    let width = max_w.min(area.width).max(min_w.min(area.width));
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_counts_characters() {
        assert_eq!(mask("pässwörd", "*"), "********");
    }

    #[test]
    fn short_ts_is_char_safe() {
        assert_eq!(short_ts("2026-11-17T21:02:47.67Z"), "2026-11-17 21:02:47");
        assert_eq!(short_ts("é"), "é");
    }

    #[test]
    fn plugin_ids_are_tidied() {
        assert_eq!(clean_plugin_id("plugin.hue"), "Hue");
        assert_eq!(clean_plugin_id("core.glue"), "Glue");
    }
}
