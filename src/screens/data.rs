//! Data & backups: backups kept on the house, making one now, restoring,
//! automation and scene export and import, and calendars.

use super::{Binding, DELETE, ENTER, NEW, Screen, ScreenId, moves};
use crate::app::App;
use crate::app::manage::backup::DataRow;
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct DataScreen;

impl Screen for DataScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Data
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        let mut v = moves(|a| a.move_data(-1), |a| a.move_data(1)).to_vec();
        match app.selected_data_row() {
            Some(DataRow::Kept(_)) => {
                v.push(Binding::new(ENTER, "Enter", "download", App::data_enter));
                v.push(Binding::new(
                    &[KeyCode::Char('R')],
                    "R",
                    "restore",
                    App::restore_selected_kept,
                ));
            }
            Some(DataRow::Action(a)) => {
                v.push(Binding::new(ENTER, "Enter", a.verb(), App::data_enter));
            }
            Some(DataRow::Calendar(_)) => {
                v.push(Binding::new(
                    DELETE,
                    "x",
                    "delete calendar",
                    App::ask_delete_selected_calendar,
                ));
            }
            None => {}
        }
        if app.is_admin() {
            v.push(Binding::new(NEW, "n", "back up now", |a| {
                a.run_backup_action(crate::app::manage::backup::BackupAction::BackupZip)
            }));
        }
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::settings::draw_data(frame, app, area);
    }

    fn on_enter(&self, app: &mut App) {
        app.load_data_screen();
    }

    fn refresh(&self, app: &mut App) {
        app.status = "Refreshing...".to_string();
        app.load_data_screen();
    }

    fn searchable(&self, _app: &App) -> bool {
        true
    }
}
