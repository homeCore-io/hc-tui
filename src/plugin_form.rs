//! A plugin's settings as a form: from its config descriptor, or failing
//! that from its config JSON Schema, the way the app builds its form.
//!
//! Only what changed is written, as core's `{"patch": {section: {key:
//! value}}}`, which core applies in place so comments and keys the form
//! does not know stay in the file. Secrets are never shown: core sends
//! them as `__redacted__`, the form says only whether one is set, and an
//! untouched secret is never sent back.
//!
//! Everything here is pure; the Settings tab in `app::plugin_settings`
//! drives it.

use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value, json};

/// What core puts where a secret was.
#[cfg(test)]
pub const REDACTED: &str = "__redacted__";
/// The form's own row for the plugin's log level (`PATCH /plugins/{id}`).
pub const LOG_LEVEL_KEY: &str = "\u{0}log_level";
pub const LOG_LEVELS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

/// `null` reads as the default, as a missing field does.
fn nullable<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

// ── The descriptor (core's `hc_types::config_descriptor`) ──────────────────

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Descriptor {
    #[serde(default, deserialize_with = "nullable")]
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Section {
    #[serde(default, deserialize_with = "nullable")]
    pub id: String,
    #[serde(default, deserialize_with = "nullable")]
    pub title: String,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub hidden: bool,
    #[serde(default)]
    pub visible_when: Option<Value>,
    #[serde(default, deserialize_with = "nullable")]
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Field {
    #[serde(default, deserialize_with = "nullable")]
    pub kind: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default, deserialize_with = "nullable")]
    pub required: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub secret: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub read_only: bool,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default, deserialize_with = "nullable")]
    pub options: Vec<Opt>,
    /// A list's item kind (a string), or a table's columns.
    #[serde(default)]
    pub item: Option<Value>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub visible_when: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Opt {
    #[serde(default)]
    pub value: Value,
    #[serde(default, deserialize_with = "nullable")]
    pub label: String,
}

// ── From a JSON Schema ──────────────────────────────────────────────────────

/// `broker_port` to "Broker port".
pub fn humanize(key: &str) -> String {
    let mut spaced = String::new();
    let mut prev_lower = false;
    for ch in key.chars() {
        if ch.is_uppercase() && prev_lower {
            spaced.push(' ');
        }
        prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
        spaced.push(if ch == '_' || ch == '-' { ' ' } else { ch });
    }
    let words = spaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut c = words.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// A description's first paragraph, on one line.
fn help_of(s: &Value) -> Option<String> {
    let d = s.get("description")?.as_str()?.trim();
    if d.is_empty() {
        return None;
    }
    let first = d.split("\n\n").next().unwrap_or(d);
    Some(first.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// A setting whose name says it is a secret.
pub fn secret_name(name: &str) -> bool {
    let n = name.to_lowercase();
    [
        "password",
        "passwd",
        "secret",
        "token",
        "api_key",
        "apikey",
        "private_key",
        "privatekey",
    ]
    .iter()
    .any(|w| n == *w || n.ends_with(&format!("_{w}")))
}

/// The schema behind a `$ref`, an `allOf`, or an optional (`T | null`).
fn resolve(root: &Value, s: &Value, depth: usize) -> Value {
    let Some(obj) = s.as_object() else {
        return json!({});
    };
    if depth > 16 {
        return json!({});
    }
    let mut out = obj.clone();
    if let Some(r) = s.get("$ref").and_then(Value::as_str) {
        let target = r
            .strip_prefix("#/$defs/")
            .map(|n| ("$defs", n))
            .or_else(|| r.strip_prefix("#/definitions/").map(|n| ("definitions", n)))
            .and_then(|(k, n)| root.get(k).and_then(|d| d.get(n)))
            .cloned()
            .unwrap_or(json!({}));
        let mut merged = resolve(root, &target, depth + 1)
            .as_object()
            .cloned()
            .unwrap_or_default();
        out.remove("$ref");
        merged.extend(out);
        out = merged;
    }
    if let Some(all) = out.get("allOf").and_then(Value::as_array).cloned()
        && !all.is_empty()
    {
        out.remove("allOf");
        let mut merged = Map::new();
        for x in &all {
            if let Some(m) = resolve(root, x, depth + 1).as_object() {
                merged.extend(m.clone());
            }
        }
        merged.extend(out);
        out = merged;
    }
    for k in ["anyOf", "oneOf"] {
        let Some(alts) = out.get(k).and_then(Value::as_array).cloned() else {
            continue;
        };
        let real: Vec<&Value> = alts
            .iter()
            .filter(|x| x.get("type").and_then(Value::as_str) != Some("null"))
            .collect();
        if real.len() == 1 {
            out.remove(k);
            let mut merged = resolve(root, real[0], depth + 1)
                .as_object()
                .cloned()
                .unwrap_or_default();
            merged.extend(out);
            merged.insert("nullable".into(), Value::Bool(true));
            out = merged;
        } else if !real.is_empty() && real.iter().all(|x| x.get("const").is_some()) {
            out.remove(k);
            out.insert("type".into(), json!("string"));
            out.insert(
                "oneOfConsts".into(),
                Value::Array(real.into_iter().cloned().collect()),
            );
        }
    }
    if let Some(types) = out.get("type").and_then(Value::as_array).cloned() {
        let real: Vec<&Value> = types
            .iter()
            .filter(|t| t.as_str() != Some("null"))
            .collect();
        if real.len() == 1 {
            out.insert("type".into(), real[0].clone());
        }
        if real.len() < types.len() {
            out.insert("nullable".into(), Value::Bool(true));
        }
    }
    Value::Object(out)
}

fn is_table(s: &Value) -> bool {
    s.get("type").and_then(Value::as_str) == Some("object")
        && s.get("properties").is_some_and(Value::is_object)
}

/// A field for one plain setting, or `None` when it is more than that.
fn field_of(root: &Value, key: &str, name: &str, raw: &Value, required: bool) -> Option<Field> {
    let s = resolve(root, raw, 0);
    let default = s.get("default").filter(|d| !d.is_null()).cloned();
    let nullable = s.get("nullable").and_then(Value::as_bool).unwrap_or(false);
    let mut f = Field {
        kind: "text".into(),
        key: Some(key.to_string()),
        label: Some(
            s.get("title")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| humanize(name)),
        ),
        help: help_of(&s),
        required: required && default.is_none() && !nullable,
        default,
        min: s.get("minimum").and_then(Value::as_f64),
        max: s.get("maximum").and_then(Value::as_f64),
        ..Default::default()
    };
    if let Some(e) = s.get("enum").and_then(Value::as_array)
        && e.iter().all(|v| !v.is_object() && !v.is_array())
    {
        let named = s.get("x-options").and_then(Value::as_array);
        let label_of = |v: &Value| {
            named
                .and_then(|n| n.iter().find(|o| o.get("value") == Some(v)))
                .and_then(|o| o.get("label"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| entry(Some(v)))
        };
        f.kind = "enum".into();
        f.options = e
            .iter()
            .filter(|v| !v.is_null())
            .map(|v| Opt {
                value: v.clone(),
                label: label_of(v),
            })
            .collect();
        return Some(f);
    }
    // Labels for enum values (Hue's bridges by name and address).
    if let Some(named) = s.get("x-options").and_then(Value::as_array)
        && f.kind == "text"
        && !named.is_empty()
    {
        f.kind = "enum".into();
        f.options = named
            .iter()
            .map(|o| Opt {
                value: o.get("value").cloned().unwrap_or(Value::Null),
                label: o
                    .get("label")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| entry(o.get("value"))),
            })
            .collect();
        return Some(f);
    }
    if let Some(c) = s.get("oneOfConsts").and_then(Value::as_array) {
        f.kind = "enum".into();
        f.options = c
            .iter()
            .map(|x| {
                let value = x.get("const").cloned().unwrap_or(Value::Null);
                let label = x
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| help_of(x))
                    .unwrap_or_else(|| entry(Some(&value)));
                Opt { value, label }
            })
            .collect();
        return Some(f);
    }
    match s.get("type").and_then(Value::as_str) {
        Some("boolean") => f.kind = "toggle".into(),
        Some("integer") => f.kind = "int".into(),
        Some("number") => f.kind = "number".into(),
        Some("string") => {
            if secret_name(name)
                || s.get("writeOnly").and_then(Value::as_bool) == Some(true)
                || s.get("format").and_then(Value::as_str) == Some("password")
            {
                f.kind = "secret".into();
                f.secret = true;
            }
        }
        Some("array") => {
            let item = resolve(root, s.get("items").unwrap_or(&Value::Null), 0);
            if is_table(&item) {
                f.kind = "table".into();
                f.read_only = true;
                return Some(f);
            }
            let kind = match item.get("type").and_then(Value::as_str) {
                Some("integer") => "int",
                Some("number") => "number",
                Some("string") => "text",
                _ => return None,
            };
            f.kind = "list".into();
            f.item = Some(json!(kind));
        }
        _ => return None,
    }
    Some(f)
}

/// homeCore's own section in a plugin's file: how it signs in.
const HOMECORE: &str = "homecore";

/// A baseline descriptor from a config's JSON Schema, or `None` when the
/// schema holds nothing a form could show.
pub fn descriptor_from_schema(schema: &Value) -> Option<Descriptor> {
    let root = schema;
    let top = resolve(root, root, 0);
    if !is_table(&top) {
        return None;
    }
    let mut sections: Vec<Section> = Vec::new();
    let mut general: Vec<Field> = Vec::new();
    let mut left_out = 0usize;
    let req = |s: &Value| -> Vec<String> {
        s.get("required")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };

    fn walk(
        root: &Value,
        path: &str,
        name: &str,
        s: &Value,
        sections: &mut Vec<Section>,
        left_out: &mut usize,
    ) {
        let required: Vec<String> = s
            .get("required")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let mut fields = Vec::new();
        let mut nested = Vec::new();
        if let Some(props) = s.get("properties").and_then(Value::as_object) {
            for (k, raw) in props {
                let v = resolve(root, raw, 0);
                let key = format!("{path}.{k}");
                if is_table(&v) {
                    nested.push((key, k.clone(), v));
                } else if let Some(f) = field_of(root, &key, k, raw, required.contains(k)) {
                    fields.push(f);
                } else {
                    *left_out += 1;
                }
            }
        }
        if !fields.is_empty() {
            let own = path == HOMECORE;
            let title = if own {
                "Connection to homeCore".to_string()
            } else {
                s.get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| humanize(name))
            };
            if own {
                for f in &mut fields {
                    f.read_only = true;
                }
            }
            sections.push(Section {
                id: path.to_string(),
                title,
                help: if own {
                    Some("homeCore wrote these when it added the plugin.".into())
                } else {
                    help_of(s)
                },
                fields,
                ..Default::default()
            });
        }
        for (key, k, v) in nested {
            walk(root, &key, &k, &v, sections, left_out);
        }
    }

    let top_req = req(&top);
    if let Some(props) = top.get("properties").and_then(Value::as_object) {
        for (k, raw) in props {
            let v = resolve(root, raw, 0);
            if is_table(&v) {
                walk(root, k, k, &v, &mut sections, &mut left_out);
                continue;
            }
            // A patch writes into a table, never the top of the file:
            // these are shown and changed in the file.
            match field_of(root, k, k, raw, top_req.contains(k)) {
                Some(mut f) => {
                    f.read_only = true;
                    general.push(f);
                }
                None => left_out += 1,
            }
        }
    }
    if !general.is_empty() {
        sections.insert(
            0,
            Section {
                id: "_general".into(),
                title: "General".into(),
                help: Some("Settings at the top of the file. Change them in the file.".into()),
                fields: general,
                ..Default::default()
            },
        );
    }
    sections.sort_by_key(|s| s.id == HOMECORE);
    let first = sections.first_mut()?;
    if left_out > 0 {
        let what = if left_out == 1 {
            "1 more setting is".to_string()
        } else {
            format!("{left_out} more settings are")
        };
        first.fields.push(Field {
            kind: "note".into(),
            text: Some(format!(
                "{what} more than this form can show. They are in the file."
            )),
            ..Default::default()
        });
    }
    Some(Descriptor { sections })
}

/// An action's parameters as one section, from its `params`: a JSON
/// Schema object (`properties`, `required`) or a map of the parameters
/// themselves. `None` when it takes none.
pub fn params_descriptor(params: &Value) -> Option<Descriptor> {
    let (props, required): (Map<String, Value>, Vec<String>) =
        match params.get("properties").and_then(Value::as_object) {
            Some(p) => (
                p.clone(),
                params
                    .get("required")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            None => (
                params
                    .as_object()?
                    .iter()
                    .filter(|(_, v)| v.is_object())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                params
                    .as_object()?
                    .iter()
                    .filter(|(_, v)| v.get("required").and_then(Value::as_bool) == Some(true))
                    .map(|(k, _)| k.clone())
                    .collect(),
            ),
        };
    if props.is_empty() {
        return None;
    }
    let fields = props
        .iter()
        .map(|(name, raw)| {
            let req = required.contains(name);
            field_of(params, name, name, raw, req).unwrap_or_else(|| Field {
                kind: "json".into(),
                key: Some(name.clone()),
                label: Some(
                    raw.get("title")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| humanize(name)),
                ),
                help: help_of(raw),
                required: req,
                ..Default::default()
            })
        })
        .collect();
    Some(Descriptor {
        sections: vec![Section {
            id: "params".into(),
            title: "Parameters".into(),
            fields,
            ..Default::default()
        }],
    })
}

// ── The form ────────────────────────────────────────────────────────────────

/// How a row is edited.
#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    Toggle,
    Text,
    Int,
    Float,
    Secret,
    Enum(Vec<Opt>),
    /// A list of plain values, typed comma separated.
    List(Box<Control>),
    /// Shown, changed in the file or the app (tables, maps).
    Fixed,
    /// Typed as JSON (an object or list parameter).
    Json,
}

#[derive(Debug, Clone)]
pub struct FormField {
    pub key: String,
    pub label: String,
    pub help: Option<String>,
    pub unit: Option<String>,
    pub control: Control,
    pub default: Option<Value>,
    pub required: bool,
    pub read_only: bool,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// What the file holds (`None` where it says nothing).
    pub original: Option<Value>,
    pub draft: Option<Value>,
    pub visible_when: Option<Value>,
    pub section_visible_when: Option<Value>,
}

#[derive(Debug, Clone)]
pub enum Row {
    Heading(String),
    Note(String),
    Field(Box<FormField>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// A file core reads and patches in place.
    Toml,
    /// A plugin elsewhere that keeps its own settings.
    Remote,
}

#[derive(Debug, Clone)]
pub struct ConfigForm {
    pub rows: Vec<Row>,
    pub format: ConfigFormat,
    /// The document as core sent it (secrets redacted), for a remote
    /// plugin's whole-document write.
    pub config: Value,
    /// Made from the schema, not the plugin's own descriptor.
    pub from_schema: bool,
}

/// The value at a dotted path.
pub fn read_path<'a>(config: &'a Value, key: &str) -> Option<&'a Value> {
    let mut at = config;
    for part in key.split('.') {
        at = at.as_object()?.get(part)?;
    }
    Some(at)
}

fn set_path(config: &mut Value, key: &str, value: Option<Value>) {
    let parts: Vec<&str> = key.split('.').collect();
    let mut at = config;
    for part in &parts[..parts.len() - 1] {
        if !at.is_object() {
            *at = json!({});
        }
        at = at
            .as_object_mut()
            .expect("an object")
            .entry(part.to_string())
            .or_insert_with(|| json!({}));
    }
    if !at.is_object() {
        *at = json!({});
    }
    let map = at.as_object_mut().expect("an object");
    let leaf = parts[parts.len() - 1].to_string();
    match value {
        Some(v) => {
            map.insert(leaf, v);
        }
        None => {
            map.remove(&leaf);
        }
    }
}

/// Text for a value in an input or a cell.
pub fn entry(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| entry(Some(x)))
            .collect::<Vec<_>>()
            .join(", "),
        Some(other) => other.to_string(),
    }
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(_)) => true,
    }
}

/// Whether a `visible_when` condition holds, reading fields with `get`.
pub fn holds(c: Option<&Value>, get: &dyn Fn(&str) -> Option<Value>) -> bool {
    let Some(c) = c.filter(|c| !c.is_null()) else {
        return true;
    };
    if let Some(all) = c.get("all").and_then(Value::as_array) {
        return all.iter().all(|x| holds(Some(x), get));
    }
    if let Some(any) = c.get("any").and_then(Value::as_array) {
        return any.iter().any(|x| holds(Some(x), get));
    }
    if let Some(not) = c.get("not") {
        return !holds(Some(not), get);
    }
    let Some(field) = c.get("field").and_then(Value::as_str) else {
        return true;
    };
    let v = get(field);
    if let Some(t) = c.get("truthy").and_then(Value::as_bool) {
        return truthy(v.as_ref()) == t;
    }
    if let Some(eq) = c.get("eq") {
        return v.as_ref() == Some(eq);
    }
    if let Some(ne) = c.get("ne") {
        return v.as_ref() != Some(ne);
    }
    if let Some(list) = c.get("in").and_then(Value::as_array) {
        return v.as_ref().is_some_and(|v| list.contains(v));
    }
    let num = v.as_ref().and_then(Value::as_f64);
    if let Some(gt) = c.get("gt").and_then(Value::as_f64) {
        return num.is_some_and(|n| n > gt);
    }
    if let Some(lt) = c.get("lt").and_then(Value::as_f64) {
        return num.is_some_and(|n| n < lt);
    }
    true
}

fn control_of(f: &Field) -> Option<Control> {
    if f.secret || f.kind == "secret" {
        return Some(Control::Secret);
    }
    let scalar = |kind: &str| match kind {
        "toggle" => Some(Control::Toggle),
        "int" | "port" | "duration" => Some(Control::Int),
        "number" => Some(Control::Float),
        "text" | "host" | "url" => Some(Control::Text),
        "json" => Some(Control::Json),
        _ => None,
    };
    match f.kind.as_str() {
        "enum" | "select" if !f.options.is_empty() => Some(Control::Enum(f.options.clone())),
        "enum" | "select" | "table" | "map" | "sensor" => Some(Control::Fixed),
        "list" => match f.item.as_ref().and_then(Value::as_str).and_then(scalar) {
            Some(c @ (Control::Text | Control::Int | Control::Float)) => {
                Some(Control::List(Box::new(c)))
            }
            _ => Some(Control::Fixed),
        },
        k => scalar(k),
    }
}

impl ConfigForm {
    /// The form for `descriptor` over `config`, with the log level first.
    pub fn new(
        descriptor: &Descriptor,
        config: Value,
        format: ConfigFormat,
        log_level: Option<&str>,
        from_schema: bool,
    ) -> Self {
        let level = log_level.map(|l| Value::String(l.to_lowercase()));
        let mut rows = vec![
            Row::Heading("Plugin".into()),
            Row::Field(Box::new(FormField {
                key: LOG_LEVEL_KEY.into(),
                label: "Log level".into(),
                help: Some("Until the plugin restarts; homeCore does not keep it.".into()),
                unit: None,
                control: Control::Enum(
                    LOG_LEVELS
                        .iter()
                        .map(|l| Opt {
                            value: json!(l),
                            label: l.to_string(),
                        })
                        .collect(),
                ),
                default: None,
                required: false,
                read_only: false,
                min: None,
                max: None,
                original: level.clone(),
                draft: level,
                visible_when: None,
                section_visible_when: None,
            })),
        ];
        // Shown sections first, then the ones the app tucks away.
        let mut ordered: Vec<&Section> = descriptor.sections.iter().filter(|s| !s.hidden).collect();
        ordered.extend(descriptor.sections.iter().filter(|s| s.hidden));
        for s in ordered {
            let title = if s.title.is_empty() {
                humanize(&s.id)
            } else {
                s.title.clone()
            };
            rows.push(Row::Heading(if s.hidden {
                format!("{title} (advanced)")
            } else {
                title
            }));
            if let Some(h) = s.help.as_deref().filter(|h| !h.trim().is_empty()) {
                rows.push(Row::Note(h.to_string()));
            }
            for f in &s.fields {
                if f.kind == "note" {
                    if let Some(t) = f.text.as_deref().or(f.help.as_deref()) {
                        rows.push(Row::Note(t.to_string()));
                    }
                    continue;
                }
                let (Some(key), Some(control)) = (f.key.as_deref(), control_of(f)) else {
                    continue;
                };
                let original = read_path(&config, key).filter(|v| !v.is_null()).cloned();
                // A patch writes into a table; a key at the top of the
                // file is changed in the file.
                let top_level = !key.contains('.');
                rows.push(Row::Field(Box::new(FormField {
                    key: key.to_string(),
                    label: f
                        .label
                        .clone()
                        .unwrap_or_else(|| humanize(key.rsplit('.').next().unwrap_or(key))),
                    help: f.help.clone(),
                    unit: f.unit.clone(),
                    read_only: f.read_only
                        || control == Control::Fixed
                        || (top_level && format == ConfigFormat::Toml),
                    control,
                    default: f.default.clone().filter(|d| !d.is_null()),
                    required: f.required,
                    min: f.min,
                    max: f.max,
                    draft: original.clone(),
                    original,
                    visible_when: f.visible_when.clone(),
                    section_visible_when: s.visible_when.clone(),
                })));
            }
        }
        Self {
            rows,
            format,
            config,
            from_schema,
        }
    }

    /// An action's parameter form: no log level, every field editable,
    /// defaults filled in.
    pub fn for_params(descriptor: &Descriptor) -> Self {
        let mut f = Self::new(descriptor, json!({}), ConfigFormat::Remote, None, false);
        f.rows.drain(..2);
        f.rows.retain(|r| !matches!(r, Row::Heading(_)));
        for r in &mut f.rows {
            if let Row::Field(x) = r {
                x.draft = x.default.clone();
            }
        }
        f
    }

    /// The values set, by key: an action's parameters.
    pub fn values(&self) -> Map<String, Value> {
        self.rows
            .iter()
            .filter_map(|r| match r {
                Row::Field(f) => f
                    .draft
                    .clone()
                    .map(|v| (f.key.clone(), typed_value(&f.control, v))),
                _ => None,
            })
            .collect()
    }

    fn field(&self, key: &str) -> Option<&FormField> {
        self.rows.iter().find_map(|r| match r {
            Row::Field(f) if f.key == key => Some(&**f),
            _ => None,
        })
    }

    /// A field's value for conditions: what is set, else its default.
    pub fn effective(&self, key: &str) -> Option<Value> {
        let f = self.field(key)?;
        f.draft.clone().or_else(|| f.default.clone())
    }

    /// Whether row `i` is shown now (its conditions hold).
    pub fn shown(&self, i: usize) -> bool {
        let get = |k: &str| self.effective(k);
        match self.rows.get(i) {
            Some(Row::Field(f)) => {
                holds(f.section_visible_when.as_ref(), &get) && holds(f.visible_when.as_ref(), &get)
            }
            Some(_) => true,
            None => false,
        }
    }

    /// The rows shown, by index.
    pub fn visible_rows(&self) -> Vec<usize> {
        (0..self.rows.len()).filter(|&i| self.shown(i)).collect()
    }

    /// The editable rows shown, by index: what the cursor moves over.
    pub fn selectable(&self) -> Vec<usize> {
        self.visible_rows()
            .into_iter()
            .filter(|&i| matches!(self.rows[i], Row::Field(_)))
            .collect()
    }

    pub fn field_at(&self, i: usize) -> Option<&FormField> {
        match self.rows.get(i) {
            Some(Row::Field(f)) => Some(f),
            _ => None,
        }
    }

    fn field_at_mut(&mut self, i: usize) -> Option<&mut FormField> {
        match self.rows.get_mut(i) {
            Some(Row::Field(f)) => Some(f),
            _ => None,
        }
    }

    pub fn changed(&self) -> Vec<&FormField> {
        self.rows
            .iter()
            .filter_map(|r| match r {
                Row::Field(f) if f.draft != f.original => Some(&**f),
                _ => None,
            })
            .collect()
    }

    pub fn dirty(&self) -> bool {
        !self.changed().is_empty()
    }

    /// Space: flip a toggle or step an enum. False when the row is not one.
    pub fn step(&mut self, i: usize) -> bool {
        let Some(f) = self.field_at_mut(i) else {
            return false;
        };
        if f.read_only {
            return false;
        }
        let now = f.draft.clone().or_else(|| f.default.clone());
        match &f.control {
            Control::Toggle => {
                f.draft = Some(Value::Bool(!truthy(now.as_ref())));
                true
            }
            Control::Enum(opts) if !opts.is_empty() => {
                let at = opts.iter().position(|o| Some(&o.value) == now.as_ref());
                let next = at.map(|p| (p + 1) % opts.len()).unwrap_or(0);
                f.draft = Some(opts[next].value.clone());
                true
            }
            _ => false,
        }
    }

    /// Whether row `i` is edited by typing.
    pub fn typed(&self, i: usize) -> bool {
        self.field_at(i).is_some_and(|f| {
            !f.read_only
                && matches!(
                    f.control,
                    Control::Text
                        | Control::Int
                        | Control::Float
                        | Control::Secret
                        | Control::Json
                        | Control::List(_)
                )
        })
    }

    /// The text an input starts with. A secret starts empty: it is never
    /// shown, and leaving it empty keeps it.
    pub fn input_text(&self, i: usize) -> String {
        match self.field_at(i) {
            Some(f) if f.control == Control::Secret => String::new(),
            Some(f) if f.control == Control::Json => {
                f.draft.as_ref().map(|v| v.to_string()).unwrap_or_default()
            }
            Some(f) => entry(f.draft.as_ref()),
            None => String::new(),
        }
    }

    /// Typed text to the row's value, or what is wrong with it.
    pub fn commit(&mut self, i: usize, text: &str) -> Result<(), String> {
        let Some(f) = self.field_at_mut(i) else {
            return Ok(());
        };
        if f.control == Control::Secret {
            if !text.is_empty() {
                f.draft = Some(Value::String(text.to_string()));
            }
            return Ok(());
        }
        let v = parse(&f.control, text, f.min, f.max)?;
        if v.is_none() && f.required && f.default.is_none() {
            return Err("This needs a value.".into());
        }
        f.draft = v;
        Ok(())
    }

    /// Back to what the file holds.
    pub fn undo(&mut self, i: usize) {
        if let Some(f) = self.field_at_mut(i) {
            f.draft = f.original.clone();
        }
    }

    pub fn discard(&mut self) {
        for r in &mut self.rows {
            if let Row::Field(f) = r {
                f.draft = f.original.clone();
            }
        }
    }

    /// The log level, when it changed.
    pub fn log_level_change(&self) -> Option<String> {
        let f = self.field(LOG_LEVEL_KEY)?;
        (f.draft != f.original)
            .then(|| f.draft.as_ref().and_then(Value::as_str).map(str::to_string))
            .flatten()
    }

    /// The config write for what changed: core's `{"patch": ...}` for a
    /// file, the whole document for a remote plugin. `None` when only the
    /// log level (or nothing) changed. An error names a setting a patch
    /// cannot empty.
    pub fn write_body(&self) -> Result<Option<Value>, String> {
        let changes: Vec<&FormField> = self
            .changed()
            .into_iter()
            .filter(|f| f.key != LOG_LEVEL_KEY)
            .collect();
        if changes.is_empty() {
            return Ok(None);
        }
        if self.format == ConfigFormat::Remote {
            let mut doc = self.config.clone();
            for f in changes {
                set_path(
                    &mut doc,
                    &f.key,
                    f.draft.clone().map(|v| typed_value(&f.control, v)),
                );
            }
            return Ok(Some(json!({ "config": doc })));
        }
        let mut patch: Map<String, Value> = Map::new();
        let mut cannot = Vec::new();
        for f in changes {
            let now = match f.draft.clone().or_else(|| f.default.clone()) {
                Some(v) => typed_value(&f.control, v),
                None => {
                    cannot.push(f.label.clone());
                    continue;
                }
            };
            let (table, leaf) = f.key.rsplit_once('.').unwrap_or(("", f.key.as_str()));
            patch
                .entry(table.to_string())
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .expect("an object")
                .insert(leaf.to_string(), now);
        }
        if !cannot.is_empty() {
            return Err(format!(
                "{} cannot be emptied here. Edit the file instead.",
                cannot.join(", ")
            ));
        }
        Ok(Some(json!({ "patch": patch })))
    }

    /// Required settings that are shown and have no value.
    pub fn problems(&self) -> Vec<String> {
        self.visible_rows()
            .into_iter()
            .filter_map(|i| self.field_at(i))
            .filter(|f| {
                f.required
                    && !f.read_only
                    && f.default.is_none()
                    && f.draft.as_ref().is_none_or(|v| entry(Some(v)).is_empty())
            })
            .map(|f| format!("{} needs a value.", f.label))
            .collect()
    }
}

/// A float setting stays a float in the file: 20.0, never 20.
fn typed_value(c: &Control, v: Value) -> Value {
    match (c, &v) {
        (Control::Float, Value::Number(n)) => n
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(v),
        _ => v,
    }
}

/// Parse typed text for `control`. Empty text is `None`: the setting
/// says nothing, and its default applies.
pub fn parse(
    control: &Control,
    text: &str,
    min: Option<f64>,
    max: Option<f64>,
) -> Result<Option<Value>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let bounds = |n: f64| -> Result<(), String> {
        if let Some(m) = min
            && n < m
        {
            return Err(format!("At least {m}."));
        }
        if let Some(m) = max
            && n > m
        {
            return Err(format!("At most {m}."));
        }
        Ok(())
    };
    match control {
        Control::Int => {
            let n: i64 = t
                .parse()
                .map_err(|_| "A whole number, please.".to_string())?;
            bounds(n as f64)?;
            Ok(Some(json!(n)))
        }
        Control::Float => {
            let n: f64 = t
                .parse()
                .ok()
                .filter(|n: &f64| n.is_finite())
                .ok_or_else(|| "That is not a number.".to_string())?;
            bounds(n)?;
            Ok(Some(json!(n)))
        }
        Control::List(item) => {
            let mut out = Vec::new();
            for part in t.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                match parse(item, part, None, None) {
                    Ok(Some(v)) => out.push(v),
                    Ok(None) => {}
                    Err(e) => return Err(format!("{part}: {e}")),
                }
            }
            Ok(Some(Value::Array(out)))
        }
        Control::Json => serde_json::from_str(t)
            .map(Some)
            .map_err(|_| "That is not JSON.".to_string()),
        _ => Ok(Some(Value::String(t.to_string()))),
    }
}

/// A row's value as a person reads it. A secret is never shown: only
/// whether one is set or changed.
pub fn describe(f: &FormField) -> (String, bool) {
    if f.control == Control::Secret {
        let set = f
            .original
            .as_ref()
            .is_some_and(|v| !entry(Some(v)).is_empty());
        let words = if f.draft != f.original {
            "changed, hidden"
        } else if set {
            "set, hidden"
        } else {
            "not set"
        };
        return (words.into(), false);
    }
    let (v, is_default) = match (&f.draft, &f.default) {
        (Some(v), _) => (v.clone(), false),
        (None, Some(d)) => (d.clone(), true),
        (None, None) => return ("not set".into(), true),
    };
    let text = match (&f.control, &v) {
        (_, Value::Bool(b)) => if *b { "on" } else { "off" }.to_string(),
        (Control::Enum(opts), _) => opts
            .iter()
            .find(|o| o.value == v)
            .map(|o| o.label.clone())
            .unwrap_or_else(|| entry(Some(&v))),
        (Control::Fixed, Value::Array(a)) => {
            let n = a.len();
            if n == 1 {
                "1 entry".into()
            } else {
                format!("{n} entries")
            }
        }
        (Control::Fixed, Value::Object(m)) => {
            let n = m.len();
            if n == 1 {
                "1 entry".into()
            } else {
                format!("{n} entries")
            }
        }
        (_, Value::String(s)) if s.is_empty() => "empty".into(),
        _ => entry(Some(&v)),
    };
    let text = match &f.unit {
        Some(u) if !matches!(v, Value::Bool(_)) => format!("{text} {}", unit_name(u)),
        _ => text,
    };
    (text, is_default)
}

fn unit_name(u: &str) -> &str {
    match u {
        "secs" => "seconds",
        "ms" => "milliseconds",
        "mins" | "min" => "minutes",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Value {
        json!({
            "title": "Config",
            "type": "object",
            "required": ["homecore"],
            "properties": {
                "debug": {"type": "boolean", "default": false},
                "homecore": {"$ref": "#/$defs/Homecore"},
                "sonos": {
                    "type": "object",
                    "properties": {
                        "poll_secs": {"type": "integer", "minimum": 5, "default": 30},
                        "gain": {"type": "number"},
                        "allow_remote": {"type": "boolean", "description": "Let other machines in.\n\nNotes for devs."},
                        "mode": {"type": "string", "enum": ["fast", "slow"]},
                        "api_token": {"type": ["string", "null"]},
                        "hosts": {"type": "array", "items": {"type": "string"}},
                        "rooms": {"type": "array", "items": {"type": "object", "properties": {"name": {"type": "string"}}}},
                        "blob": {"type": "object"}
                    }
                }
            },
            "$defs": {
                "Homecore": {
                    "type": "object",
                    "properties": {
                        "broker_host": {"type": "string"},
                        "password": {"type": "string"}
                    }
                }
            }
        })
    }

    fn config() -> Value {
        json!({
            "homecore": {"broker_host": "127.0.0.1", "password": REDACTED},
            "sonos": {"poll_secs": 60, "gain": 1.5, "api_token": REDACTED, "hosts": ["a", "b"],
                      "rooms": [{"name": "Den"}], "unknown_key": 7}
        })
    }

    fn form() -> ConfigForm {
        let d = descriptor_from_schema(&schema()).unwrap();
        ConfigForm::new(&d, config(), ConfigFormat::Toml, Some("INFO"), true)
    }

    fn row(f: &ConfigForm, key: &str) -> usize {
        f.rows
            .iter()
            .position(|r| matches!(r, Row::Field(x) if x.key == key))
            .unwrap_or_else(|| panic!("no row {key}"))
    }

    #[test]
    fn the_schema_becomes_sections_and_controls() {
        let d = descriptor_from_schema(&schema()).unwrap();
        let ids: Vec<&str> = d.sections.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["_general", "sonos", "homecore"],
            "homeCore's own last"
        );
        let f = form();
        let control = |k: &str| f.field_at(row(&f, k)).unwrap().control.clone();
        assert_eq!(control("sonos.poll_secs"), Control::Int);
        assert_eq!(control("sonos.gain"), Control::Float);
        assert_eq!(control("sonos.allow_remote"), Control::Toggle);
        assert!(matches!(control("sonos.mode"), Control::Enum(o) if o.len() == 2));
        assert_eq!(
            control("sonos.api_token"),
            Control::Secret,
            "a nullable secret"
        );
        assert_eq!(control("homecore.password"), Control::Secret);
        assert_eq!(
            control("sonos.hosts"),
            Control::List(Box::new(Control::Text))
        );
        assert_eq!(control("sonos.rooms"), Control::Fixed);
        // The help is the first paragraph only.
        let help = f
            .field_at(row(&f, "sonos.allow_remote"))
            .unwrap()
            .help
            .clone();
        assert_eq!(help.as_deref(), Some("Let other machines in."));
        // Top-level keys and homeCore's own section are read only.
        assert!(f.field_at(row(&f, "debug")).unwrap().read_only);
        assert!(
            f.field_at(row(&f, "homecore.broker_host"))
                .unwrap()
                .read_only
        );
        // The object with no properties is left out, and said so.
        assert!(
            f.rows
                .iter()
                .any(|r| matches!(r, Row::Note(t) if t.contains("1 more setting")))
        );
        assert_eq!(humanize("broker_port"), "Broker port");
        assert_eq!(humanize("pollInterval"), "Poll interval");
    }

    #[test]
    fn secrets_are_never_shown_or_sent_back_untouched() {
        let mut f = form();
        let i = row(&f, "sonos.api_token");
        let (text, _) = describe(f.field_at(i).unwrap());
        assert_eq!(text, "set, hidden");
        assert!(!text.contains(REDACTED));
        assert_eq!(f.input_text(i), "", "an input never starts with the secret");
        // Leaving the input empty keeps it.
        f.commit(i, "").unwrap();
        assert!(!f.dirty());
        assert_eq!(f.write_body().unwrap(), None);
        f.commit(i, "new-token").unwrap();
        assert_eq!(describe(f.field_at(i).unwrap()).0, "changed, hidden");
        let body = f.write_body().unwrap().unwrap();
        assert_eq!(
            body,
            json!({"patch": {"sonos": {"api_token": "new-token"}}})
        );
    }

    #[test]
    fn only_changes_are_written_as_a_patch() {
        let mut f = form();
        let poll = row(&f, "sonos.poll_secs");
        assert_eq!(f.input_text(poll), "60");
        assert!(f.commit(poll, "2").is_err(), "below the minimum");
        assert!(f.commit(poll, "abc").is_err());
        f.commit(poll, "45").unwrap();
        let gain = row(&f, "sonos.gain");
        f.commit(gain, "2").unwrap();
        let remote = row(&f, "sonos.allow_remote");
        assert!(f.step(remote));
        let hosts = row(&f, "sonos.hosts");
        f.commit(hosts, "a, c").unwrap();
        let body = f.write_body().unwrap().unwrap();
        assert_eq!(
            body,
            json!({"patch": {"sonos": {"poll_secs": 45, "gain": 2.0, "allow_remote": true, "hosts": ["a", "c"]}}})
        );
        // A float stays a float on the wire, and unknown keys are not sent.
        let text = serde_json::to_string(&body).unwrap();
        assert!(text.contains("\"gain\":2.0"), "{text}");
        assert!(!text.contains("unknown_key") && !text.contains("rooms"));
        f.undo(gain);
        f.discard();
        assert!(!f.dirty());
    }

    #[test]
    fn an_emptied_setting_writes_its_default_or_is_refused() {
        let mut f = form();
        let poll = row(&f, "sonos.poll_secs");
        f.commit(poll, "").unwrap();
        assert_eq!(
            f.write_body().unwrap().unwrap(),
            json!({"patch": {"sonos": {"poll_secs": 30}}})
        );
        f.discard();
        let gain = row(&f, "sonos.gain");
        f.commit(gain, "").unwrap();
        assert!(
            f.write_body()
                .unwrap_err()
                .contains("Gain cannot be emptied")
        );
    }

    #[test]
    fn the_log_level_is_its_own_change() {
        let mut f = form();
        let i = row(&f, LOG_LEVEL_KEY);
        assert_eq!(describe(f.field_at(i).unwrap()).0, "info");
        f.step(i);
        assert_eq!(f.log_level_change().as_deref(), Some("debug"));
        assert_eq!(f.write_body().unwrap(), None, "not part of the file");
    }

    #[test]
    fn a_remote_plugin_gets_the_whole_document() {
        let d = descriptor_from_schema(&schema()).unwrap();
        let mut f = ConfigForm::new(&d, config(), ConfigFormat::Remote, None, true);
        let i = row(&f, "sonos.mode");
        f.step(i);
        let body = f.write_body().unwrap().unwrap();
        assert_eq!(body["config"]["sonos"]["mode"], json!("fast"));
        // Everything else goes back as it came, secrets still redacted for
        // core to restore.
        assert_eq!(body["config"]["sonos"]["unknown_key"], json!(7));
        assert_eq!(body["config"]["homecore"]["password"], json!(REDACTED));
    }

    #[test]
    fn descriptors_read_with_nulls_and_conditions() {
        let d: Descriptor = serde_json::from_value(json!({"sections": [
            {"id": "a", "title": "A", "hidden": null, "fields": [
                {"kind": "toggle", "key": "a.on", "secret": null, "options": null},
                {"kind": "text", "key": "a.host", "visible_when": {"field": "a.on", "truthy": true}},
                {"kind": "enum", "key": "a.level", "default": "info",
                 "options": [{"value": "info", "label": "Info"}, {"value": "debug", "label": "Debug"}]},
                {"kind": "note", "text": "Read the manual."},
                {"kind": "table", "key": "a.rows"}
            ]},
            {"id": "conn", "title": "Connection", "hidden": true, "fields": [
                {"kind": "secret", "key": "homecore.password"}
            ]}
        ]}))
        .unwrap();
        let mut f = ConfigForm::new(
            &d,
            json!({"a": {"rows": [{}, {}]}}),
            ConfigFormat::Toml,
            None,
            false,
        );
        let host = row(&f, "a.host");
        assert!(!f.shown(host), "hidden until a.on");
        f.step(row(&f, "a.on"));
        assert!(f.shown(host));
        let level = row(&f, "a.level");
        assert_eq!(describe(f.field_at(level).unwrap()), ("Info".into(), true));
        let rows = row(&f, "a.rows");
        assert_eq!(describe(f.field_at(rows).unwrap()).0, "2 entries");
        assert!(f.field_at(rows).unwrap().read_only);
        assert!(
            f.rows
                .iter()
                .any(|r| matches!(r, Row::Heading(h) if h == "Connection (advanced)"))
        );
        assert!(
            f.rows
                .iter()
                .any(|r| matches!(r, Row::Note(t) if t == "Read the manual."))
        );
        assert!(f.selectable().iter().all(|&i| f.field_at(i).is_some()));
    }

    #[test]
    fn action_parameters_become_a_form() {
        // JSON Schema style, as the Python SDK sends it.
        let params = json!({
            "type": "object",
            "required": ["message"],
            "properties": {
                "message": {"type": "string", "title": "Message"},
                "bridge": {"type": "string", "enum": ["a", "b"],
                           "x-options": [{"value": "a", "label": "Upstairs"}, {"value": "b", "label": "Den"}]},
                "count": {"type": "integer", "default": 3},
                "api_key": {"type": "string"},
                "extra": {"type": "object"}
            }
        });
        let d = params_descriptor(&params).unwrap();
        let mut f = ConfigForm::for_params(&d);
        assert!(f.rows.iter().all(|r| matches!(r, Row::Field(_))));
        let bridge = row(&f, "bridge");
        assert!(
            matches!(&f.field_at(bridge).unwrap().control, Control::Enum(o) if o[0].label == "Upstairs")
        );
        assert_eq!(
            f.field_at(row(&f, "api_key")).unwrap().control,
            Control::Secret
        );
        assert_eq!(f.field_at(row(&f, "extra")).unwrap().control, Control::Json);
        assert_eq!(f.problems(), vec!["Message needs a value."]);
        f.commit(row(&f, "message"), "hi").unwrap();
        f.commit(row(&f, "extra"), "{\"a\": 1}").unwrap();
        assert!(f.commit(row(&f, "extra"), "{nope").is_err());
        let v = Value::Object(f.values());
        assert_eq!(v, json!({"message": "hi", "count": 3, "extra": {"a": 1}}));
        // A map of the parameters themselves reads too; none is None.
        let d =
            params_descriptor(&json!({"level": {"type": "boolean", "required": true}})).unwrap();
        assert_eq!(d.sections[0].fields.len(), 1);
        assert!(params_descriptor(&json!({"type": "object"})).is_none());
    }

    #[test]
    fn required_settings_without_a_value_are_problems() {
        let d: Descriptor = serde_json::from_value(json!({"sections": [
            {"id": "a", "title": "A", "fields": [{"kind": "text", "key": "a.name", "required": true}]}
        ]}))
        .unwrap();
        let mut f = ConfigForm::new(&d, json!({}), ConfigFormat::Toml, None, false);
        assert_eq!(f.problems(), vec!["Name needs a value."]);
        let i = row(&f, "a.name");
        assert!(f.commit(i, " ").is_err());
        f.commit(i, "den").unwrap();
        assert!(f.problems().is_empty());
    }
}
