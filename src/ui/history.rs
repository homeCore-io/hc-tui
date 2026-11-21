//! The device history explorer (`h` on Devices).

use super::widgets::{centered_rect, short_ts};
use crate::app::history_view::{RANGES, changes, summary};
use crate::app::{App, Load};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub fn draw_history_view(frame: &mut Frame<'_>, app: &App) {
    let Some(h) = &app.devices_tab.history_view else {
        return;
    };
    let popup = centered_rect(80, 70, frame.area());
    frame.render_widget(Clear, popup);
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(format!(" History · {} ", h.name))
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(26), Constraint::Min(20)])
        .split(inner);

    // What to look at.
    let attrs: Vec<Line> = h
        .attrs
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let style = if i == h.attr_sel {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            Line::from(Span::styled(
                format!(" {:<24}", crate::text::take_chars(a, 24)),
                style,
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(attrs), cols[0]);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(cols[1]);

    // Over how long.
    let mut ranges = vec![Span::raw(" ")];
    for (i, (label, _)) in RANGES.iter().enumerate() {
        let style = if i == h.range {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        ranges.push(Span::styled(*label, style));
        ranges.push(Span::raw("   "));
    }
    frame.render_widget(Paragraph::new(Line::from(ranges)), rows[0]);

    let body: Vec<Line> = match &h.points {
        Load::Ready(points) if points.is_empty() => vec![Line::from(Span::styled(
            " Nothing recorded in this time.",
            Style::default().fg(Color::DarkGray),
        ))],
        Load::Ready(points) => match summary(points) {
            // Numbers: a line of bars, oldest on the left, and what they span.
            Some((lo, hi, latest, n)) => {
                let width = rows[1].width.saturating_sub(2) as usize;
                let mut values: Vec<f64> = points.iter().filter_map(|p| p.value.as_f64()).collect();
                values.reverse();
                vec![
                    Line::from(Span::styled(
                        format!(" {}", crate::app::controls::sparkline(&values, width)),
                        Style::default().fg(Color::Cyan),
                    )),
                    Line::from(""),
                    Line::from(format!(
                        " now {}   lowest {}   highest {}   ({n} readings)",
                        num(latest),
                        num(lo),
                        num(hi)
                    )),
                    Line::from(Span::styled(
                        format!(
                            " from {} to {}",
                            short_ts(&points[points.len() - 1].recorded_at),
                            short_ts(&points[0].recorded_at)
                        ),
                        Style::default().fg(Color::DarkGray),
                    )),
                ]
            }
            // On/off and words: when it changed.
            None => changes(points)
                .into_iter()
                .take(rows[1].height as usize)
                .map(|(at, v)| {
                    Line::from(vec![
                        Span::styled(
                            format!(" {:<21}", short_ts(&at)),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(v),
                    ])
                })
                .collect(),
        },
        Load::Failed(e) => vec![Line::from(format!(" Couldn't read the history: {e}"))],
        _ => vec![Line::from(Span::styled(
            " loading...",
            Style::default().fg(Color::DarkGray),
        ))],
    };
    frame.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), rows[1]);
    frame.render_widget(
        Paragraph::new(" j/k reading   ←/→ range   r refresh   Esc close")
            .style(Style::default().fg(Color::DarkGray)),
        rows[2],
    );
}

/// A reading in as few digits as say it: 21.5, 40, 0.25.
fn num(v: f64) -> String {
    if v.fract().abs() < 1e-9 {
        format!("{v:.0}")
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::num;

    #[test]
    fn a_reading_has_no_trailing_zeros() {
        assert_eq!(num(21.5), "21.5");
        assert_eq!(num(40.0), "40");
        assert_eq!(num(0.25), "0.25");
        assert_eq!(num(-3.10), "-3.1");
    }
}
