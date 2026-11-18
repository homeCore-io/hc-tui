//! Scenes: homeCore's scenes and every plugin's. homeCore scenes can be
//! made, edited and deleted here; a plugin's scenes only run.

use super::{Binding, DELETE, ENTER, NEW, SPACE, Screen, ScreenId, moves};
use crate::app::App;
use crate::app::home::{HomeItem, ItemKind};
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct ScenesScreen;

impl Screen for ScenesScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Scenes
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        let mut v = moves(|a| a.move_scene(-1), |a| a.move_scene(1)).to_vec();
        if app.can("scenes:run") {
            v.extend([
                Binding::new(
                    &[KeyCode::Char('a')],
                    "a",
                    "activate",
                    App::activate_selected_scene,
                ),
                Binding::new(SPACE, "Space", "activate", App::activate_selected_scene).more(),
            ]);
        }
        if app.can("scenes:write") {
            v.extend([
                Binding::new(NEW, "n", "new", App::new_scene),
                Binding::new(ENTER, "Enter", "edit", App::edit_selected_scene),
                Binding::new(DELETE, "x", "delete", App::ask_delete_scene).more(),
            ]);
        }
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::scenes::draw_scenes_table(frame, app, area);
    }

    fn searchable(&self, _app: &App) -> bool {
        true
    }

    fn pin_target(&self, app: &App) -> Option<HomeItem> {
        app.selected_scene()
            .map(|s| HomeItem::new(ItemKind::Scene, &s.id))
    }
}
