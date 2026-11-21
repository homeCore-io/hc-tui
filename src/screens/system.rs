//! System: health, versions, uptime and counts, temperatures, core's log
//! level (Left and Right), who is connected, and restarting core.

use super::{Binding, Screen, ScreenId};
use crate::app::App;
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct SystemScreen;

impl Screen for SystemScreen {
    fn id(&self) -> ScreenId {
        ScreenId::System
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        if app.manage.status.vocab.is_some() {
            return vec![
                Binding::new(
                    &[KeyCode::Char('r')],
                    "r",
                    "check again",
                    App::open_vocabulary_check,
                ),
                Binding::new(&[KeyCode::Esc], "Esc", "close", App::close_vocabulary_check),
            ];
        }
        let mut v = Vec::new();
        if app.can("devices:read") && app.can("automations:read") {
            v.push(Binding::new(
                &[KeyCode::Char('c')],
                "c",
                "check what devices take",
                App::open_vocabulary_check,
            ));
        }
        if app.can_set_log_level() {
            v.push(Binding::new(
                &[KeyCode::Right],
                "←/→",
                "core log level",
                |a| a.step_core_log_level(1),
            ));
            v.push(
                Binding::new(&[KeyCode::Left], "←", "quieter core log", |a| {
                    a.step_core_log_level(-1)
                })
                .more(),
            );
        }
        if app.is_admin() {
            v.push(Binding::new(
                &[KeyCode::Char('R')],
                "R",
                "restart core",
                |a| a.ask_restart_core(None),
            ));
        }
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::settings::draw_system(frame, app, area);
    }

    fn on_enter(&self, app: &mut App) {
        app.load_system_screen();
    }

    fn refresh(&self, app: &mut App) {
        app.status = "Refreshing...".to_string();
        app.load_system_screen();
    }
}
