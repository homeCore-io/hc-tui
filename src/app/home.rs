//! Home: the rooms, devices, scenes, modes, helpers and automations a
//! person picked, under headings they named.
//!
//! The list is kept per server and per signed-in user in hc-tui's config
//! folder, `~/.config/hc-tui/home/<server>-<user>.json`, written 0600
//! through a temporary file. Items are kept by id; one whose target is
//! gone shows as missing until it is removed. Core keeps no UI settings
//! and the app's favorites live on the app's own server, so this Home is
//! hc-tui's alone.

use super::{App, ListSel};
use crate::cache::{sanitize_component, write_private};
use crate::screens::ScreenId;
use crate::text::matches_query;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The name of the heading made when there is none yet.
pub const DEFAULT_HEADING: &str = "Favorites";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Room,
    Device,
    Scene,
    Mode,
    Helper,
    Automation,
}

impl ItemKind {
    pub const ALL: [ItemKind; 6] = [
        ItemKind::Room,
        ItemKind::Device,
        ItemKind::Scene,
        ItemKind::Mode,
        ItemKind::Helper,
        ItemKind::Automation,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Room => "room",
            Self::Device => "device",
            Self::Scene => "scene",
            Self::Mode => "mode",
            Self::Helper => "helper",
            Self::Automation => "automation",
        }
    }

    /// The picker's tab name.
    pub fn plural(self) -> &'static str {
        match self {
            Self::Room => "Rooms",
            Self::Device => "Devices",
            Self::Scene => "Scenes",
            Self::Mode => "Modes",
            Self::Helper => "Helpers",
            Self::Automation => "Automations",
        }
    }

    /// The screen that shows this kind.
    pub fn screen(self) -> ScreenId {
        match self {
            Self::Room => ScreenId::Rooms,
            Self::Device => ScreenId::Devices,
            Self::Scene => ScreenId::Scenes,
            Self::Mode => ScreenId::Modes,
            Self::Helper => ScreenId::Helpers,
            Self::Automation => ScreenId::Automations,
        }
    }
}

/// One thing on Home, by kind and id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HomeItem {
    pub kind: ItemKind,
    pub id: String,
}

impl HomeItem {
    pub fn new(kind: ItemKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
        }
    }

    /// Unique on Home; what the cursor follows.
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HomeGroup {
    pub name: String,
    #[serde(default)]
    pub items: Vec<HomeItem>,
}

/// The saved file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HomeLayout {
    #[serde(default = "layout_version")]
    pub version: u32,
    #[serde(default)]
    pub groups: Vec<HomeGroup>,
}

fn layout_version() -> u32 {
    1
}

impl Default for HomeLayout {
    fn default() -> Self {
        Self {
            version: layout_version(),
            groups: Vec::new(),
        }
    }
}

impl HomeLayout {
    pub fn is_empty(&self) -> bool {
        self.groups.iter().all(|g| g.items.is_empty())
    }

    pub fn contains(&self, item: &HomeItem) -> bool {
        self.position(item).is_some()
    }

    /// `(group, index)` of `item`.
    pub fn position(&self, item: &HomeItem) -> Option<(usize, usize)> {
        self.groups
            .iter()
            .enumerate()
            .find_map(|(g, grp)| grp.items.iter().position(|i| i == item).map(|i| (g, i)))
    }

    /// Every item in display order.
    pub fn items(&self) -> impl Iterator<Item = &HomeItem> {
        self.groups.iter().flat_map(|g| g.items.iter())
    }

    pub fn keys(&self) -> Vec<String> {
        self.items().map(HomeItem::key).collect()
    }

    pub fn find_key(&self, key: &str) -> Option<&HomeItem> {
        self.items().find(|i| i.key() == key)
    }

    /// Add `item` under heading `group` (made as [`DEFAULT_HEADING`]
    /// when there is no heading). False when it is already on Home.
    pub fn add(&mut self, item: HomeItem, group: usize) -> bool {
        if self.contains(&item) {
            return false;
        }
        if self.groups.is_empty() {
            self.groups.push(HomeGroup {
                name: DEFAULT_HEADING.to_string(),
                items: Vec::new(),
            });
        }
        let g = group.min(self.groups.len() - 1);
        self.groups[g].items.push(item);
        true
    }

    /// Take `item` off Home. A heading left empty goes too.
    pub fn remove(&mut self, item: &HomeItem) -> bool {
        let Some((g, i)) = self.position(item) else {
            return false;
        };
        self.groups[g].items.remove(i);
        self.prune();
        true
    }

    /// `*`: pin when absent (under the first heading), unpin when there.
    /// Returns true when the item is on Home afterwards.
    pub fn toggle(&mut self, item: HomeItem) -> bool {
        if self.remove(&item) {
            false
        } else {
            self.add(item, 0)
        }
    }

    /// Drop headings with nothing under them.
    pub fn prune(&mut self) {
        self.groups.retain(|g| !g.items.is_empty());
    }

    /// Move `item` one place up (`-1`) or down (`1`). At the edge of its
    /// heading it moves into the next heading over.
    pub fn move_item(&mut self, item: &HomeItem, delta: isize) -> bool {
        let Some((g, i)) = self.position(item) else {
            return false;
        };
        let len = self.groups[g].items.len();
        if delta < 0 {
            if i > 0 {
                self.groups[g].items.swap(i, i - 1);
            } else if g > 0 {
                let it = self.groups[g].items.remove(0);
                self.groups[g - 1].items.push(it);
            } else {
                return false;
            }
        } else if i + 1 < len {
            self.groups[g].items.swap(i, i + 1);
        } else if g + 1 < self.groups.len() {
            let it = self.groups[g].items.remove(i);
            self.groups[g + 1].items.insert(0, it);
        } else {
            return false;
        }
        self.prune();
        true
    }
}

// ── The file ────────────────────────────────────────────────────────────────

/// `<server>-<user>.json`. The server is the base URL without its scheme.
pub fn home_file_name(base_url: &str, username: &str) -> String {
    let server = base_url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    format!(
        "{}-{}.json",
        sanitize_component(server),
        sanitize_component(username)
    )
}

pub fn load_layout(path: &Path) -> HomeLayout {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_layout(path: &Path, layout: &HomeLayout) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(layout)?;
    write_private(path, &bytes)?;
    Ok(())
}

// ── Screen state ────────────────────────────────────────────────────────────

/// The `a` picker: search, a kind tab and the heading to add under.
#[derive(Debug, Default, Clone)]
pub struct Picker {
    pub query: String,
    /// `None` is the All tab.
    pub kind: Option<ItemKind>,
    pub sel: ListSel,
    pub group: usize,
}

impl Picker {
    pub fn tabs() -> Vec<Option<ItemKind>> {
        std::iter::once(None)
            .chain(ItemKind::ALL.into_iter().map(Some))
            .collect()
    }

    fn step_kind(&mut self, forward: bool) {
        let tabs = Self::tabs();
        let i = tabs.iter().position(|k| *k == self.kind).unwrap_or(0);
        let n = tabs.len();
        self.kind = tabs[if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        }];
        self.sel.reset();
    }
}

/// `h`: rename the heading under the cursor, or add a new one.
#[derive(Debug, Clone)]
pub struct HeadingEditor {
    pub name: String,
    /// The heading being renamed; `None` adds a new heading.
    pub rename: Option<usize>,
}

#[derive(Debug, Default)]
pub struct HomeState {
    pub layout: HomeLayout,
    pub sel: ListSel,
    pub picker: Option<Picker>,
    pub heading: Option<HeadingEditor>,
    /// The file this layout came from and is saved to.
    pub path: Option<PathBuf>,
}

/// A row of the picker.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub item: HomeItem,
    pub name: String,
    pub kind_label: String,
    /// Room or plugin, to tell like names apart.
    pub context: String,
}

/// What an item on Home shows.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryView {
    pub name: String,
    pub kind_label: String,
    pub state: String,
    /// On, active or enabled, for color.
    pub on: Option<bool>,
}

/// Helpers are homeCore's own devices.
pub fn is_helper(d: &crate::model::DeviceState) -> bool {
    d.plugin_id == "core.glue"
}

pub fn is_mode_device(d: &crate::model::DeviceState) -> bool {
    d.plugin_id == "core.mode" || d.device_type() == "mode"
}

/// Everything that can go on Home, narrowed to `kind` and `query`.
pub fn candidates(app: &App, kind: Option<ItemKind>, query: &str) -> Vec<Candidate> {
    let want = |k: ItemKind| kind.is_none_or(|w| w == k);
    let mut out = Vec::new();
    if want(ItemKind::Room) {
        for a in &app.areas {
            out.push(Candidate {
                item: HomeItem::new(ItemKind::Room, &a.id),
                name: a.name.clone(),
                kind_label: "room".into(),
                context: String::new(),
            });
        }
    }
    if want(ItemKind::Device) {
        for d in &app.devices {
            if d.is_scene() || is_helper(d) || is_mode_device(d) {
                continue;
            }
            out.push(Candidate {
                item: HomeItem::new(ItemKind::Device, &d.device_id),
                name: d.display_name().to_string(),
                kind_label: kind_word(d.device_type()),
                context: app.room_label(d).unwrap_or_default(),
            });
        }
    }
    if want(ItemKind::Scene) {
        for s in &app.scenes {
            out.push(Candidate {
                item: HomeItem::new(ItemKind::Scene, &s.id),
                name: s.name.clone(),
                kind_label: "scene".into(),
                context: s.room.clone().unwrap_or_else(|| scene_source(s)),
            });
        }
    }
    if want(ItemKind::Mode) {
        for m in &app.modes {
            out.push(Candidate {
                item: HomeItem::new(ItemKind::Mode, &m.config.id),
                name: m.config.name.clone(),
                kind_label: "mode".into(),
                context: m.config.kind.clone(),
            });
        }
    }
    if want(ItemKind::Helper) {
        for d in app.devices.iter().filter(|d| is_helper(d)) {
            out.push(Candidate {
                item: HomeItem::new(ItemKind::Helper, &d.device_id),
                name: d.display_name().to_string(),
                kind_label: kind_word(d.device_type()),
                context: app.room_label(d).unwrap_or_default(),
            });
        }
    }
    if want(ItemKind::Automation) {
        for r in &app.rules {
            out.push(Candidate {
                item: HomeItem::new(ItemKind::Automation, &r.id),
                name: r.name.clone(),
                kind_label: "automation".into(),
                context: String::new(),
            });
        }
    }
    out.retain(|c| matches_query(query, &[&c.name, &c.context, &c.kind_label]));
    // Kinds in tab order, names A to Z within each.
    out.sort_by_cached_key(|c| {
        (
            ItemKind::ALL.iter().position(|k| *k == c.item.kind),
            c.name.to_lowercase(),
        )
    });
    out
}

fn kind_word(device_type: &str) -> String {
    match device_type {
        "" => "device".into(),
        t => t.replace('_', " "),
    }
}

fn scene_source(s: &crate::model::Scene) -> String {
    match (s.source, s.plugin_id.as_deref()) {
        (crate::model::SceneSource::Native, _) => "homeCore".into(),
        (_, Some(p)) => p.trim_start_matches("plugin.").to_string(),
        _ => String::new(),
    }
}

fn on_word(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn bool_attr(d: &crate::model::DeviceState, k: &str) -> Option<bool> {
    d.attributes.get(k).and_then(Value::as_bool)
}

/// `12:41` or `1:02:03`.
pub fn clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

impl App {
    // ── Loading and saving ───────────────────────────────────────────────

    /// Read this user's Home for this server. Called on sign-in.
    pub(crate) fn load_home(&mut self) {
        let Some(user) = self.current_user.as_ref() else {
            return;
        };
        let path = self
            .home_dir
            .join(home_file_name(self.client.base_url(), &user.username));
        self.home.layout = load_layout(&path);
        self.home.path = Some(path);
        self.home.sel.reset();
    }

    /// Write Home in the background.
    pub(crate) fn save_home(&self) {
        let Some(path) = self.home.path.clone() else {
            return;
        };
        let layout = self.home.layout.clone();
        let write = move || {
            let _ = save_layout(&path, &layout);
        };
        match tokio::runtime::Handle::try_current() {
            Ok(h) => {
                h.spawn_blocking(write);
            }
            Err(_) => write(),
        }
    }

    pub fn selected_home_item(&self) -> Option<HomeItem> {
        let key = self.home.sel.id()?;
        self.home.layout.find_key(key).cloned()
    }

    pub(crate) fn resolve_home_selection(&mut self) {
        let keys = self.home.layout.keys();
        self.home.sel.resolve(&keys);
    }

    // ── What an item shows ───────────────────────────────────────────────

    /// The live view of `item`; `None` when its target is gone.
    pub fn home_entry(&self, item: &HomeItem) -> Option<EntryView> {
        match item.kind {
            ItemKind::Room => {
                let a = self.areas.iter().find(|a| a.id == item.id)?;
                let s = self.room_summary(a);
                let state = match (s.lights_on, s.lights_total) {
                    (_, 0) => "-".to_string(),
                    (0, _) => "off".to_string(),
                    (on, total) => format!("{on} of {total} on"),
                };
                Some(EntryView {
                    name: a.name.clone(),
                    kind_label: "room".into(),
                    state,
                    on: (s.lights_total > 0).then_some(s.lights_on > 0),
                })
            }
            ItemKind::Device | ItemKind::Helper => {
                let d = self.device_by_id(&item.id)?;
                let (state, on) = if is_helper(d) {
                    self.helper_state(d)
                } else {
                    let on = bool_attr(d, "on").or_else(|| bool_attr(d, "locked"));
                    let mut st = self.device_status(d).to_lowercase();
                    if !d.available {
                        st = "offline".into();
                    }
                    (st, on)
                };
                Some(EntryView {
                    name: d.display_name().to_string(),
                    kind_label: kind_word(d.device_type()),
                    state,
                    on,
                })
            }
            ItemKind::Scene => {
                let s = self.scenes.iter().find(|s| s.id == item.id)?;
                Some(EntryView {
                    name: s.name.clone(),
                    kind_label: "scene".into(),
                    state: match s.active {
                        Some(true) => "active".into(),
                        _ => String::new(),
                    },
                    on: s.active,
                })
            }
            ItemKind::Mode => {
                let m = self.modes.iter().find(|m| m.config.id == item.id)?;
                let on = self.mode_on(&m.config.id);
                Some(EntryView {
                    name: m.config.name.clone(),
                    kind_label: "mode".into(),
                    state: on_word(on).into(),
                    on: Some(on),
                })
            }
            ItemKind::Automation => {
                let r = self.rules.iter().find(|r| r.id == item.id)?;
                let state = if r.error.is_some() {
                    "needs fixing".to_string()
                } else {
                    on_word(r.enabled).to_string()
                };
                Some(EntryView {
                    name: r.name.clone(),
                    kind_label: "automation".into(),
                    state,
                    on: Some(r.enabled),
                })
            }
        }
    }

    /// A helper's state and whether it counts as on.
    pub fn helper_state(&self, d: &crate::model::DeviceState) -> (String, Option<bool>) {
        use super::devices::{is_timer, timer_remaining_secs, timer_state};
        if is_timer(d) {
            let st = timer_state(d);
            let left = timer_remaining_secs(d);
            return match st {
                "running" => (format!("running {} left", clock(left)), Some(true)),
                "paused" => (format!("paused {} left", clock(left)), Some(false)),
                other => (other.to_string(), Some(false)),
            };
        }
        if let Some(on) = bool_attr(d, "on") {
            return (on_word(on).into(), Some(on));
        }
        for k in ["value", "count", "option", "selected"] {
            if let Some(v) = d.attributes.get(k) {
                let s = match v {
                    Value::String(s) => s.clone(),
                    Value::Null => "-".into(),
                    other => other.to_string(),
                };
                return (s, None);
            }
        }
        (self.device_status(d).to_lowercase(), None)
    }

    pub fn mode_on(&self, id: &str) -> bool {
        self.device_by_id(id)
            .and_then(|d| bool_attr(d, "on"))
            .or_else(|| {
                self.modes
                    .iter()
                    .find(|m| m.config.id == id)
                    .and_then(|m| m.state.as_ref())
                    .and_then(|s| bool_attr(s, "on"))
            })
            .unwrap_or(false)
    }

    // ── Actions ──────────────────────────────────────────────────────────

    /// Space on Home: the item's main action.
    pub fn home_main_action(&mut self) {
        let Some(item) = self.selected_home_item() else {
            return;
        };
        if self.home_entry(&item).is_none() {
            self.status = "That item is missing; x takes it off Home".into();
            return;
        }
        match item.kind {
            ItemKind::Room => {
                let on = self
                    .areas
                    .iter()
                    .find(|a| a.id == item.id)
                    .map(|a| self.room_summary(a).lights_on == 0)
                    .unwrap_or(true);
                self.room_switch(item.id.clone(), on);
            }
            ItemKind::Device | ItemKind::Helper => self.device_main_action(&item.id),
            ItemKind::Scene => self.activate_scene_id(&item.id),
            ItemKind::Mode => self.toggle_mode(&item.id),
            ItemKind::Automation => self.toggle_rule_id(&item.id),
        }
    }

    /// Enter on Home: open the item on its own screen.
    pub fn home_open(&mut self) {
        let Some(item) = self.selected_home_item() else {
            return;
        };
        if self.home_entry(&item).is_none() {
            self.status = "That item is missing; x takes it off Home".into();
            return;
        }
        self.open_item(&item);
        self.return_to = Some(ScreenId::Home);
    }

    /// Show `item` selected on the screen for its kind.
    pub fn open_item(&mut self, item: &HomeItem) {
        let id = item.id.clone();
        match item.kind {
            ItemKind::Room => self.rooms.sel.select_id(id),
            ItemKind::Device => {
                self.devices_tab.sub = super::DeviceSubPanel::All;
                self.devices_tab.filter = Default::default();
                self.devices_tab.room = None;
                self.queries.remove(&ScreenId::Devices);
                self.devices_tab.sel.select_id(id);
            }
            ItemKind::Scene => {
                self.queries.remove(&ScreenId::Scenes);
                self.scenes_sel.select_id(id);
            }
            ItemKind::Mode => {
                self.queries.remove(&ScreenId::Modes);
                self.manage.modes_sel.select_id(id);
            }
            ItemKind::Helper => {
                self.queries.remove(&ScreenId::Helpers);
                self.helpers_sel.select_id(id);
            }
            ItemKind::Automation => {
                self.queries.remove(&ScreenId::Automations);
                self.rules_tab.filter_tag.clear();
                self.rules_tab.filter_trigger.clear();
                self.rules_tab.filter_stale = false;
                self.rules_tab.sel.select_id(id);
            }
        }
        self.switch_screen(item.kind.screen());
    }

    /// `x` on Home. The item stays in homeCore, so nothing asks first.
    pub fn home_remove(&mut self) {
        let Some(item) = self.selected_home_item() else {
            return;
        };
        let name = self
            .home_entry(&item)
            .map(|e| e.name)
            .unwrap_or_else(|| item.id.clone());
        self.home.layout.remove(&item);
        self.save_home();
        self.status = format!("Took {name} off Home");
    }

    /// `J` / `K`.
    pub fn home_move(&mut self, delta: isize) {
        let Some(item) = self.selected_home_item() else {
            return;
        };
        if self.home.layout.move_item(&item, delta) {
            self.save_home();
        }
    }

    /// `*` from any list: pin or unpin what the screen has selected.
    pub fn pin_item(&mut self, item: HomeItem) {
        let name = self
            .home_entry(&item)
            .map(|e| e.name)
            .unwrap_or_else(|| item.id.clone());
        let now = self.home.layout.toggle(item);
        self.save_home();
        self.status = if now {
            format!("Pinned {name} to Home")
        } else {
            format!("Took {name} off Home")
        };
    }

    // ── Picker ───────────────────────────────────────────────────────────

    pub fn open_home_picker(&mut self) {
        let group = self
            .selected_home_item()
            .and_then(|i| self.home.layout.position(&i))
            .map(|(g, _)| g)
            .unwrap_or(0);
        self.home.picker = Some(Picker {
            group,
            ..Default::default()
        });
    }

    /// The picker's rows for its current tab and search.
    pub fn picker_rows(&self) -> Vec<Candidate> {
        match &self.home.picker {
            Some(p) => candidates(self, p.kind, &p.query),
            None => Vec::new(),
        }
    }

    /// The headings the picker can add under. With none yet, the one
    /// that will be made.
    pub fn picker_headings(&self) -> Vec<String> {
        if self.home.layout.groups.is_empty() {
            vec![DEFAULT_HEADING.to_string()]
        } else {
            self.home
                .layout
                .groups
                .iter()
                .map(|g| g.name.clone())
                .collect()
        }
    }

    pub(crate) fn on_key_picker(&mut self, key: KeyEvent) {
        let rows = self.picker_rows();
        let keys: Vec<String> = rows.iter().map(|c| c.item.key()).collect();
        let headings = self.picker_headings().len();
        let Some(p) = self.home.picker.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.home.picker = None;
                self.home.layout.prune();
                self.save_home();
            }
            KeyCode::Tab => p.step_kind(true),
            KeyCode::BackTab => p.step_kind(false),
            KeyCode::Down => p.sel.move_by(1, &keys),
            KeyCode::Up => p.sel.move_by(-1, &keys),
            KeyCode::PageDown => p.sel.move_by(10, &keys),
            KeyCode::PageUp => p.sel.move_by(-10, &keys),
            KeyCode::Left => p.group = (p.group + headings - 1) % headings.max(1),
            KeyCode::Right => p.group = (p.group + 1) % headings.max(1),
            KeyCode::Backspace => {
                p.query.pop();
                p.sel.reset();
            }
            KeyCode::Enter => {
                p.sel.resolve(&keys);
                let idx = p.sel.index();
                let group = p.group;
                let Some(c) = rows.get(idx).cloned() else {
                    return;
                };
                if self.home.layout.add(c.item.clone(), group) {
                    self.home.sel.select_id(c.item.key());
                    self.save_home();
                    self.status = format!("Added {} to Home", c.name);
                } else {
                    self.status = format!("{} is already on Home", c.name);
                }
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                p.query.push(ch);
                p.sel.reset();
            }
            _ => {}
        }
    }

    pub(crate) fn resolve_picker_selection(&mut self) {
        if self.home.picker.is_none() {
            return;
        }
        let keys: Vec<String> = self.picker_rows().iter().map(|c| c.item.key()).collect();
        if let Some(p) = self.home.picker.as_mut() {
            p.sel.resolve(&keys);
        }
    }

    // ── Headings ─────────────────────────────────────────────────────────

    /// `h`: rename the heading under the cursor (Tab switches to adding
    /// a new one).
    pub fn open_heading_editor(&mut self) {
        let current = self
            .selected_home_item()
            .and_then(|i| self.home.layout.position(&i))
            .map(|(g, _)| g);
        self.home.heading = Some(match current {
            Some(g) => HeadingEditor {
                name: self.home.layout.groups[g].name.clone(),
                rename: Some(g),
            },
            None => HeadingEditor {
                name: String::new(),
                rename: None,
            },
        });
    }

    pub(crate) fn on_key_heading(&mut self, key: KeyEvent) {
        let current = self
            .selected_home_item()
            .and_then(|i| self.home.layout.position(&i))
            .map(|(g, _)| g);
        let Some(ed) = self.home.heading.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.home.heading = None,
            KeyCode::Tab | KeyCode::BackTab => {
                ed.rename = match ed.rename {
                    Some(_) => None,
                    None => current,
                };
                ed.name = match ed.rename {
                    Some(g) => self.home.layout.groups[g].name.clone(),
                    None => String::new(),
                };
            }
            KeyCode::Enter => {
                let name = ed.name.trim().to_string();
                if name.is_empty() {
                    self.error = Some("a heading needs a name".into());
                    return;
                }
                match ed.rename {
                    Some(g) if g < self.home.layout.groups.len() => {
                        self.home.layout.groups[g].name = name.clone();
                        self.home.heading = None;
                        self.save_home();
                        self.status = format!("Renamed the heading to {name}");
                    }
                    _ => {
                        // A new heading: open the picker on it, since a
                        // heading with nothing under it is not kept.
                        self.home.layout.groups.push(HomeGroup {
                            name: name.clone(),
                            items: Vec::new(),
                        });
                        let group = self.home.layout.groups.len() - 1;
                        self.home.heading = None;
                        self.home.picker = Some(Picker {
                            group,
                            ..Default::default()
                        });
                        self.status = format!("Pick what goes under {name}");
                    }
                }
            }
            _ => {
                super::editors::edit_text(&mut ed.name, key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::model::Role;

    fn item(kind: ItemKind, id: &str) -> HomeItem {
        HomeItem::new(kind, id)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "hc-tui-home-{tag}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ))
    }

    #[test]
    fn the_file_name_names_server_and_user() {
        let a = home_file_name("http://127.0.0.1:18080/", "john");
        let b = home_file_name("http://10.0.0.2:8080", "john");
        let c = home_file_name("http://127.0.0.1:18080", "jane");
        assert_eq!(a, "127%2E0%2E0%2E1%3A18080-john.json");
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert!(!home_file_name("http://h", "../x").contains('/'));
    }

    #[test]
    fn layout_round_trips_through_its_file() {
        let dir = temp_dir("rt");
        let path = dir.join("home").join("s-u.json");
        let mut l = HomeLayout::default();
        l.add(item(ItemKind::Room, "kitchen"), 0);
        l.groups.push(HomeGroup {
            name: "Quick".into(),
            items: vec![item(ItemKind::Scene, "evening")],
        });
        save_layout(&path, &l).unwrap();
        assert_eq!(load_layout(&path), l);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        // A missing or broken file is an empty Home.
        assert!(load_layout(&dir.join("nope.json")).is_empty());
        std::fs::write(&path, b"{not json").unwrap();
        assert!(load_layout(&path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sign_in_loads_this_users_home() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _g = rt.enter();
        let dir = temp_dir("signin");
        let mut a = signed_in(Role::Admin);
        a.home_dir = dir.clone();
        a.load_home();
        a.home.layout.add(item(ItemKind::Device, "lamp"), 0);
        save_layout(a.home.path.as_ref().unwrap(), &a.home.layout).unwrap();

        let mut b = signed_in(Role::Admin);
        b.home_dir = dir.clone();
        b.load_home();
        assert!(b.home.layout.contains(&item(ItemKind::Device, "lamp")));

        let mut other = signed_in(Role::User);
        other.home_dir = dir.clone();
        other.load_home();
        assert!(other.home.layout.is_empty(), "each user has their own");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_targets_show_as_missing_and_can_be_removed() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _g = rt.enter();
        let mut a = signed_in(Role::Admin);
        a.devices = vec![device("lamp", "Lamp")];
        a.home.layout.add(item(ItemKind::Device, "lamp"), 0);
        a.home.layout.add(item(ItemKind::Device, "gone"), 0);
        a.home.layout.add(item(ItemKind::Automation, "r-gone"), 0);
        assert!(a.home_entry(&item(ItemKind::Device, "lamp")).is_some());
        assert!(a.home_entry(&item(ItemKind::Device, "gone")).is_none());
        assert!(
            a.home_entry(&item(ItemKind::Automation, "r-gone"))
                .is_none()
        );

        a.refresh_views();
        a.home.sel.select_id(item(ItemKind::Device, "gone").key());
        a.home_main_action();
        assert!(a.status.contains("missing"));
        a.home_remove();
        assert!(!a.home.layout.contains(&item(ItemKind::Device, "gone")));
        assert_eq!(a.home.layout.keys().len(), 2);
    }

    #[test]
    fn toggling_pins_and_empty_headings_go() {
        let mut l = HomeLayout::default();
        assert!(l.toggle(item(ItemKind::Scene, "s")));
        assert_eq!(l.groups[0].name, DEFAULT_HEADING);
        assert!(!l.add(item(ItemKind::Scene, "s"), 0), "no duplicates");
        assert!(!l.toggle(item(ItemKind::Scene, "s")));
        assert!(l.groups.is_empty());
    }

    #[test]
    fn moving_crosses_headings() {
        let mut l = HomeLayout {
            version: 1,
            groups: vec![
                HomeGroup {
                    name: "A".into(),
                    items: vec![item(ItemKind::Room, "1"), item(ItemKind::Room, "2")],
                },
                HomeGroup {
                    name: "B".into(),
                    items: vec![item(ItemKind::Room, "3")],
                },
            ],
        };
        assert!(l.move_item(&item(ItemKind::Room, "2"), 1));
        assert_eq!(l.groups[1].items[0], item(ItemKind::Room, "2"));
        assert!(!l.move_item(&item(ItemKind::Room, "1"), -1));
        assert!(l.move_item(&item(ItemKind::Room, "1"), 1));
        // A emptied: dropped, so B is the only heading.
        assert_eq!(l.groups.len(), 1);
        assert_eq!(l.groups[0].name, "B");
    }

    #[test]
    fn picker_filters_by_kind_and_search() {
        let mut a = signed_in(Role::Admin);
        let mut porch = device("porch_light", "Porch Light");
        porch.device_type = Some("light".into());
        let mut timer = device("timer_laundry", "Laundry Timer");
        timer.plugin_id = "core.glue".into();
        timer.device_type = Some("timer".into());
        let mut scene = device("porch_low", "Porch Low");
        scene.device_type = Some("scene".into());
        a.devices = vec![porch, timer, scene];
        a.rebuild_scenes();
        a.areas = vec![crate::model::Area {
            id: "a1".into(),
            name: "Porch".into(),
            slug: "porch".into(),
            device_ids: vec![],
            aliases: vec![],
        }];
        a.rules = vec![
            serde_json::from_value(
                serde_json::json!({"id": "r1", "name": "Porch light at sunset"}),
            )
            .unwrap(),
        ];

        let names = |kind, q: &str| -> Vec<String> {
            candidates(&a, kind, q)
                .into_iter()
                .map(|c| c.name)
                .collect()
        };
        assert_eq!(
            names(None, "porch"),
            vec!["Porch", "Porch Light", "Porch Low", "Porch light at sunset"]
        );
        assert_eq!(names(Some(ItemKind::Device), "porch"), vec!["Porch Light"]);
        assert_eq!(names(Some(ItemKind::Helper), ""), vec!["Laundry Timer"]);
        assert_eq!(names(Some(ItemKind::Scene), "PORCH"), vec!["Porch Low"]);
        assert!(names(Some(ItemKind::Mode), "").is_empty());
        // Helpers and scene devices are not offered as plain devices.
        assert_eq!(names(Some(ItemKind::Device), ""), vec!["Porch Light"]);
    }

    #[test]
    fn the_picker_adds_under_the_chosen_heading() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _g = rt.enter();
        let mut a = signed_in(Role::Admin);
        a.devices = vec![device("lamp", "Lamp"), device("fan", "Fan")];
        a.open_home_picker();
        for c in "lamp".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.refresh_views();
        a.on_key(key(KeyCode::Enter));
        assert!(a.home.layout.contains(&item(ItemKind::Device, "lamp")));
        a.on_key(key(KeyCode::Enter));
        assert!(a.status.contains("already"));
        a.on_key(key(KeyCode::Esc));
        assert!(a.home.picker.is_none());
    }
}
