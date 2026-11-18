//! Login screen: a centered card with the wordmark, server address,
//! username and password fields, a phase indicator and key hints.

use super::widgets::{centered_rect_min, mask};
use crate::app::{App, FocusField, LoginPhase};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

const LOGIN_ACCENT: Color = Color::Cyan;

pub fn draw_login(frame: &mut Frame<'_>, app: &App) {
    let popup = centered_rect_min(60, 22, 18, frame.area());
    frame.render_widget(Clear, popup);
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(LOGIN_ACCENT));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // wordmark
            Constraint::Length(1), // kicker
            Constraint::Length(1),
            Constraint::Length(1), // server
            Constraint::Length(1),
            Constraint::Length(3), // username
            Constraint::Length(3), // password
            Constraint::Length(1),
            Constraint::Length(1), // status
            Constraint::Min(0),
            Constraint::Length(1), // footer
        ])
        .horizontal_margin(2)
        .vertical_margin(1)
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "homeCore",
            Style::default()
                .fg(LOGIN_ACCENT)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center),
        layout[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "T E R M I N A L",
            Style::default().fg(Color::DarkGray),
        ))
        .alignment(Alignment::Center),
        layout[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("server  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                app.client.base_url().to_string(),
                Style::default().fg(Color::Gray),
            ),
        ]))
        .alignment(Alignment::Center),
        layout[3],
    );

    let focus = app.login.focus();
    let user_focused = focus == FocusField::Username;
    frame.render_widget(
        Paragraph::new(field_text(
            &app.login.username,
            user_focused,
            "type your username",
        ))
        .block(field_block("username", user_focused)),
        layout[5],
    );

    let pw_focused = focus == FocusField::Password;
    let pw = if app.login.password.is_empty() && !pw_focused {
        Line::from(Span::styled(
            "•••••••••",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        let mut spans = vec![Span::raw(mask(&app.login.password, "•"))];
        if pw_focused {
            spans.push(Span::styled("▎", Style::default().fg(LOGIN_ACCENT)));
        }
        Line::from(spans)
    };
    frame.render_widget(
        Paragraph::new(pw).block(field_block("password", pw_focused)),
        layout[6],
    );

    let status = if let Some(err) = &app.error {
        Line::from(vec![
            Span::styled(
                " ✗ ",
                Style::default()
                    .bg(Color::Red)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(err.clone(), Style::default().fg(Color::Red)),
        ])
    } else if app.login.in_progress {
        let label = match app.login.phase {
            Some(LoginPhase::Restoring) => "restoring session",
            _ => "authenticating",
        };
        Line::from(vec![
            Span::styled(
                format!(" {} ", app.spinner()),
                Style::default().fg(LOGIN_ACCENT),
            ),
            Span::styled(label, Style::default().fg(Color::Gray)),
        ])
    } else {
        Line::from(Span::styled(
            app.status.clone(),
            Style::default().fg(Color::DarkGray),
        ))
    };
    frame.render_widget(
        Paragraph::new(status).alignment(Alignment::Center),
        layout[8],
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Tab", Style::default().fg(LOGIN_ACCENT)),
            Span::styled(" switch field  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter", Style::default().fg(LOGIN_ACCENT)),
            Span::styled(" sign in  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc", Style::default().fg(LOGIN_ACCENT)),
            Span::styled(" quit", Style::default().fg(Color::DarkGray)),
        ]))
        .alignment(Alignment::Center),
        layout[10],
    );
}

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
    let mut spans = vec![Span::raw(value)];
    if focused {
        spans.push(Span::styled("▎", Style::default().fg(LOGIN_ACCENT)));
    }
    Line::from(spans)
}
