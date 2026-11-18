//! Shapes for device controls, history and removed devices.
//!
//! These mirror core's `hc_types::schema` (`DeviceSchema`,
//! `AttributeSchema`, `DeviceAction`), the rows of
//! `GET /devices/{id}/history` and `GET /devices/removed`. Every
//! optional field defaults, so a newer core still parses.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// `GET /devices/{id}/schema`: what a device's attributes are and which
/// of them can be set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceSchema {
    #[serde(default)]
    pub attributes: HashMap<String, AttributeSchema>,
    #[serde(default)]
    pub actions: Vec<DeviceAction>,
    /// The headline attributes, most important first.
    #[serde(default)]
    pub primary: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttributeKind {
    Bool,
    Integer,
    Float,
    String,
    Enum,
    ColorXy,
    ColorRgb,
    ColorTemp,
    #[default]
    #[serde(other)]
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttributeCategory {
    Diagnostic,
    Config,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AttributeSchema {
    #[serde(default)]
    pub kind: AttributeKind,
    /// Core's default: absent means writable.
    #[serde(default = "yes")]
    pub writable: bool,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default)]
    pub options: Option<Vec<AttributeOption>>,
    /// Absent for the attributes that matter day to day.
    #[serde(default)]
    pub category: Option<AttributeCategory>,
    #[serde(default)]
    pub states: Option<BoolStates>,
}

fn yes() -> bool {
    true
}

/// An enum choice: core sends a bare string or `{value, label?}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AttributeOption {
    pub value: String,
    pub label: Option<String>,
}

impl AttributeOption {
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.value)
    }
}

impl<'de> Deserialize<'de> for AttributeOption {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let text = |v: &Value| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        Ok(match &v {
            Value::Object(m) => AttributeOption {
                value: m.get("value").map(text).unwrap_or_default(),
                label: m.get("label").and_then(Value::as_str).map(str::to_string),
            },
            other => AttributeOption {
                value: text(other),
                label: None,
            },
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateLabel {
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BoolStates {
    #[serde(default)]
    pub when_true: StateLabel,
    #[serde(default)]
    pub when_false: StateLabel,
}

/// A command the device takes as `{"action": id, ...params}`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceAction {
    pub id: String,
    #[serde(default)]
    pub label: String,
    /// When set, ask with this text before running it.
    #[serde(default)]
    pub confirm: Option<String>,
    #[serde(default)]
    pub params: Vec<ParamSpec>,
    /// The attribute this action sets.
    #[serde(default)]
    pub writes: Option<String>,
    #[serde(default)]
    pub requires_role: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParamSpec {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
}

impl DeviceAction {
    /// Runs with one key: no parameter that must be given.
    pub fn one_key(&self) -> bool {
        self.params
            .iter()
            .all(|p| !p.required || p.default.is_some())
    }
}

/// One row of `GET /devices/{id}/history`, newest first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryPoint {
    #[serde(default)]
    pub attribute: String,
    #[serde(default)]
    pub value: Value,
    #[serde(default)]
    pub recorded_at: String,
}

/// One entry of `GET /devices/removed`: a device homeCore was told to
/// forget. It comes back when its plugin reports it again after
/// `DELETE /devices/removed/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemovedDevice {
    pub device_id: String,
    #[serde(default)]
    pub plugin_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub removed_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_schema_from_core_parses() {
        let s: DeviceSchema = serde_json::from_value(json!({
            "attributes": {
                "on": {"kind": "bool", "display_name": "Power"},
                "speed": {"kind": "enum", "options": ["off", "low"]},
                "src": {"kind": "enum", "options": [{"value": "app", "label": "The app"}]},
                "level": {"kind": "integer", "options": [{"value": 2, "label": "Two"}], "writable": false},
                "odd": {"kind": "something_new", "category": "whatever"}
            },
            "actions": [{"id": "stop", "label": "Stop", "requires_role": "user"}],
            "primary": ["on"]
        }))
        .unwrap();
        assert!(
            s.attributes["on"].writable,
            "absent writable means writable"
        );
        assert!(!s.attributes["level"].writable);
        let speed = s.attributes["speed"].options.as_ref().unwrap();
        assert_eq!(speed[1].value, "low");
        assert_eq!(speed[1].label(), "low");
        let src = s.attributes["src"].options.as_ref().unwrap();
        assert_eq!(src[0].label(), "The app");
        assert_eq!(
            s.attributes["level"].options.as_ref().unwrap()[0].value,
            "2"
        );
        assert_eq!(s.attributes["odd"].kind, AttributeKind::Json);
        assert_eq!(s.attributes["odd"].category, Some(AttributeCategory::Other));
        assert!(s.actions[0].one_key());
    }
}
