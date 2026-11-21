//! The device history explorer (`h` on Devices): any reading of the
//! selected device over a range, from `GET /devices/{id}/history`.
//!
//! The Devices pane keeps its own 24-hour line of the main reading; this is
//! for looking further: another attribute, or a week or a month.

use super::{App, Load};
use crate::model::DeviceState;
use crate::model_devices::HistoryPoint;
use serde_json::Value;

/// The ranges `←/→` steps through: a label and how many hours back.
pub const RANGES: [(&str, i64); 5] = [
    ("1 hour", 1),
    ("6 hours", 6),
    ("24 hours", 24),
    ("7 days", 24 * 7),
    ("30 days", 24 * 30),
];
/// The range it opens on.
const DEFAULT_RANGE: usize = 2;
/// Points asked for: about a screen's width; core thins to it.
const POINTS: usize = 240;

#[derive(Debug)]
pub struct HistoryView {
    pub device_id: String,
    pub name: String,
    pub attrs: Vec<String>,
    pub attr_sel: usize,
    pub range: usize,
    /// Newest first, as core sends them.
    pub points: Load<Vec<HistoryPoint>>,
}

impl HistoryView {
    pub fn attr(&self) -> Option<&str> {
        self.attrs.get(self.attr_sel).map(String::as_str)
    }
}

/// The attributes worth a history: numbers and on/off, the device's main
/// reading first, then the rest by name.
pub fn history_attrs(d: &DeviceState, primary: Option<&str>) -> Vec<String> {
    let mut v: Vec<String> = d
        .attributes
        .iter()
        .filter(|(_, val)| val.is_number() || val.is_boolean())
        .map(|(k, _)| k.clone())
        .collect();
    v.sort();
    if let Some(p) = primary
        && let Some(i) = v.iter().position(|k| k == p)
    {
        let p = v.remove(i);
        v.insert(0, p);
    }
    v
}

/// Lowest, highest and latest of the numbers, and how many there were.
pub fn summary(points: &[HistoryPoint]) -> Option<(f64, f64, f64, usize)> {
    let nums: Vec<f64> = points.iter().filter_map(|p| p.value.as_f64()).collect();
    let latest = *nums.first()?;
    let lo = nums.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = nums.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Some((lo, hi, latest, nums.len()))
}

/// Each time the value changed, newest first: when, and what it became.
/// For on/off and words, where a line of bars says little.
pub fn changes(points: &[HistoryPoint]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    // Oldest first, so a change is a value that differs from the one before.
    let mut prev: Option<&Value> = None;
    for p in points.iter().rev() {
        if prev != Some(&p.value) {
            out.push((p.recorded_at.clone(), value_text(&p.value)));
        }
        prev = Some(&p.value);
    }
    out.reverse();
    out
}

fn value_text(v: &Value) -> String {
    match v {
        Value::Bool(true) => "on".into(),
        Value::Bool(false) => "off".into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

impl App {
    /// `h` on a device: open the explorer on its main reading, 24 hours.
    pub fn open_history_view(&mut self) {
        let Some(d) = self.selected_device() else {
            return;
        };
        let primary = super::controls::history_attribute(d, self.device_schema(&d.device_id));
        let attrs = history_attrs(d, primary.as_deref());
        let (device_id, name) = (d.device_id.clone(), d.display_name().to_string());
        if attrs.is_empty() {
            self.status = format!("{name} has no readings to show");
            return;
        }
        self.devices_tab.history_view = Some(HistoryView {
            device_id,
            name,
            attrs,
            attr_sel: 0,
            range: DEFAULT_RANGE,
            points: Load::Loading,
        });
        self.load_history_view();
    }

    pub fn close_history_view(&mut self) {
        self.devices_tab.history_view = None;
    }

    pub fn move_history_attr(&mut self, delta: isize) {
        if let Some(h) = self.devices_tab.history_view.as_mut() {
            let n = h.attrs.len() as isize;
            h.attr_sel = (h.attr_sel as isize + delta).rem_euclid(n) as usize;
        }
        self.load_history_view();
    }

    pub fn step_history_range(&mut self, delta: isize) {
        if let Some(h) = self.devices_tab.history_view.as_mut() {
            h.range = (h.range as isize + delta).clamp(0, RANGES.len() as isize - 1) as usize;
        }
        self.load_history_view();
    }

    pub fn load_history_view(&mut self) {
        let Some(h) = self.devices_tab.history_view.as_mut() else {
            return;
        };
        let Some(attr) = h.attr().map(str::to_string) else {
            return;
        };
        h.points = Load::Loading;
        let (id, range) = (h.device_id.clone(), h.range);
        let from = (chrono::Utc::now() - chrono::Duration::hours(RANGES[range].1))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let c = self.client.clone();
        let (want_id, want_attr) = (id.clone(), attr.clone());
        self.spawn(
            async move { c.device_history(&id, &attr, &from, POINTS).await },
            move |app, res| {
                // Only if it still shows what was asked for.
                if let Some(h) = app.devices_tab.history_view.as_mut()
                    && h.device_id == want_id
                    && h.attr() == Some(want_attr.as_str())
                    && h.range == range
                {
                    h.points = match res {
                        Ok(p) => Load::Ready(p),
                        Err(e) => Load::Failed(e.to_string()),
                    };
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pt(at: &str, v: Value) -> HistoryPoint {
        serde_json::from_value(json!({"attribute": "x", "value": v, "recorded_at": at})).unwrap()
    }

    #[test]
    fn the_main_reading_comes_first_then_the_rest_by_name() {
        let d: DeviceState = serde_json::from_value(json!({
            "device_id": "t", "name": "T", "plugin_id": "p",
            "attributes": {"temperature": 21.5, "humidity": 40, "battery_pct": 80,
                           "label": "x", "low": false}
        }))
        .unwrap();
        assert_eq!(
            history_attrs(&d, Some("temperature")),
            ["temperature", "battery_pct", "humidity", "low"]
        );
    }

    #[test]
    fn a_summary_is_the_lowest_highest_and_newest() {
        // Newest first, as core sends them.
        let p = [
            pt("3", json!(21.0)),
            pt("2", json!(25.5)),
            pt("1", json!(18.0)),
        ];
        assert_eq!(summary(&p), Some((18.0, 25.5, 21.0, 3)));
        assert_eq!(summary(&[pt("1", json!(true))]), None, "no numbers");
    }

    #[test]
    fn changes_keep_only_the_moments_it_changed_newest_first() {
        // On at t2 (t3 only repeats it), off again at t4.
        let p = [
            pt("t4", json!(false)),
            pt("t3", json!(true)),
            pt("t2", json!(true)),
            pt("t1", json!(false)),
        ];
        assert_eq!(
            changes(&p),
            [
                ("t4".to_string(), "off".to_string()),
                ("t2".to_string(), "on".to_string()),
                ("t1".to_string(), "off".to_string()),
            ]
        );
    }
}
