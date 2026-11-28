//! Screens: one module per screen, the sidebar's order, and each
//! screen's key table.
//!
//! A screen is a unit struct that implements [`Screen`]. Its
//! [`Screen::bindings`] is the one list of keys for its current state:
//! the key handler runs from it, the footer shows its short form and the
//! `?` overlay lists all of it, so the three can never disagree.
//!
//! Adding a screen:
//! 1. add a [`ScreenId`] variant (title, sidebar place, who may see it),
//! 2. write `screens/<name>.rs` with a unit struct implementing `Screen`,
//! 3. return it from [`screen`].
//!
//! The state and requests live on `App` (in `app/`), drawing in `ui/`;
//! the screen module joins the two and says which keys do what.

pub mod alerts;
pub mod audit;
pub mod automations;
pub mod data;
pub mod devices;
pub mod events;
pub mod helpers;
pub mod home;
pub mod logs;
pub mod modes;
pub mod people;
pub mod plugins;
pub mod rooms;
pub mod rotations;
pub mod scenes;
pub mod system;

use crate::app::App;
use crate::app::home::HomeItem;
use crate::model::UserInfo;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Frame, layout::Rect};

/// Every screen, in sidebar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScreenId {
    Home,
    Rooms,
    Devices,
    Scenes,
    Modes,
    Helpers,
    Automations,
    Alerts,
    Plugins,
    Rotations,
    People,
    Data,
    Logs,
    Events,
    Audit,
    System,
}

impl ScreenId {
    pub const ALL: [ScreenId; 16] = [
        ScreenId::Home,
        ScreenId::Rooms,
        ScreenId::Devices,
        ScreenId::Scenes,
        ScreenId::Modes,
        ScreenId::Helpers,
        ScreenId::Automations,
        ScreenId::Alerts,
        ScreenId::Plugins,
        ScreenId::Rotations,
        ScreenId::People,
        ScreenId::Data,
        ScreenId::Logs,
        ScreenId::Events,
        ScreenId::Audit,
        ScreenId::System,
    ];

    /// The name in the sidebar, the app's words.
    pub fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Rooms => "Rooms",
            Self::Devices => "Devices",
            Self::Scenes => "Scenes",
            Self::Modes => "Modes",
            Self::Helpers => "Helpers",
            Self::Automations => "Automations",
            Self::Alerts => "Alerts",
            Self::Plugins => "Plugins",
            Self::Rotations => "Rotations",
            Self::People => "People",
            Self::Data => "Data & backups",
            Self::Logs => "Logs",
            Self::Events => "Events",
            Self::Audit => "Audit",
            Self::System => "System",
        }
    }

    /// The digit that jumps here. Only the main screens have one, and it
    /// stays the same whatever else is hidden.
    pub fn hotkey(self) -> Option<char> {
        let n = match self {
            Self::Home => 1,
            Self::Rooms => 2,
            Self::Devices => 3,
            Self::Scenes => 4,
            Self::Modes => 5,
            Self::Helpers => 6,
            Self::Automations => 7,
            Self::Alerts => 8,
            Self::Plugins => 9,
            _ => return None,
        };
        char::from_digit(n, 10)
    }

    pub fn from_hotkey(c: char) -> Option<ScreenId> {
        Self::ALL.into_iter().find(|s| s.hotkey() == Some(c))
    }

    /// Below the Settings divider.
    pub fn is_settings(self) -> bool {
        self.hotkey().is_none()
    }

    /// Whether `user` can use this screen, from the scopes `/auth/me`
    /// sent (or core's role table when it sent none).
    pub fn visible_to(self, user: &UserInfo) -> bool {
        let admin = user.role.is_admin();
        match self {
            Self::Home | Self::System => true,
            Self::Rooms => user.can("areas:read"),
            Self::Devices | Self::Modes | Self::Helpers | Self::Alerts | Self::Events => {
                user.can("devices:read")
            }
            Self::Scenes => user.can("scenes:read"),
            Self::Automations | Self::Rotations => user.can("automations:read"),
            Self::Plugins => user.can("plugins:read"),
            Self::People => user.can("users:read"),
            // The backup zip is admin only; exporting and importing
            // automations and scenes needs authoring rights.
            Self::Data => admin || user.can("automations:write"),
            // The log stream is admin only.
            Self::Logs => admin,
            Self::Audit => user.can("audit:read"),
        }
    }
}

/// The screens `user` can open, in sidebar order.
pub fn visible_screens(user: Option<&UserInfo>) -> Vec<ScreenId> {
    match user {
        Some(u) => ScreenId::ALL
            .into_iter()
            .filter(|s| s.visible_to(u))
            .collect(),
        None => vec![ScreenId::Home],
    }
}

/// One key (or a few that do the same thing) and what it does.
#[derive(Clone, Copy)]
pub struct Binding {
    /// The keys that run it. Empty for an entry that only documents a
    /// key handled elsewhere (the digits).
    pub keys: &'static [KeyCode],
    /// How the key is written: `"Space"`, `"j/k"`.
    pub label: &'static str,
    /// What it does, short: `"room on/off"`.
    pub help: &'static str,
    /// Shown in the footer; every binding shows in `?`.
    pub footer: bool,
    pub run: fn(&mut App),
}

impl std::fmt::Debug for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.label, self.help)
    }
}

impl Binding {
    pub fn new(
        keys: &'static [KeyCode],
        label: &'static str,
        help: &'static str,
        run: fn(&mut App),
    ) -> Self {
        Self {
            keys,
            label,
            help,
            footer: true,
            run,
        }
    }

    /// Listed in `?` only, not in the footer.
    pub fn more(mut self) -> Self {
        self.footer = false;
        self
    }

    /// A help line for keys handled outside the table.
    pub fn doc(label: &'static str, help: &'static str) -> Self {
        Self::new(&[], label, help, |_| {}).more()
    }

    pub fn matches(&self, key: &KeyEvent) -> bool {
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return false;
        }
        self.keys.contains(&key.code)
    }

    /// `"x delete"`, as the footer shows it.
    pub fn short(&self) -> String {
        format!("{} {}", self.label, self.help)
    }
}

pub const UP: &[KeyCode] = &[KeyCode::Up, KeyCode::Char('k')];
pub const DOWN: &[KeyCode] = &[KeyCode::Down, KeyCode::Char('j')];
pub const PAGE_UP: &[KeyCode] = &[KeyCode::PageUp];
pub const PAGE_DOWN: &[KeyCode] = &[KeyCode::PageDown];
pub const ENTER: &[KeyCode] = &[KeyCode::Enter];
pub const SPACE: &[KeyCode] = &[KeyCode::Char(' ')];
pub const NEW: &[KeyCode] = &[KeyCode::Char('n')];
pub const DELETE: &[KeyCode] = &[KeyCode::Char('x'), KeyCode::Delete];
pub const ESC: &[KeyCode] = &[KeyCode::Esc];

/// A key press with no modifiers, for passing a bound key on to a
/// handler that matches on keys itself.
pub fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// `j/k` (and the arrows) for a list.
pub fn moves(up: fn(&mut App), down: fn(&mut App)) -> [Binding; 2] {
    [
        Binding::new(DOWN, "j", "move down", down).more(),
        Binding::new(UP, "k", "move up", up).more(),
    ]
}

/// A screen of the main UI.
pub trait Screen: Sync {
    fn id(&self) -> ScreenId;

    /// The keys for the screen's current state (a detail view open, a
    /// sub-tab chosen). Global keys are added by the app.
    fn bindings(&self, app: &App) -> Vec<Binding>;

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect);

    /// Called when the screen comes on, to load what it needs.
    fn on_enter(&self, _app: &mut App) {}

    /// `r`. Most screens resync everything.
    fn refresh(&self, app: &mut App) {
        app.status = "Refreshing...".to_string();
        app.start_sync();
    }

    /// Whether `/` searches this screen's list.
    fn searchable(&self, _app: &App) -> bool {
        false
    }

    /// `Esc`: close a detail or go back. True when it did something.
    fn back(&self, _app: &mut App) -> bool {
        false
    }

    /// What `*` pins to Home: the item under the cursor.
    fn pin_target(&self, _app: &App) -> Option<HomeItem> {
        None
    }

    /// A count for the sidebar, such as open alerts.
    fn badge(&self, _app: &App) -> Option<usize> {
        None
    }
}

/// The screen for `id`. The one place a new screen is registered.
pub fn screen(id: ScreenId) -> &'static dyn Screen {
    match id {
        ScreenId::Home => &home::HomeScreen,
        ScreenId::Rooms => &rooms::RoomsScreen,
        ScreenId::Devices => &devices::DevicesScreen,
        ScreenId::Scenes => &scenes::ScenesScreen,
        ScreenId::Rotations => &rotations::RotationsScreen,
        ScreenId::Modes => &modes::ModesScreen,
        ScreenId::Helpers => &helpers::HelpersScreen,
        ScreenId::Automations => &automations::AutomationsScreen,
        ScreenId::Alerts => &alerts::AlertsScreen,
        ScreenId::Plugins => &plugins::PluginsScreen,
        ScreenId::People => &people::PeopleScreen,
        ScreenId::Data => &data::DataScreen,
        ScreenId::Logs => &logs::LogsScreen,
        ScreenId::Events => &events::EventsScreen,
        ScreenId::Audit => &audit::AuditScreen,
        ScreenId::System => &system::SystemScreen,
    }
}

/// Keys that work on every screen, after the screen's own. The digits
/// are handled by the app; their entry here is for `?`.
pub fn global_bindings(app: &App) -> Vec<Binding> {
    let s = screen(app.screen);
    let mut v = Vec::new();
    if s.searchable(app) {
        v.push(Binding::new(
            &[KeyCode::Char('/')],
            "/",
            "search",
            App::open_search,
        ));
    }
    v.extend([
        Binding::new(&[KeyCode::Char('?')], "?", "keys", App::toggle_help),
        Binding::new(&[KeyCode::Char('r')], "r", "refresh", |a| {
            screen(a.screen).refresh(a)
        })
        .more(),
        Binding::new(&[KeyCode::Char('*')], "*", "pin or unpin on Home", |a| {
            a.pin_selected()
        })
        .more(),
        Binding::new(ESC, "Esc", "back", App::go_back).more(),
        Binding::doc("1 to 9", "jump to a screen"),
        Binding::new(&[KeyCode::Tab], "Tab", "next screen", |a| a.step_screen(1)).more(),
        Binding::new(&[KeyCode::BackTab], "Shift+Tab", "previous screen", |a| {
            a.step_screen(-1)
        })
        .more(),
        Binding::new(
            &[KeyCode::Char('T')],
            "T",
            "local or UTC time",
            App::toggle_utc,
        )
        .more(),
        Binding::new(&[KeyCode::Char('L')], "L", "sign out", App::logout).more(),
        Binding::new(&[KeyCode::Char('q')], "q", "quit", |a| a.should_quit = true).more(),
    ]);
    v
}

/// The footer for the current state: the screen's footer keys, then
/// `/ search` and `? keys`.
pub fn footer_hints(app: &App) -> Vec<String> {
    let mut out: Vec<String> = screen(app.screen)
        .bindings(app)
        .iter()
        .filter(|b| b.footer)
        .map(Binding::short)
        .collect();
    out.extend(
        global_bindings(app)
            .iter()
            .filter(|b| b.footer)
            .map(Binding::short),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::model::Role;

    fn titles(role: Role) -> Vec<&'static str> {
        visible_screens(Some(&user(role)))
            .into_iter()
            .map(ScreenId::title)
            .collect()
    }

    #[test]
    fn admins_see_every_screen() {
        assert_eq!(titles(Role::Admin).len(), ScreenId::ALL.len());
    }

    #[test]
    fn a_household_member_sees_the_house_and_no_admin_screens() {
        let t = titles(Role::HouseholdMember);
        for want in [
            "Home",
            "Rooms",
            "Devices",
            "Scenes",
            "Modes",
            "Helpers",
            "Automations",
            "Alerts",
            "Events",
            "System",
        ] {
            assert!(t.contains(&want), "{want} missing from {t:?}");
        }
        for hidden in ["Plugins", "People", "Data & backups", "Logs", "Audit"] {
            assert!(!t.contains(&hidden), "{hidden} shown to {t:?}");
        }
    }

    #[test]
    fn plugins_and_audit_follow_their_scopes() {
        let t = titles(Role::Observer);
        assert!(t.contains(&"Plugins") && t.contains(&"Audit"));
        assert!(!t.contains(&"People") && !t.contains(&"Logs"));
        let t = titles(Role::User);
        assert!(!t.contains(&"Plugins") && !t.contains(&"Audit"));
        assert!(t.contains(&"Data & backups"), "users author automations");
        let t = titles(Role::RuleEditor);
        assert!(t.contains(&"Plugins") && t.contains(&"Data & backups"));
    }

    #[test]
    fn server_scopes_win_over_the_role_table() {
        let mut u = user(Role::User);
        u.scopes = Some(vec!["devices:read".into(), "plugins:read".into()]);
        let t: Vec<_> = visible_screens(Some(&u));
        assert!(t.contains(&ScreenId::Plugins));
        assert!(!t.contains(&ScreenId::Rooms), "no areas:read");
    }

    #[test]
    fn every_screen_is_registered_under_its_own_id() {
        for id in ScreenId::ALL {
            assert_eq!(screen(id).id(), id);
        }
    }

    #[test]
    fn hotkeys_are_fixed_and_unique() {
        let keys: Vec<char> = ScreenId::ALL.iter().filter_map(|s| s.hotkey()).collect();
        assert_eq!(keys, vec!['1', '2', '3', '4', '5', '6', '7', '8', '9']);
        assert_eq!(ScreenId::from_hotkey('9'), Some(ScreenId::Plugins));
        assert!(ScreenId::People.is_settings());
    }

    /// Every state of every screen: the footer comes from the table, no
    /// key does two things, and `d` deletes nothing.
    #[test]
    fn key_tables_and_footers_agree() {
        for role in [Role::Admin, Role::HouseholdMember, Role::Observer] {
            let mut a = signed_in(role);
            for id in ScreenId::ALL {
                a.screen = id;
                for state in 0..crate::app::test_support::SCREEN_STATES {
                    crate::app::test_support::enter_state(&mut a, id, state);
                    let table = screen(id).bindings(&a);
                    let footer = footer_hints(&a);
                    let mut all = table.clone();
                    all.extend(global_bindings(&a));
                    for f in &footer {
                        assert!(
                            all.iter().any(|b| &b.short() == f),
                            "{id:?}: footer {f} not in the key table"
                        );
                    }
                    assert!(footer.iter().any(|f| f == "? keys"), "{id:?}: no ? keys");
                    let mut seen: Vec<KeyCode> = Vec::new();
                    for b in &table {
                        for k in b.keys {
                            assert!(!seen.contains(k), "{id:?}: {k:?} bound twice");
                            seen.push(*k);
                        }
                        if b.keys.contains(&KeyCode::Char('d')) {
                            assert!(
                                !b.help.contains("delete") && !b.help.contains("remove"),
                                "{id:?}: d deletes"
                            );
                        }
                        if b.keys.contains(&KeyCode::Char('x')) {
                            assert!(
                                ["delete", "remove", "uninstall", "revoke", "end"]
                                    .iter()
                                    .any(|w| b.help.contains(w)),
                                "{id:?}: x does {}",
                                b.help
                            );
                        }
                        for w in ["Deregister", "Fire history", "Rules", "Areas"] {
                            assert!(!b.help.contains(w), "{id:?}: old word {w}");
                        }
                        assert!(!b.help.contains('\u{2014}') && !b.help.contains('\u{2013}'));
                    }
                }
            }
        }
    }
}
