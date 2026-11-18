//! Alerts: what homeCore says needs looking at. A plain list for now.

use super::{Binding, Screen, ScreenId, moves};
use crate::app::App;
use ratatui::{Frame, layout::Rect};

pub struct AlertsScreen;

impl Screen for AlertsScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Alerts
    }

    fn bindings(&self, _app: &App) -> Vec<Binding> {
        moves(|a| a.move_alert(-1), |a| a.move_alert(1)).to_vec()
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::lists::draw_alerts(frame, app, area);
    }

    fn searchable(&self, _app: &App) -> bool {
        true
    }

    fn badge(&self, app: &App) -> Option<usize> {
        Some(app.open_alerts())
    }
}
