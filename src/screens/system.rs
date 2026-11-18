//! System: core's version, uptime, counts and database sizes.

use super::{Binding, Screen, ScreenId};
use crate::app::App;
use ratatui::{Frame, layout::Rect};

pub struct SystemScreen;

impl Screen for SystemScreen {
    fn id(&self) -> ScreenId {
        ScreenId::System
    }

    fn bindings(&self, _app: &App) -> Vec<Binding> {
        Vec::new()
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::manage::draw_status(frame, app, area);
    }

    fn on_enter(&self, app: &mut App) {
        app.refresh_system_status();
    }

    fn refresh(&self, app: &mut App) {
        app.refresh_system_status();
    }
}
