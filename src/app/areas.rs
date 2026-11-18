//! Areas tab: a list of rooms on the left, the highlighted room's
//! devices on the right.

use super::editors::{AreaEditor, Confirm, ConfirmAction};
use super::{App, ListSel, Section};
use crate::model::Area;
use crossterm::event::{KeyCode, KeyEvent};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AreasPane {
    #[default]
    AreasList,
    DeviceList,
}

#[derive(Debug, Default)]
pub struct AreasTab {
    pub pane: AreasPane,
    pub list_sel: ListSel,
    pub devices_sel: ListSel,
    /// Devices marked with Space in the right pane.
    pub marked: HashSet<String>,
    pub editor: Option<AreaEditor>,
}

impl App {
    pub fn selected_area(&self) -> Option<&Area> {
        match self.areas_tab.list_sel.id() {
            Some(id) => self.areas.iter().find(|a| a.id == id),
            None => self.areas.get(self.areas_tab.list_sel.index()),
        }
    }

    pub(super) fn compute_area_views(&mut self) {
        let ids: HashSet<String> = self
            .selected_area()
            .map(|a| a.device_ids.iter().cloned().collect())
            .unwrap_or_default();
        self.views.area_devices = (0..self.devices.len())
            .filter(|&i| ids.contains(&self.devices[i].device_id))
            .collect();
    }

    pub(super) fn area_device_ids(&self) -> Vec<String> {
        self.views
            .area_devices
            .iter()
            .filter_map(|&i| self.devices.get(i))
            .map(|d| d.device_id.clone())
            .collect()
    }

    pub(super) fn on_key_areas(&mut self, key: KeyEvent) {
        let area_ids: Vec<String> = self.areas.iter().map(|a| a.id.clone()).collect();
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => {
                self.areas_tab.pane = AreasPane::AreasList;
                self.areas_tab.marked.clear();
            }
            KeyCode::Char('l') | KeyCode::Right if self.selected_area().is_some() => {
                self.areas_tab.pane = AreasPane::DeviceList;
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j') => {
                let delta = if matches!(key.code, KeyCode::Up | KeyCode::Char('k')) {
                    -1
                } else {
                    1
                };
                match self.areas_tab.pane {
                    AreasPane::AreasList => {
                        self.areas_tab.list_sel.move_by(delta, &area_ids);
                        self.areas_tab.devices_sel.reset();
                        self.areas_tab.marked.clear();
                    }
                    AreasPane::DeviceList => {
                        let ids = self.area_device_ids();
                        self.areas_tab.devices_sel.move_by(delta, &ids);
                    }
                }
            }
            KeyCode::Enter => match self.areas_tab.pane {
                AreasPane::AreasList if self.selected_area().is_some() => {
                    self.areas_tab.pane = AreasPane::DeviceList;
                    self.areas_tab.devices_sel.reset();
                    self.areas_tab.marked.clear();
                }
                AreasPane::AreasList => {}
                AreasPane::DeviceList => self.open_area_editor_edit(),
            },
            KeyCode::Char('n') => {
                self.areas_tab.editor = Some(AreaEditor {
                    id: None,
                    name: String::new(),
                });
            }
            KeyCode::Char('d') => match self.areas_tab.pane {
                AreasPane::AreasList => self.ask_delete_area(),
                AreasPane::DeviceList => self.remove_marked_from_area(),
            },
            KeyCode::Char(' ') if self.areas_tab.pane == AreasPane::DeviceList => {
                if let Some(id) = self.areas_tab.devices_sel.id().map(str::to_string)
                    && !self.areas_tab.marked.remove(&id)
                {
                    self.areas_tab.marked.insert(id);
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') if !self.areas_tab.marked.is_empty() => {
                self.add_marked_to_area();
            }
            KeyCode::Char('-') if !self.areas_tab.marked.is_empty() => {
                self.remove_marked_from_area();
            }
            _ => {}
        }
    }

    fn open_area_editor_edit(&mut self) {
        let Some(area) = self.selected_area() else {
            return;
        };
        self.areas_tab.editor = Some(AreaEditor {
            id: Some(area.id.clone()),
            name: area.name.clone(),
        });
    }

    pub(super) fn save_area_editor(&mut self) {
        let Some(ed) = self.areas_tab.editor.clone() else {
            return;
        };
        let name = ed.name.trim().to_string();
        if name.is_empty() {
            self.error = Some("area name cannot be empty".to_string());
            return;
        }
        let c = self.client.clone();
        match ed.id {
            None => self.spawn_ok(
                "create area",
                async move { c.create_area(&name).await },
                |app, area: Area| {
                    app.areas_tab.editor = None;
                    app.status = format!("Created area '{}'", area.name);
                    app.areas_tab.list_sel.select_id(area.id.clone());
                    upsert_area(&mut app.areas, area);
                    app.save_cache(&[Section::Areas]);
                },
            ),
            Some(id) => self.spawn_ok(
                "rename area",
                async move { c.rename_area(&id, &name).await },
                |app, area: Area| {
                    app.areas_tab.editor = None;
                    app.status = format!("Renamed area to '{}'", area.name);
                    upsert_area(&mut app.areas, area);
                    app.save_cache(&[Section::Areas]);
                },
            ),
        }
    }

    fn ask_delete_area(&mut self) {
        let Some(area) = self.selected_area() else {
            return;
        };
        let (id, name) = (area.id.clone(), area.name.clone());
        let n = area.device_ids.len();
        self.ask(
            Confirm::new(
                "Delete area",
                format!("Delete area \"{name}\"?"),
                ConfirmAction::DeleteArea { id, name },
            )
            .with_detail(format!("{n} device(s) will have no room")),
        );
    }

    pub(super) fn delete_area(&mut self, id: String, name: String) {
        let c = self.client.clone();
        let target = id.clone();
        self.spawn_ok(
            "delete area",
            async move { c.delete_area(&target).await },
            move |app, ()| {
                app.areas.retain(|a| a.id != id);
                app.areas_tab.pane = AreasPane::AreasList;
                app.areas_tab.marked.clear();
                app.status = format!("Deleted area '{name}'");
                app.save_cache(&[Section::Areas]);
            },
        );
    }

    fn set_area_devices(&mut self, device_ids: Vec<String>, done: String) {
        let Some(area) = self.selected_area() else {
            return;
        };
        let c = self.client.clone();
        let id = area.id.clone();
        self.spawn_ok(
            "update area devices",
            async move { c.set_area_devices(&id, &device_ids).await },
            move |app, area: Area| {
                app.areas_tab.marked.clear();
                app.status = done;
                upsert_area(&mut app.areas, area);
                app.save_cache(&[Section::Areas]);
                // Rooms of the moved devices changed too.
                app.start_sync();
            },
        );
    }

    fn add_marked_to_area(&mut self) {
        let Some(area) = self.selected_area() else {
            return;
        };
        let mut ids = area.device_ids.clone();
        let mut added = 0;
        for id in &self.areas_tab.marked {
            if !ids.contains(id) {
                ids.push(id.clone());
                added += 1;
            }
        }
        self.set_area_devices(ids, format!("Added {added} device(s) to area"));
    }

    fn remove_marked_from_area(&mut self) {
        if self.areas_tab.marked.is_empty() {
            self.status = "Mark devices with Space first".to_string();
            return;
        }
        let Some(area) = self.selected_area() else {
            return;
        };
        let marked = &self.areas_tab.marked;
        let ids: Vec<String> = area
            .device_ids
            .iter()
            .filter(|d| !marked.contains(*d))
            .cloned()
            .collect();
        let n = marked.len();
        self.set_area_devices(ids, format!("Removed {n} device(s) from area"));
    }
}

pub fn upsert_area(areas: &mut Vec<Area>, area: Area) {
    match areas.iter_mut().find(|a| a.id == area.id) {
        Some(a) => *a = area,
        None => areas.push(area),
    }
}
