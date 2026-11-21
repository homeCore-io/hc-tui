//! Devices homeCore holds that no plugin manages any more (`o` on Devices),
//! from `GET /devices/orphaned`.
//!
//! Core knows how many are orphaned from each plugin (it holds more devices
//! than the plugin says it manages) but not which: the suspects are that many
//! least-recently-seen devices. A real orphan is usually months behind its
//! siblings, but not always, so nothing is marked for you and every removal
//! names what it removes.

use super::editors::{Confirm, ConfirmAction};
use super::{App, Load};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct Suspect {
    pub plugin_id: String,
    pub device_id: String,
    pub name: String,
    pub stale_secs: u64,
}

#[derive(Debug, Default)]
pub struct OrphanView {
    pub suspects: Load<Vec<Suspect>>,
    /// Per plugin: `(plugin_id, homeCore holds, the plugin reports)`.
    pub counts: Vec<(String, u64, u64)>,
    pub sel: usize,
    pub marked: BTreeSet<String>,
}

/// The suspects and per-plugin counts in a `GET /devices/orphaned` answer.
pub fn parse_orphans(v: &Value) -> (Vec<Suspect>, Vec<(String, u64, u64)>) {
    let mut suspects = Vec::new();
    let mut counts = Vec::new();
    for p in v
        .get("plugins")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let plugin_id = p
            .get("plugin_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        counts.push((
            plugin_id.clone(),
            p.get("core_holds").and_then(Value::as_u64).unwrap_or(0),
            p.get("plugin_reports").and_then(Value::as_u64).unwrap_or(0),
        ));
        for s in p
            .get("suspects")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let device_id = s
                .get("device_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            suspects.push(Suspect {
                plugin_id: plugin_id.clone(),
                name: s
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&device_id)
                    .to_string(),
                device_id,
                stale_secs: s.get("stale_secs").and_then(Value::as_u64).unwrap_or(0),
            });
        }
    }
    (suspects, counts)
}

impl App {
    /// `o` on Devices.
    pub fn open_orphans(&mut self) {
        self.devices_tab.orphans = Some(OrphanView {
            suspects: Load::Loading,
            ..Default::default()
        });
        let c = self.client.clone();
        self.spawn(
            async move { c.get_json::<Value>("/devices/orphaned").await },
            |app, res| {
                if let Some(o) = app.devices_tab.orphans.as_mut() {
                    match res {
                        Ok(v) => {
                            let (suspects, counts) = parse_orphans(&v);
                            o.suspects = Load::Ready(suspects);
                            o.counts = counts;
                        }
                        Err(e) => o.suspects = Load::Failed(e.to_string()),
                    }
                }
            },
        );
    }

    pub fn close_orphans(&mut self) {
        self.devices_tab.orphans = None;
    }

    pub fn move_orphan(&mut self, delta: isize) {
        if let Some(o) = self.devices_tab.orphans.as_mut()
            && let Some(list) = o.suspects.ready()
            && !list.is_empty()
        {
            o.sel = (o.sel as isize + delta).rem_euclid(list.len() as isize) as usize;
        }
    }

    /// Space: mark or unmark the selected suspect.
    pub fn mark_orphan(&mut self) {
        if let Some(o) = self.devices_tab.orphans.as_mut()
            && let Some(id) = o
                .suspects
                .ready()
                .and_then(|l| l.get(o.sel))
                .map(|s| s.device_id.clone())
            && !o.marked.remove(&id)
        {
            o.marked.insert(id);
        }
    }

    /// `x`: remove the marked suspects, after saying which.
    pub fn ask_remove_orphans(&mut self) {
        let Some(o) = self.devices_tab.orphans.as_ref() else {
            return;
        };
        let Some(list) = o.suspects.ready() else {
            return;
        };
        let chosen: Vec<&Suspect> = list
            .iter()
            .filter(|s| o.marked.contains(&s.device_id))
            .collect();
        if chosen.is_empty() {
            self.status = "Mark the devices to remove with Space first".into();
            return;
        }
        let names: Vec<String> = chosen.iter().map(|s| s.name.clone()).collect();
        let ids: Vec<String> = chosen.iter().map(|s| s.device_id.clone()).collect();
        self.ask(
            Confirm::new(
                "Remove these devices?",
                format!(
                    "{} will be removed from homeCore. Automations that use them are turned off.",
                    names.join(", ")
                ),
                ConfirmAction::DeleteDevices { ids },
            )
            .with_detail("They are guesses: the least recently seen. Check each is really gone."),
        );
    }

    pub(crate) fn delete_devices(&mut self, ids: Vec<String>) {
        self.devices_tab.orphans = None;
        for id in ids {
            self.delete_device(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_suspects_and_counts_come_from_each_plugin() {
        let v = json!({"plugins": [{
            "plugin_id": "plugin.hue", "core_holds": 50, "plugin_reports": 49, "orphan_count": 1,
            "suspects": [{"device_id": "hue_x", "name": "Old group", "stale_secs": 9000000}]
        }], "total_orphans": 1});
        let (s, c) = parse_orphans(&v);
        assert_eq!(c, [("plugin.hue".to_string(), 50, 49)]);
        assert_eq!(s[0].name, "Old group");
        assert_eq!(s[0].stale_secs, 9_000_000);
    }
}
