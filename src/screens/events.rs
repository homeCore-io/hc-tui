//! Events: what happened on homeCore's bus, newest first.

use super::{Binding, Screen, ScreenId, moves};
use crate::app::App;
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct EventsScreen;

impl Screen for EventsScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Events
    }

    fn bindings(&self, _app: &App) -> Vec<Binding> {
        let mut v = moves(|a| a.move_event(-1), |a| a.move_event(1)).to_vec();
        v.push(Binding::new(
            &[KeyCode::Char('f')],
            "f",
            "event type",
            App::cycle_event_filter,
        ));
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::manage::draw_events(frame, app, area);
    }

    fn searchable(&self, _app: &App) -> bool {
        true
    }
}
