//! Helpers: homeCore's own switches, timers, counters and the rest.

use super::{Binding, NEW, SPACE, Screen, ScreenId, moves};
use crate::app::App;
use crate::app::home::{HomeItem, ItemKind};
use ratatui::{Frame, layout::Rect};

pub struct HelpersScreen;

impl Screen for HelpersScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Helpers
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        let mut v = moves(|a| a.move_helper(-1), |a| a.move_helper(1)).to_vec();
        if app.can("devices:command") {
            v.push(Binding::new(
                SPACE,
                "Space",
                "main action",
                App::helper_main_action_selected,
            ));
        }
        if app.can("devices:write") {
            v.push(Binding::new(
                NEW,
                "n",
                "new helper",
                App::open_helper_creator,
            ));
        }
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::lists::draw_helpers(frame, app, area);
    }

    fn searchable(&self, _app: &App) -> bool {
        true
    }

    fn pin_target(&self, app: &App) -> Option<HomeItem> {
        app.selected_helper()
            .map(|d| HomeItem::new(ItemKind::Helper, &d.device_id))
    }
}
