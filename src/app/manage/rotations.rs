//! Rotations: presets, playlists and favorites a device plays one at a time
//! (homeCore 0.1.130). Listed here with what plays now and next, and played
//! or skipped; they are made and changed in the app.

use crate::app::{App, ListSel};
use crate::model::Rotation;

#[derive(Debug, Default)]
pub struct RotationsPanel {
    pub list: Vec<Rotation>,
    pub loading: bool,
    pub error: Option<String>,
    pub sel: ListSel,
}

impl App {
    /// The rotations that match the `/` search, by name or device.
    pub fn visible_rotations(&self) -> Vec<&Rotation> {
        let q = self
            .query(crate::screens::ScreenId::Rotations)
            .to_lowercase();
        self.manage
            .rotations
            .list
            .iter()
            .filter(|r| {
                q.is_empty()
                    || r.name.to_lowercase().contains(&q)
                    || self.rotation_device(r).to_lowercase().contains(&q)
            })
            .collect()
    }

    /// The name of the device a rotation plays on.
    pub fn rotation_device(&self, r: &Rotation) -> String {
        self.devices
            .iter()
            .find(|d| d.device_id == r.device_id)
            .map(|d| d.display_name().to_string())
            .unwrap_or_else(|| r.device_id.clone())
    }

    pub fn selected_rotation(&self) -> Option<&Rotation> {
        let ids: Vec<&str> = self
            .visible_rotations()
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        let id = self.manage.rotations.sel.id().map(str::to_string);
        let i = match id {
            Some(id) => ids.iter().position(|x| *x == id)?,
            None => self.manage.rotations.sel.index(),
        };
        self.visible_rotations().get(i).copied()
    }

    pub fn move_rotation(&mut self, delta: isize) {
        let ids: Vec<String> = self
            .visible_rotations()
            .iter()
            .map(|r| r.id.clone())
            .collect();
        self.manage.rotations.sel.move_by(delta, &ids);
    }

    pub(crate) fn refresh_rotations(&mut self) {
        let p = &mut self.manage.rotations;
        p.loading = true;
        p.error = None;
        let c = self.client.clone();
        self.spawn(async move { c.list_rotations().await }, move |app, res| {
            let p = &mut app.manage.rotations;
            p.loading = false;
            match res {
                Ok(list) => {
                    p.list = list;
                    let ids: Vec<String> = p.list.iter().map(|r| r.id.clone()).collect();
                    p.sel.resolve(&ids);
                }
                Err(e) => {
                    p.error = Some(e.to_string());
                    p.list.clear();
                }
            }
        });
    }

    /// Play the selected rotation's choice for now.
    pub fn play_selected_rotation(&mut self) {
        self.rotation_call(false);
    }

    /// Move the selected rotation on without playing.
    pub fn skip_selected_rotation(&mut self) {
        self.rotation_call(true);
    }

    fn rotation_call(&mut self, skip: bool) {
        let Some(r) = self.selected_rotation() else {
            return;
        };
        let (id, name) = (r.id.clone(), r.name.clone());
        let c = self.client.clone();
        self.spawn(
            async move {
                if skip {
                    c.advance_rotation(&id).await
                } else {
                    c.play_rotation(&id).await
                }
            },
            move |app, res| {
                app.status = match res {
                    Ok(v) if skip => format!(
                        "{name}: next is {}",
                        v["current_name"].as_str().unwrap_or("the next one")
                    ),
                    Ok(v) => format!(
                        "{name}: playing {}",
                        v["played"].as_str().unwrap_or("the next one")
                    ),
                    Err(e) => format!("{name}: {e}"),
                };
                app.refresh_rotations();
            },
        );
    }
}
