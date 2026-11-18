//! The `?` overlay: every key of the current screen, then the keys that
//! work everywhere. Built from the same tables as the footer.

use crate::app::App;
use crate::screens::{self, Binding};
use crate::text::truncate;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

/// The overlay's lines: a title per section, then `key  what`.
pub fn help_lines(app: &App) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let section = |title: &str, list: Vec<Binding>, out: &mut Vec<(String, String)>| {
        if list.is_empty() {
            return;
        }
        out.push((title.to_string(), String::new()));
        for b in list {
            out.push((b.label.to_string(), b.help.to_string()));
        }
        out.push((String::new(), String::new()));
    };
    let screen = screens::screen(app.screen);
    section(screen.id().title(), screen.bindings(app), &mut out);
    section("Everywhere", screens::global_bindings(app), &mut out);
    out
}

pub fn draw_help(frame: &mut Frame<'_>, app: &App) {
    if !app.help_open {
        return;
    }
    let lines = help_lines(app);
    let area = frame.area();
    let w = 56.min(area.width);
    let h = (lines.len() as u16 + 2).min(area.height);
    let r = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };
    frame.render_widget(Clear, r);
    let text: Vec<Line> = lines
        .into_iter()
        .map(|(k, what)| {
            if what.is_empty() {
                Line::from(Span::styled(
                    format!(" {k}"),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(vec![
                    Span::styled(format!("   {:<12}", k), Style::default().fg(Color::Yellow)),
                    Span::raw(truncate(&what, w.saturating_sub(18) as usize)),
                ])
            }
        })
        .collect();
    frame.render_widget(
        Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Keys  Esc closes "),
        ),
        r,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::model::Role;
    use crate::screens::ScreenId;

    /// Everything in the footer is in `?` too.
    #[test]
    fn help_lists_every_footer_key() {
        let mut a = signed_in(Role::Admin);
        for id in ScreenId::ALL {
            a.screen = id;
            let help: Vec<String> = help_lines(&a)
                .into_iter()
                .filter(|(_, w)| !w.is_empty())
                .map(|(k, w)| format!("{k} {w}"))
                .collect();
            for f in screens::footer_hints(&a) {
                assert!(help.contains(&f), "{id:?}: {f} missing from ?");
            }
        }
    }
}
