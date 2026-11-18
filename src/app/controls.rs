//! Device controls in the Devices detail pane, the category sub-tabs,
//! the history sparkline and removed devices.
//!
//! What a device can be set to comes from core's schema
//! (`GET /devices/{id}/schema`): every writable attribute that is not a
//! diagnostic becomes a control, and actions that take no parameter
//! become buttons. Devices without a schema fall back to the shared
//! vocabulary (`on`, `brightness_pct`, `locked`, ...). Nothing here
//! knows about any one plugin.
//!
//! Temperatures travel in °C (core converts readings at ingest and never
//! converts commands); they are shown and stepped in the house's unit
//! and converted back before sending.

use super::{App, Load};
use crate::model::{DeviceState, TempUnit};
use crate::model_devices::{
    AttributeCategory, AttributeKind, AttributeOption, AttributeSchema, DeviceSchema, RemovedDevice,
};
use crate::text::normalize_label;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long after the last key press a change is sent.
pub const SEND_DELAY: Duration = Duration::from_millis(600);
/// How long a history line is kept before it is fetched again.
const HISTORY_TTL: Duration = Duration::from_secs(300);
/// About how many points core sends back for the sparkline.
const HISTORY_POINTS: usize = 60;

// ── Classification ──────────────────────────────────────────────────────────

/// A device's kind, as the app reads it: a person's hint first, then
/// the device type.
pub fn kind_of(d: &DeviceState) -> &str {
    if let Some(h) = d.ui_hint.as_deref().filter(|h| !h.is_empty()) {
        return h;
    }
    match d.device_type() {
        "virtual_switch" => "switch",
        t => t,
    }
}

const LEFT_OUT: &[&str] = &["gateway", "bridge", "pico_remote", "remote", "mode"];
const SWITCH_KINDS: &[&str] = &["switch", "outlet", "plug", "virtual_switch"];
const MEDIA_KINDS: &[&str] = &["media_player", "tv", "speaker", "receiver"];
const CLIMATE_KINDS: &[&str] = &[
    "thermostat",
    "fan",
    "humidifier",
    "dehumidifier",
    "hvac",
    "climate",
    "air_purifier",
    "heater",
];
const COVER_KINDS: &[&str] = &["cover", "garage", "garage_door", "gate", "shade", "blind"];
const SENSOR_KINDS: &[&str] = &[
    "temperature_sensor",
    "humidity_sensor",
    "motion_sensor",
    "occupancy_sensor",
    "vibration_sensor",
    "illuminance_sensor",
    "air_quality_sensor",
    "smoke_sensor",
    "co_sensor",
    "weather_station",
    "weather",
    "rain_sensor",
    "lightning_sensor",
    "sensor",
    "binary_sensor",
    "energy_meter",
    "power_meter",
    "contact_sensor",
    "door",
    "window",
    "water_sensor",
    "leak_sensor",
];

/// A switch whose name says it runs a light: "Porch Light".
fn light_named(name: &str) -> bool {
    let n = name.to_lowercase();
    [
        "light",
        "lamp",
        "chandelier",
        "sconce",
        "pendant",
        "bulb",
        "overhead",
    ]
    .iter()
    .any(|w| n.contains(w))
}

/// The Devices sub-tab a device belongs to, besides All and Offline.
/// `None` for kinds that only show under All (switches, buttons,
/// cameras, bridges).
pub fn category_of(d: &DeviceState) -> Option<super::DeviceSubPanel> {
    use super::DeviceSubPanel as C;
    let k = kind_of(d);
    if super::media::is_media_player(d) || MEDIA_KINDS.contains(&k) {
        return Some(C::MediaPlayers);
    }
    if LEFT_OUT.contains(&k) || d.is_scene() {
        return None;
    }
    let helper = super::home::is_helper(d);
    if matches!(k, "light" | "dimmer") {
        return Some(C::Lights);
    }
    if k == "group" {
        return d
            .attributes
            .get("brightness_pct")
            .is_some_and(Value::is_number)
            .then_some(C::Lights);
    }
    if SWITCH_KINDS.contains(&k) {
        if helper {
            return None;
        }
        let name = d.display_name().to_lowercase();
        if ["humidifier", "dehumidifier", "heater"]
            .iter()
            .any(|w| name.contains(w))
        {
            return Some(C::Climate);
        }
        return light_named(&name).then_some(C::Lights);
    }
    if k == "lock" {
        return Some(C::Locks);
    }
    if COVER_KINDS.contains(&k) {
        return Some(C::Covers);
    }
    if CLIMATE_KINDS.contains(&k) {
        return Some(C::Climate);
    }
    if SENSOR_KINDS.contains(&k) {
        return Some(C::Sensors);
    }
    None
}

// ── Temperatures ────────────────────────────────────────────────────────────

/// The temperature unit a schema unit names, if it names one.
pub fn temp_unit_of(unit: &str) -> Option<TempUnit> {
    match unit
        .trim()
        .trim_start_matches('°')
        .to_ascii_lowercase()
        .as_str()
    {
        "c" | "celsius" => Some(TempUnit::Celsius),
        "f" | "fahrenheit" => Some(TempUnit::Fahrenheit),
        _ => None,
    }
}

/// `v` in unit `from`, converted to `to`.
pub fn convert_temp(v: f64, from: TempUnit, to: TempUnit) -> f64 {
    match (from, to) {
        (TempUnit::Celsius, TempUnit::Fahrenheit) => v * 9.0 / 5.0 + 32.0,
        (TempUnit::Fahrenheit, TempUnit::Celsius) => (v - 32.0) * 5.0 / 9.0,
        _ => v,
    }
}

/// One key press in the shown unit: 1°F or 0.5°C.
pub fn temp_step(shown: TempUnit) -> f64 {
    match shown {
        TempUnit::Fahrenheit => 1.0,
        TempUnit::Celsius => 0.5,
    }
}

/// A temperature setting moved one step in the house's unit. `current`,
/// `min`, `max` and the result are in the device's unit (°C on the
/// bus), rounded to hundredths as the app sends them.
pub fn nudge_temp(
    current: f64,
    device: TempUnit,
    house: TempUnit,
    dir: i32,
    min: f64,
    max: f64,
) -> f64 {
    let step = temp_step(house);
    let shown = convert_temp(current, device, house);
    let next = ((shown / step).round() + f64::from(dir)) * step;
    let back = convert_temp(next, house, device).clamp(min, max);
    (back * 100.0).round() / 100.0
}

/// A temperature in the device's unit as the house reads it: `68°F`,
/// `20.5°C`.
pub fn temp_text(v: f64, device: TempUnit, house: TempUnit) -> String {
    let shown = convert_temp(v, device, house);
    match house {
        TempUnit::Fahrenheit => format!("{shown:.0}°F"),
        TempUnit::Celsius => format!("{shown:.1}°C"),
    }
}

// ── Controls ────────────────────────────────────────────────────────────────

/// Color presets, as the app offers them: name and CIE xy.
pub const COLOR_PRESETS: &[(&str, f64, f64, [u8; 3])] = &[
    ("warm white", 0.4596, 0.4105, [255, 217, 160]),
    ("soft white", 0.4148, 0.3959, [255, 241, 222]),
    ("daylight", 0.3127, 0.329, [228, 238, 255]),
    ("red", 0.675, 0.322, [229, 72, 77]),
    ("orange", 0.5562, 0.4084, [245, 144, 61]),
    ("green", 0.2151, 0.7106, [63, 174, 90]),
    ("blue", 0.167, 0.04, [62, 123, 250]),
    ("purple", 0.2725, 0.1096, [142, 78, 198]),
];

#[derive(Debug, Clone, PartialEq)]
pub struct ActionButton {
    pub id: String,
    pub label: String,
    pub confirm: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ControlKind {
    /// A true/false attribute, with its words.
    Toggle { on: String, off: String },
    /// A number between `min` and `max`. `temp` is the device's unit
    /// when it is a temperature.
    Range {
        min: f64,
        max: f64,
        step: f64,
        unit: Option<String>,
        temp: Option<TempUnit>,
    },
    /// One of a list. `numeric` sends the value as a number.
    Choice {
        options: Vec<AttributeOption>,
        numeric: bool,
    },
    /// Color presets; `rgb` for a `color_rgb` attribute, else CIE xy.
    Colors { rgb: bool },
    /// The device's one-key actions.
    Actions(Vec<ActionButton>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    /// The attribute it sets; empty for the actions row.
    pub attr: String,
    pub label: String,
    pub kind: ControlKind,
}

fn device_temp_unit(d: &DeviceState, attr: &str, spec_unit: Option<&str>) -> Option<TempUnit> {
    // A sibling `<attr>_unit` wins over the schema, as in the app.
    let sibling = d
        .attributes
        .get(&format!("{attr}_unit"))
        .and_then(Value::as_str);
    sibling.or(spec_unit).and_then(temp_unit_of)
}

fn control_from_schema(d: &DeviceState, attr: &str, spec: &AttributeSchema) -> Option<Control> {
    let label = spec
        .display_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| normalize_label(attr));
    let kind = match spec.kind {
        AttributeKind::Bool => {
            let (on, off) = match &spec.states {
                Some(s) if !s.when_true.label.is_empty() && !s.when_false.label.is_empty() => {
                    (s.when_true.label.clone(), s.when_false.label.clone())
                }
                _ => ("on".to_string(), "off".to_string()),
            };
            ControlKind::Toggle { on, off }
        }
        AttributeKind::Integer | AttributeKind::Float | AttributeKind::ColorTemp => {
            if let Some(opts) = spec.options.as_ref().filter(|o| !o.is_empty()) {
                ControlKind::Choice {
                    options: opts.clone(),
                    numeric: true,
                }
            } else {
                let (min, max) = (spec.min?, spec.max?);
                let step = spec.step.unwrap_or(match spec.kind {
                    AttributeKind::ColorTemp => 50.0,
                    AttributeKind::Integer => 1.0,
                    _ => 0.1,
                });
                let temp = device_temp_unit(d, attr, spec.unit.as_deref());
                ControlKind::Range {
                    min,
                    max,
                    step,
                    unit: spec.unit.clone(),
                    temp,
                }
            }
        }
        AttributeKind::Enum => ControlKind::Choice {
            options: spec.options.clone().filter(|o| !o.is_empty())?,
            numeric: false,
        },
        AttributeKind::String => {
            let list = d
                .attributes
                .get(&format!("available_{attr}s"))
                .and_then(Value::as_array)?;
            let options: Vec<AttributeOption> = list
                .iter()
                .filter_map(Value::as_str)
                .map(|s| AttributeOption {
                    value: s.to_string(),
                    label: None,
                })
                .collect();
            if options.is_empty() {
                return None;
            }
            ControlKind::Choice {
                options,
                numeric: false,
            }
        }
        AttributeKind::ColorXy => ControlKind::Colors { rgb: false },
        AttributeKind::ColorRgb => ControlKind::Colors { rgb: true },
        AttributeKind::Json => return None,
    };
    Some(Control {
        attr: attr.to_string(),
        label,
        kind,
    })
}

/// Controls from the shared vocabulary, for a device without a schema.
fn vocabulary_controls(d: &DeviceState) -> Vec<Control> {
    let has = |k: &str| d.attributes.contains_key(k);
    let num = |k: &str| d.attributes.get(k).and_then(Value::as_f64);
    let range = |attr: &str, label: &str, min: f64, max: f64, unit: &str| Control {
        attr: attr.into(),
        label: label.into(),
        kind: ControlKind::Range {
            min,
            max,
            step: 1.0,
            unit: Some(unit.into()),
            temp: temp_unit_of(unit),
        },
    };
    let mut v = Vec::new();
    if d.attributes.get("on").is_some_and(Value::is_boolean) {
        v.push(Control {
            attr: "on".into(),
            label: "Power".into(),
            kind: ControlKind::Toggle {
                on: "on".into(),
                off: "off".into(),
            },
        });
    }
    if num("brightness_pct").is_some() {
        v.push(range("brightness_pct", "Brightness", 0.0, 100.0, "%"));
    }
    if let (Some(_), Some(lo), Some(hi)) = (
        num("color_temp"),
        num("color_temp_min"),
        num("color_temp_max"),
    ) {
        let mut c = range("color_temp", "Warmth", lo, hi, "K");
        if let ControlKind::Range { step, .. } = &mut c.kind {
            *step = 50.0;
        }
        v.push(c);
    }
    if has("color_xy") {
        v.push(Control {
            attr: "color_xy".into(),
            label: "Color".into(),
            kind: ControlKind::Colors { rgb: false },
        });
    }
    if num("position").is_some() {
        v.push(range("position", "Position", 0.0, 100.0, "%"));
    }
    if num("speed_pct").is_some() {
        v.push(range("speed_pct", "Speed", 0.0, 100.0, "%"));
    }
    if d.attributes.get("locked").is_some_and(Value::is_boolean) {
        v.push(Control {
            attr: "locked".into(),
            label: "Lock".into(),
            kind: ControlKind::Toggle {
                on: "locked".into(),
                off: "unlocked".into(),
            },
        });
    }
    for (attr, label) in [
        ("setpoint", "Setpoint"),
        ("heat_setpoint", "Heat to"),
        ("cool_setpoint", "Cool to"),
    ] {
        if num(attr).is_some() {
            v.push(range(attr, label, 5.0, 35.0, "°C"));
        }
    }
    v
}

/// The controls for `d`, in order: the headline settings, the one-key
/// actions, then setup settings. `admin` shows actions only admins may
/// run.
pub fn controls_for(d: &DeviceState, schema: Option<&DeviceSchema>, admin: bool) -> Vec<Control> {
    let Some(schema) = schema else {
        return vocabulary_controls(d);
    };
    let rank = |name: &str| {
        schema
            .primary
            .iter()
            .position(|p| p == name)
            .unwrap_or(usize::MAX)
    };
    let mut names: Vec<(&String, &AttributeSchema)> = schema
        .attributes
        .iter()
        .filter(|(_, s)| s.writable && s.category != Some(AttributeCategory::Diagnostic))
        .collect();
    names.sort_by(|(a, sa), (b, sb)| {
        let setup = |s: &AttributeSchema| s.category == Some(AttributeCategory::Config);
        (setup(sa), rank(a), *a).cmp(&(setup(sb), rank(b), *b))
    });
    let mut main = Vec::new();
    let mut setup = Vec::new();
    for (name, spec) in names {
        if let Some(c) = control_from_schema(d, name, spec) {
            if spec.category == Some(AttributeCategory::Config) {
                setup.push(c);
            } else {
                main.push(c);
            }
        }
    }
    let buttons: Vec<ActionButton> = schema
        .actions
        .iter()
        .filter(|a| a.one_key())
        .filter(|a| admin || a.requires_role.as_deref() != Some("admin"))
        .map(|a| ActionButton {
            id: a.id.clone(),
            label: if a.label.is_empty() {
                normalize_label(&a.id)
            } else {
                a.label.clone()
            },
            confirm: a.confirm.clone(),
        })
        .collect();
    if !buttons.is_empty() {
        main.push(Control {
            attr: String::new(),
            label: "Actions".into(),
            kind: ControlKind::Actions(buttons),
        });
    }
    main.extend(setup);
    main
}

/// The color preset nearest to the device's color now.
pub fn nearest_preset(v: Option<&Value>, rgb: bool) -> Option<usize> {
    let v = v?;
    let dist = |i: usize| -> f64 {
        let (_, x, y, c) = COLOR_PRESETS[i];
        if rgb {
            let get = |k: &str, n: usize| {
                v.get(k)
                    .or_else(|| v.get(n))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
            };
            let (r, g, b) = (get("r", 0), get("g", 1), get("b", 2));
            (r - f64::from(c[0])).powi(2)
                + (g - f64::from(c[1])).powi(2)
                + (b - f64::from(c[2])).powi(2)
        } else {
            let px = v.get("x").and_then(Value::as_f64).unwrap_or(0.0);
            let py = v.get("y").and_then(Value::as_f64).unwrap_or(0.0);
            (px - x).powi(2) + (py - y).powi(2)
        }
    };
    (0..COLOR_PRESETS.len()).min_by(|&a, &b| dist(a).total_cmp(&dist(b)))
}

fn preset_value(i: usize, rgb: bool, current: Option<&Value>) -> Value {
    let (_, x, y, c) = COLOR_PRESETS[i];
    if !rgb {
        return json!({ "x": x, "y": y });
    }
    // Send the shape the device reports.
    if current.is_some_and(Value::is_array) {
        json!([c[0], c[1], c[2]])
    } else {
        json!({ "r": c[0], "g": c[1], "b": c[2] })
    }
}

/// The key step of a range: 5 for a percentage, 100 K for color
/// temperature, else the schema's step.
fn key_step(c: &Control) -> f64 {
    let ControlKind::Range { step, unit, .. } = &c.kind else {
        return 1.0;
    };
    match unit.as_deref() {
        Some("%") => step.max(5.0),
        Some("K") => step.max(100.0),
        _ => *step,
    }
}

fn round_to(v: f64, step: f64) -> f64 {
    if step <= 0.0 {
        return v;
    }
    let r = (v / step).round() * step;
    (r * 1000.0).round() / 1000.0
}

/// What one press of ←/→ (`dir` -1 or 1) on control `c` sends, and the
/// words for the status line. `None` when nothing changes.
pub fn change_body(
    d: &DeviceState,
    c: &Control,
    dir: i32,
    house: TempUnit,
) -> Option<(Map<String, Value>, String)> {
    let cur = d.attributes.get(&c.attr);
    let mut body = Map::new();
    let words = match &c.kind {
        ControlKind::Toggle { on, off } => {
            let now = cur.and_then(Value::as_bool).unwrap_or(false);
            let want = dir > 0;
            if now == want {
                return None;
            }
            body.insert(c.attr.clone(), Value::Bool(want));
            if want { on.clone() } else { off.clone() }
        }
        ControlKind::Range {
            min,
            max,
            step,
            unit,
            temp,
        } => {
            let now = cur.and_then(Value::as_f64).unwrap_or(*min);
            let next = match temp {
                Some(dev) => nudge_temp(now, *dev, house, dir, *min, *max),
                None => {
                    let ks = key_step(c);
                    round_to((now + f64::from(dir) * ks).clamp(*min, *max), *step)
                }
            };
            if (next - now).abs() < 1e-9 {
                return None;
            }
            let has_on = d.attributes.get("on").is_some_and(Value::is_boolean);
            if c.attr == "brightness_pct" && has_on && next <= 0.0 {
                body.insert("on".into(), Value::Bool(false));
            } else {
                body.insert(c.attr.clone(), number(next));
                if c.attr == "brightness_pct" && has_on {
                    body.insert("on".into(), Value::Bool(true));
                }
            }
            match temp {
                Some(dev) => temp_text(next, *dev, house),
                None => value_with_unit(next, unit.as_deref()),
            }
        }
        ControlKind::Choice { options, numeric } => {
            let now = cur.map(value_text).unwrap_or_default();
            let n = options.len() as i64;
            let i = options.iter().position(|o| o.value == now);
            let next = match i {
                Some(i) => (i as i64 + i64::from(dir)).clamp(0, n - 1) as usize,
                None => 0,
            };
            if Some(next) == i {
                return None;
            }
            let o = &options[next];
            let v = if *numeric {
                o.value
                    .parse::<f64>()
                    .map(number)
                    .unwrap_or_else(|_| Value::String(o.value.clone()))
            } else {
                Value::String(o.value.clone())
            };
            body.insert(c.attr.clone(), v);
            o.label().to_string()
        }
        ControlKind::Colors { rgb } => {
            let n = COLOR_PRESETS.len() as i64;
            let next = match nearest_preset(cur, *rgb) {
                Some(i) => (i as i64 + i64::from(dir)).rem_euclid(n) as usize,
                None => 0,
            };
            body.insert(c.attr.clone(), preset_value(next, *rgb, cur));
            if d.attributes.get("on").is_some_and(Value::is_boolean) {
                body.insert("on".into(), Value::Bool(true));
            }
            COLOR_PRESETS[next].0.to_string()
        }
        ControlKind::Actions(_) => return None,
    };
    Some((body, words))
}

/// A whole number as an integer, else as it is.
fn number(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        json!(v as i64)
    } else {
        json!(v)
    }
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn value_with_unit(v: f64, unit: Option<&str>) -> String {
    let n = if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    };
    match unit {
        Some("%") => format!("{n}%"),
        Some(u) if !u.is_empty() => format!("{n} {u}"),
        _ => n,
    }
}

/// The control's value now, as the pane shows it.
pub fn control_value(d: &DeviceState, c: &Control, house: TempUnit) -> String {
    let cur = d.attributes.get(&c.attr);
    match &c.kind {
        ControlKind::Toggle { on, off } => match cur.and_then(Value::as_bool) {
            Some(true) => on.clone(),
            Some(false) => off.clone(),
            None => "-".into(),
        },
        ControlKind::Range { unit, temp, .. } => match (cur.and_then(Value::as_f64), temp) {
            (Some(v), Some(dev)) => temp_text(v, *dev, house),
            (Some(v), None) => value_with_unit(v, unit.as_deref()),
            (None, _) => "-".into(),
        },
        ControlKind::Choice { options, .. } => {
            let now = cur.map(value_text).unwrap_or_default();
            options
                .iter()
                .find(|o| o.value == now)
                .map(|o| o.label().to_string())
                .unwrap_or_else(|| if now.is_empty() { "-".into() } else { now })
        }
        ControlKind::Colors { rgb } => nearest_preset(cur, *rgb)
            .map(|i| COLOR_PRESETS[i].0.to_string())
            .unwrap_or_else(|| "-".into()),
        ControlKind::Actions(_) => String::new(),
    }
}

// ── History ─────────────────────────────────────────────────────────────────

/// The attribute the sparkline follows: brightness for a light,
/// temperature for a sensor, power for a switch, else the first
/// numeric headline attribute. Media players get none.
pub fn history_attribute(d: &DeviceState, schema: Option<&DeviceSchema>) -> Option<String> {
    if category_of(d) == Some(super::DeviceSubPanel::MediaPlayers) {
        return None;
    }
    let num = |k: &str| d.attributes.get(k).is_some_and(Value::is_number);
    let k = kind_of(d);
    let preferred = match k {
        "light" | "dimmer" => Some("brightness_pct"),
        "temperature_sensor" | "weather_station" | "weather" | "thermostat" => Some("temperature"),
        "switch" | "outlet" | "plug" => Some("power_w"),
        _ => None,
    };
    if let Some(p) = preferred.filter(|p| num(p)) {
        return Some(p.to_string());
    }
    if let Some(s) = schema {
        for p in &s.primary {
            let Some(spec) = s.attributes.get(p) else {
                continue;
            };
            if spec.category == Some(AttributeCategory::Diagnostic) {
                continue;
            }
            match spec.kind {
                AttributeKind::Integer | AttributeKind::Float if num(p) => {
                    return Some(p.clone());
                }
                AttributeKind::Bool if p != "on" && d.attributes.contains_key(p) => {
                    return Some(p.clone());
                }
                _ => {}
            }
        }
    }
    [
        "temperature",
        "humidity",
        "brightness_pct",
        "power_w",
        "setpoint",
    ]
    .iter()
    .find(|k| num(k))
    .map(|k| k.to_string())
}

/// History values, oldest first: numbers as they are, true and false as
/// 1 and 0.
pub fn history_values(points: &[crate::model_devices::HistoryPoint]) -> Vec<f64> {
    let mut v: Vec<f64> = points
        .iter()
        .filter_map(|p| match &p.value {
            Value::Number(n) => n.as_f64(),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        })
        .collect();
    v.reverse();
    v
}

/// `values` as at most `width` block characters, each the mean of its
/// share of the values.
pub fn sparkline(values: &[f64], width: usize) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if values.is_empty() || width == 0 {
        return String::new();
    }
    let n = width.min(values.len());
    let buckets: Vec<f64> = (0..n)
        .map(|i| {
            let a = i * values.len() / n;
            let b = ((i + 1) * values.len() / n).max(a + 1);
            values[a..b].iter().sum::<f64>() / (b - a) as f64
        })
        .collect();
    let lo = buckets.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = buckets.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    buckets
        .iter()
        .map(|v| {
            if hi - lo < 1e-9 {
                BARS[3]
            } else {
                BARS[(((v - lo) / (hi - lo)) * 7.0).round() as usize]
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct History {
    pub attr: String,
    pub at: Instant,
    pub values: Load<Vec<f64>>,
}

// ── State ───────────────────────────────────────────────────────────────────

/// A change waiting for the keys to stop.
#[derive(Debug, Clone)]
pub struct PendingSend {
    pub device_id: String,
    pub body: Map<String, Value>,
    pub due: Instant,
    pub words: String,
}

/// The Devices screen's controls, caches and removed list.
#[derive(Debug, Default)]
pub struct ControlsState {
    /// The control Tab moved to, when one has focus.
    pub focus: Option<usize>,
    /// The highlighted button on an actions row.
    pub button: usize,
    pub schemas: HashMap<String, Load<Option<DeviceSchema>>>,
    pub history: HashMap<String, History>,
    pub pending: Option<PendingSend>,
    pub removed: Load<Vec<RemovedDevice>>,
    pub removed_sel: super::ListSel,
    pub picker: Option<super::picker::DevicePicker>,
}

impl App {
    /// The schema of `id` when it has loaded.
    pub fn device_schema(&self, id: &str) -> Option<&DeviceSchema> {
        match self.devices_tab.controls.schemas.get(id) {
            Some(Load::Ready(Some(s))) => Some(s),
            _ => None,
        }
    }

    /// The controls of `d`, once its schema has been asked for.
    pub fn device_controls(&self, d: &DeviceState) -> Vec<Control> {
        match self.devices_tab.controls.schemas.get(&d.device_id) {
            Some(Load::Ready(s)) => controls_for(d, s.as_ref(), self.is_admin()),
            Some(Load::Failed(_)) => controls_for(d, None, self.is_admin()),
            _ => Vec::new(),
        }
    }

    pub fn focused_control(&self) -> Option<(usize, Control)> {
        let i = self.devices_tab.controls.focus?;
        let d = self.selected_device()?;
        let mut cs = self.device_controls(d);
        if cs.is_empty() {
            return None;
        }
        let i = i.min(cs.len() - 1);
        Some((i, cs.swap_remove(i)))
    }

    /// Load the selected device's schema and history line when they are
    /// not here yet. Called once per frame while Devices is on screen.
    pub(crate) fn load_selected_device_extras(&mut self) {
        if !self.authenticated
            || self.screen != crate::screens::ScreenId::Devices
            || tokio::runtime::Handle::try_current().is_err()
        {
            return;
        }
        let Some(d) = self.selected_device() else {
            return;
        };
        let id = d.device_id.clone();
        if !self.devices_tab.controls.schemas.contains_key(&id) {
            self.fetch_schema(id.clone(), |_| {});
            return;
        }
        if self.devices_tab.controls.schemas[&id].is_loading() {
            return;
        }
        let Some(attr) = history_attribute(d, self.device_schema(&id)) else {
            return;
        };
        let fresh = self
            .devices_tab
            .controls
            .history
            .get(&id)
            .is_some_and(|h| h.attr == attr && h.at.elapsed() < HISTORY_TTL);
        if !fresh {
            self.fetch_history(id, attr);
        }
    }

    /// Ask for `id`'s schema, then run `then`.
    pub(crate) fn fetch_schema(
        &mut self,
        id: String,
        then: impl FnOnce(&mut App) + Send + 'static,
    ) {
        self.devices_tab
            .controls
            .schemas
            .insert(id.clone(), Load::Loading);
        let c = self.client.clone();
        let target = id.clone();
        self.spawn(
            async move { c.device_schema(&target).await },
            move |app, res| {
                let v = match res {
                    Ok(s) => Load::Ready(s),
                    Err(e) => Load::Failed(e.to_string()),
                };
                app.devices_tab.controls.schemas.insert(id, v);
                then(app);
            },
        );
    }

    fn fetch_history(&mut self, id: String, attr: String) {
        self.devices_tab.controls.history.insert(
            id.clone(),
            History {
                attr: attr.clone(),
                at: Instant::now(),
                values: Load::Loading,
            },
        );
        let from = (chrono::Utc::now() - chrono::Duration::hours(24))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let c = self.client.clone();
        let (target, a) = (id.clone(), attr.clone());
        self.spawn(
            async move { c.device_history(&target, &a, &from, HISTORY_POINTS).await },
            move |app, res| {
                let values = match res {
                    Ok(points) => Load::Ready(history_values(&points)),
                    Err(e) => Load::Failed(e.to_string()),
                };
                app.devices_tab.controls.history.insert(
                    id,
                    History {
                        attr,
                        at: Instant::now(),
                        values,
                    },
                );
            },
        );
    }

    // ── Keys ─────────────────────────────────────────────────────────────

    /// Tab (`dir` 1) and Shift+Tab (-1): the next or previous control.
    pub fn step_control(&mut self, dir: isize) {
        let Some(d) = self.selected_device() else {
            return;
        };
        let n = self.device_controls(d).len();
        if n == 0 {
            self.status = format!("{} has nothing to set here", d.display_name());
            return;
        }
        let st = &mut self.devices_tab.controls;
        st.button = 0;
        st.focus = Some(match st.focus {
            None if dir < 0 => n - 1,
            None => 0,
            Some(i) => ((i.min(n - 1) as isize + dir).rem_euclid(n as isize)) as usize,
        });
    }

    pub fn unfocus_control(&mut self) -> bool {
        self.devices_tab.controls.focus.take().is_some()
    }

    /// ←/→ on the focused control.
    pub fn change_control(&mut self, dir: i32) {
        let Some((_, c)) = self.focused_control() else {
            return;
        };
        if let ControlKind::Actions(buttons) = &c.kind {
            let n = buttons.len() as i64;
            let b = &mut self.devices_tab.controls.button;
            *b = (*b as i64 + i64::from(dir)).rem_euclid(n.max(1)) as usize;
            return;
        }
        let Some(d) = self.selected_device() else {
            return;
        };
        let Some((body, words)) = change_body(d, &c, dir, self.temp_unit) else {
            return;
        };
        let (id, name) = (d.device_id.clone(), d.display_name().to_string());
        if c.attr == "locked" && body.get("locked") == Some(&Value::Bool(false)) {
            self.ask(super::Confirm::new(
                "Unlock?",
                format!("{name} will be unlocked."),
                super::editors::ConfirmAction::Devices(DeviceConfirm::Send {
                    id,
                    body,
                    words: format!("{name} unlocked"),
                }),
            ));
            return;
        }
        self.queue_send(id, body, format!("{name} {}: {words}", c.label));
    }

    /// Space on the focused control: flip a switch, press a button,
    /// next choice.
    pub fn press_control(&mut self) {
        let Some((_, c)) = self.focused_control() else {
            return;
        };
        let Some(d) = self.selected_device() else {
            return;
        };
        let (id, name) = (d.device_id.clone(), d.display_name().to_string());
        match &c.kind {
            ControlKind::Actions(buttons) => {
                let i = self.devices_tab.controls.button.min(buttons.len() - 1);
                let b = buttons[i].clone();
                let body = json!({ "action": b.id }).as_object().cloned().unwrap();
                let words = format!("{name}: {}", b.label);
                match b.confirm {
                    Some(q) => self.ask(super::Confirm::new(
                        format!("{}?", b.label),
                        q,
                        super::editors::ConfirmAction::Devices(DeviceConfirm::Send {
                            id,
                            body,
                            words,
                        }),
                    )),
                    None => self.command_device(id, Value::Object(body), words),
                }
            }
            ControlKind::Toggle { .. } => {
                let now = d
                    .attributes
                    .get(&c.attr)
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                self.change_control(if now { -1 } else { 1 });
            }
            _ => self.change_control(1),
        }
    }

    /// Hold a change for [`SEND_DELAY`] so a run of key presses sends
    /// once. The pane shows the new value at once.
    pub(crate) fn queue_send(&mut self, id: String, body: Map<String, Value>, words: String) {
        if self
            .devices_tab
            .controls
            .pending
            .as_ref()
            .is_some_and(|p| p.device_id != id)
        {
            self.flush_device_controls_now();
        }
        if let Some(d) = self.devices.iter_mut().find(|d| d.device_id == id) {
            for (k, v) in &body {
                d.attributes.insert(k.clone(), v.clone());
            }
        }
        let p = self
            .devices_tab
            .controls
            .pending
            .get_or_insert_with(|| PendingSend {
                device_id: id,
                body: Map::new(),
                due: Instant::now(),
                words: String::new(),
            });
        p.body.extend(body);
        p.due = Instant::now() + SEND_DELAY;
        self.status = words.clone();
        p.words = words;
    }

    /// Send a held change once its delay is over. Called every tick.
    pub fn flush_device_controls(&mut self) {
        if self
            .devices_tab
            .controls
            .pending
            .as_ref()
            .is_some_and(|p| Instant::now() >= p.due)
        {
            self.flush_device_controls_now();
        }
    }

    fn flush_device_controls_now(&mut self) {
        if let Some(p) = self.devices_tab.controls.pending.take() {
            self.command_device(p.device_id, Value::Object(p.body), p.words);
        }
    }

    // ── Removed devices ──────────────────────────────────────────────────

    pub fn load_removed_devices(&mut self) {
        self.devices_tab.controls.removed = Load::Loading;
        let c = self.client.clone();
        self.spawn(async move { c.list_removed_devices().await }, |app, res| {
            app.devices_tab.controls.removed = match res {
                Ok(v) => Load::Ready(v),
                Err(e) => Load::Failed(e.to_string()),
            };
        });
    }

    pub fn removed_list(&self) -> &[RemovedDevice] {
        self.devices_tab
            .controls
            .removed
            .ready()
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn move_removed(&mut self, delta: isize) {
        let ids: Vec<String> = self
            .removed_list()
            .iter()
            .map(|r| r.device_id.clone())
            .collect();
        self.devices_tab.controls.removed_sel.move_by(delta, &ids);
    }

    pub fn selected_removed(&self) -> Option<&RemovedDevice> {
        let list = self.removed_list();
        let sel = &self.devices_tab.controls.removed_sel;
        sel.id()
            .and_then(|id| list.iter().find(|r| r.device_id == id))
            .or_else(|| list.get(sel.index()))
    }

    /// Space on Removed: let the device come back.
    pub fn restore_selected_device(&mut self) {
        let Some(r) = self.selected_removed().cloned() else {
            return;
        };
        let c = self.client.clone();
        let id = r.device_id.clone();
        let name = if r.name.is_empty() {
            r.device_id.clone()
        } else {
            r.name.clone()
        };
        self.spawn_ok(
            "bring back",
            async move { c.restore_device(&id).await },
            move |app, ()| {
                if let Load::Ready(v) = &mut app.devices_tab.controls.removed {
                    v.retain(|x| x.device_id != r.device_id);
                }
                app.status = format!("{name} comes back when its plugin next reports it");
            },
        );
    }
}

/// Questions the Devices, Rooms and Scenes screens ask before acting.
#[derive(Debug, Clone)]
pub enum DeviceConfirm {
    /// Send a command (an unlock, an action that asks first).
    Send {
        id: String,
        body: Map<String, Value>,
        words: String,
    },
    /// Take devices out of a room.
    LeaveRoom {
        area_id: String,
        device_ids: Vec<String>,
    },
    DeleteScene {
        id: String,
        name: String,
    },
}

impl App {
    pub(crate) fn run_device_confirm(&mut self, c: DeviceConfirm) {
        match c {
            DeviceConfirm::Send { id, body, words } => {
                self.command_device(id, Value::Object(body), words)
            }
            DeviceConfirm::LeaveRoom {
                area_id,
                device_ids,
            } => self.change_room(area_id, Vec::new(), device_ids),
            DeviceConfirm::DeleteScene { id, name } => self.delete_scene(id, name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::DeviceSubPanel as C;
    use super::super::test_support::*;
    use super::*;

    fn dev(ty: &str, attrs: Value) -> DeviceState {
        let mut d = device("d", "Thing");
        d.device_type = Some(ty.into());
        d.attributes = attrs.as_object().cloned().unwrap();
        d
    }

    fn schema(v: Value) -> DeviceSchema {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn classification_follows_kind_hint_and_name() {
        assert_eq!(category_of(&dev("light", json!({}))), Some(C::Lights));
        assert_eq!(category_of(&dev("thermostat", json!({}))), Some(C::Climate));
        assert_eq!(category_of(&dev("fan", json!({}))), Some(C::Climate));
        assert_eq!(
            category_of(&dev("media_player", json!({}))),
            Some(C::MediaPlayers)
        );
        assert_eq!(category_of(&dev("shade", json!({}))), Some(C::Covers));
        assert_eq!(category_of(&dev("lock", json!({}))), Some(C::Locks));
        assert_eq!(
            category_of(&dev("contact_sensor", json!({}))),
            Some(C::Sensors)
        );
        assert_eq!(
            category_of(&dev("water_sensor", json!({}))),
            Some(C::Sensors)
        );
        assert_eq!(category_of(&dev("pico_remote", json!({}))), None);
        assert_eq!(category_of(&dev("camera", json!({}))), None);
        // A switch is a light when its name says so, or a person says so.
        let mut s = dev("switch", json!({"on": true}));
        assert_eq!(category_of(&s), None);
        s.name = "Porch Light".into();
        assert_eq!(category_of(&s), Some(C::Lights));
        s.name = "Garage Heater".into();
        assert_eq!(category_of(&s), Some(C::Climate));
        let mut hinted = dev("switch", json!({}));
        hinted.ui_hint = Some("light".into());
        assert_eq!(category_of(&hinted), Some(C::Lights));
        // homeCore's own switches stay switches.
        let mut helper = dev("switch", json!({}));
        helper.name = "Night light mode".into();
        helper.plugin_id = "core.glue".into();
        assert_eq!(category_of(&helper), None);
        assert_eq!(
            category_of(&dev("group", json!({"brightness_pct": 3}))),
            Some(C::Lights)
        );
        assert_eq!(category_of(&dev("group", json!({"on": true}))), None);
    }

    #[test]
    fn temperatures_convert_both_ways() {
        let (c, f) = (TempUnit::Celsius, TempUnit::Fahrenheit);
        assert_eq!(convert_temp(20.0, c, f), 68.0);
        assert_eq!(convert_temp(68.0, f, c), 20.0);
        assert!((convert_temp(convert_temp(21.3, c, f), f, c) - 21.3).abs() < 1e-9);
        assert_eq!(convert_temp(7.0, c, c), 7.0);
        assert_eq!(temp_unit_of("°C"), Some(c));
        assert_eq!(temp_unit_of("F"), Some(f));
        assert_eq!(temp_unit_of("%"), None);
        assert_eq!(temp_text(20.0, c, f), "68°F");
        assert_eq!(temp_text(20.0, c, c), "20.0°C");
    }

    #[test]
    fn a_setpoint_steps_in_the_house_unit_and_sends_celsius() {
        let (c, f) = (TempUnit::Celsius, TempUnit::Fahrenheit);
        // 20 °C is 68 °F; one step up is 69 °F, sent as 20.56 °C.
        assert_eq!(nudge_temp(20.0, c, f, 1, -40.0, 45.0), 20.56);
        assert_eq!(nudge_temp(20.0, c, f, -1, -40.0, 45.0), 19.44);
        // In °C the step is half a degree.
        assert_eq!(nudge_temp(20.0, c, c, 1, -40.0, 45.0), 20.5);
        // The schema's range holds, in °C.
        assert_eq!(nudge_temp(45.0, c, f, 1, -40.0, 45.0), 45.0);
        // An odd value snaps to the grid first.
        assert_eq!(nudge_temp(20.1, c, c, 1, 5.0, 35.0), 20.5);
    }

    #[test]
    fn schema_attributes_become_controls_in_order() {
        let d = dev(
            "thermostat",
            json!({"hvac_mode": "heat", "setpoint": 7.0, "temperature": 6.7, "hysteresis": 1.0}),
        );
        let s = schema(json!({
            "attributes": {
                "temperature": {"kind": "float", "unit": "°C", "writable": false},
                "hvac_mode": {"kind": "enum", "display_name": "Mode",
                    "options": [{"label": "Heat", "value": "heat"}, {"label": "Off", "value": "off"}]},
                "setpoint": {"kind": "float", "min": -40.0, "max": 45.0, "step": 0.5, "unit": "°C",
                    "display_name": "Target"},
                "hysteresis": {"kind": "float", "min": 0.0, "max": 5.0, "category": "config"},
                "rssi": {"kind": "integer", "min": -100.0, "max": 0.0, "category": "diagnostic"},
                "blob": {"kind": "json"}
            },
            "actions": [
                {"id": "resume", "label": "Back to the usual target"},
                {"id": "hold", "label": "Hold", "params": [{"name": "setpoint", "required": true}]},
                {"id": "wipe", "label": "Wipe", "requires_role": "admin", "confirm": "Sure?"}
            ],
            "primary": ["setpoint", "temperature", "hvac_mode"]
        }));
        let cs = controls_for(&d, Some(&s), false);
        let names: Vec<&str> = cs.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(names, vec!["Target", "Mode", "Actions", "Hysteresis"]);
        match &cs[0].kind {
            ControlKind::Range { temp, min, max, .. } => {
                assert_eq!(*temp, Some(TempUnit::Celsius));
                assert_eq!((*min, *max), (-40.0, 45.0));
            }
            k => panic!("{k:?}"),
        }
        match &cs[2].kind {
            ControlKind::Actions(b) => {
                assert_eq!(b.len(), 1, "no params, no admin: {b:?}");
                assert_eq!(b[0].id, "resume");
            }
            k => panic!("{k:?}"),
        }
        let admin = controls_for(&d, Some(&s), true);
        match &admin[2].kind {
            ControlKind::Actions(b) => assert_eq!(b[1].confirm.as_deref(), Some("Sure?")),
            k => panic!("{k:?}"),
        }
        // The setpoint in °F: shown and stepped in °F, sent in °C.
        let (body, words) = change_body(&d, &cs[0], 1, TempUnit::Fahrenheit).unwrap();
        assert_eq!(
            body["setpoint"],
            json!(7.78),
            "44.6°F shows as 45°F; up is 46°F"
        );
        assert_eq!(words, "46°F");
        assert_eq!(control_value(&d, &cs[0], TempUnit::Fahrenheit), "45°F");
        // The mode moves along its options.
        let (body, words) = change_body(&d, &cs[1], 1, TempUnit::Celsius).unwrap();
        assert_eq!(body["hvac_mode"], "off");
        assert_eq!(words, "Off");
        assert!(change_body(&d, &cs[1], -1, TempUnit::Celsius).is_none());
    }

    #[test]
    fn a_light_dims_turns_off_at_zero_and_takes_colors() {
        let d = dev(
            "light",
            json!({"on": true, "brightness_pct": 5, "color_xy": {"x": 0.4596, "y": 0.4105}}),
        );
        let s = schema(json!({
            "attributes": {
                "on": {"kind": "bool"},
                "brightness_pct": {"kind": "integer", "min": 0.0, "max": 100.0, "step": 1.0, "unit": "%"},
                "color_xy": {"kind": "color_xy"},
                "color_temp": {"kind": "color_temp"}
            },
            "primary": ["on", "brightness_pct"]
        }));
        let cs = controls_for(&d, Some(&s), false);
        let attrs: Vec<&str> = cs.iter().map(|c| c.attr.as_str()).collect();
        assert_eq!(
            attrs,
            vec!["on", "brightness_pct", "color_xy"],
            "no range, no warmth"
        );
        let (body, _) = change_body(&d, &cs[1], -1, TempUnit::Celsius).unwrap();
        assert_eq!(Value::Object(body), json!({"on": false}));
        let (body, words) = change_body(&d, &cs[1], 1, TempUnit::Celsius).unwrap();
        assert_eq!(
            Value::Object(body),
            json!({"brightness_pct": 10, "on": true})
        );
        assert_eq!(words, "10%");
        let (body, words) = change_body(&d, &cs[2], 1, TempUnit::Celsius).unwrap();
        assert_eq!(words, "soft white");
        assert_eq!(body["color_xy"], json!({"x": 0.4148, "y": 0.3959}));
        let (body, _) = change_body(&d, &cs[0], -1, TempUnit::Celsius).unwrap();
        assert_eq!(Value::Object(body), json!({"on": false}));
        assert!(
            change_body(&d, &cs[0], 1, TempUnit::Celsius).is_none(),
            "already on"
        );
    }

    #[test]
    fn numbered_options_send_numbers() {
        let d = dev("lock", json!({"sound_level": 2, "locked": true}));
        let s = schema(json!({
            "attributes": {
                "sound_level": {"kind": "integer", "min": 0.0, "max": 3.0, "category": "config",
                    "options": [{"label": "Mute", "value": "0"}, {"label": "Low", "value": "1"},
                                {"label": "Medium", "value": "2"}, {"label": "High", "value": "3"}]},
                "locked": {"kind": "bool", "states": {"when_true": {"label": "locked"},
                                                      "when_false": {"label": "unlocked"}}}
            }
        }));
        let cs = controls_for(&d, Some(&s), false);
        assert_eq!(cs[0].attr, "locked");
        assert_eq!(control_value(&d, &cs[0], TempUnit::Celsius), "locked");
        assert_eq!(control_value(&d, &cs[1], TempUnit::Celsius), "Medium");
        let (body, _) = change_body(&d, &cs[1], 1, TempUnit::Celsius).unwrap();
        assert_eq!(body["sound_level"], json!(3));
    }

    #[test]
    fn without_a_schema_the_vocabulary_gives_controls() {
        let d = dev(
            "shade",
            json!({"position": 40, "heat_setpoint": 20.0, "locked": false}),
        );
        let cs = controls_for(&d, None, false);
        let attrs: Vec<&str> = cs.iter().map(|c| c.attr.as_str()).collect();
        assert_eq!(attrs, vec!["position", "locked", "heat_setpoint"]);
        let (body, _) = change_body(&d, &cs[0], 1, TempUnit::Celsius).unwrap();
        assert_eq!(body["position"], json!(45));
    }

    #[test]
    fn the_history_line_follows_the_right_attribute() {
        let light = dev("light", json!({"on": true, "brightness_pct": 50}));
        assert_eq!(
            history_attribute(&light, None).as_deref(),
            Some("brightness_pct")
        );
        let sensor = dev(
            "temperature_sensor",
            json!({"temperature": 20.0, "humidity": 40}),
        );
        assert_eq!(
            history_attribute(&sensor, None).as_deref(),
            Some("temperature")
        );
        let player = dev("media_player", json!({"volume": 20}));
        assert_eq!(history_attribute(&player, None), None);
        let motion = dev("motion_sensor", json!({"motion": false}));
        let s = schema(json!({"attributes": {"motion": {"kind": "bool"}}, "primary": ["motion"]}));
        assert_eq!(
            history_attribute(&motion, Some(&s)).as_deref(),
            Some("motion")
        );
    }

    #[test]
    fn sparklines_scale_to_their_values() {
        assert_eq!(sparkline(&[0.0, 7.0], 10), "▁█");
        assert_eq!(sparkline(&[1.0, 1.0, 1.0], 2), "▄▄");
        assert_eq!(sparkline(&[0.0, 0.0, 10.0, 10.0], 2), "▁█");
        assert_eq!(sparkline(&[], 5), "");
        let pts: Vec<crate::model_devices::HistoryPoint> = serde_json::from_value(json!([
            {"attribute": "on", "value": true, "recorded_at": "b"},
            {"attribute": "on", "value": false, "recorded_at": "a"}
        ]))
        .unwrap();
        assert_eq!(history_values(&pts), vec![0.0, 1.0], "oldest first");
    }

    #[test]
    fn changes_wait_then_go_together() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _g = rt.enter();
        let mut a = signed_in(crate::model::Role::Admin);
        a.devices = vec![dev("light", json!({"on": true, "brightness_pct": 50}))];
        let body = |v: Value| v.as_object().cloned().unwrap();
        a.queue_send("d".into(), body(json!({"brightness_pct": 55})), "55".into());
        a.queue_send("d".into(), body(json!({"brightness_pct": 60})), "60".into());
        assert_eq!(
            a.devices[0].attributes["brightness_pct"], 60,
            "shown at once"
        );
        a.flush_device_controls();
        assert!(
            a.devices_tab.controls.pending.is_some(),
            "not before the delay"
        );
        a.devices_tab.controls.pending.as_mut().unwrap().due = Instant::now();
        a.flush_device_controls();
        assert!(a.devices_tab.controls.pending.is_none());
        assert_eq!(a.pending, 1, "one request for both presses");
    }
}
