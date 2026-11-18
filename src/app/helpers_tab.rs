//! Helpers screen: every kind of helper core has, each kind's own keys,
//! and making, changing and deleting one.
//!
//! What core takes when a helper is made (`POST /glue`) differs from
//! what it lets change afterwards (`PATCH /glue/{id}`, a whitelist per
//! kind); [`fields_for`] spells out both, in the app's words.

use super::App;
use super::editors::{Confirm, ConfirmAction, edit_text};
use crate::model::DeviceState;
use crossterm::event::{KeyCode, KeyEvent};
use serde_json::{Map, Value, json};

/// Every kind of helper, in the order the type picker lists them.
pub const HELPER_TYPES: [&str; 11] = [
    "switch",
    "timer",
    "counter",
    "number",
    "select",
    "text",
    "button",
    "datetime",
    "group",
    "threshold",
    "schedule",
];

/// A kind's name, as the app says it.
pub fn type_label(t: &str) -> &'static str {
    match t {
        "switch" => "Switch",
        "timer" => "Timer",
        "counter" => "Counter",
        "number" => "Number",
        "select" => "Select",
        "text" => "Text",
        "button" => "Button",
        "datetime" => "Date & time",
        "group" => "Group",
        "threshold" => "Threshold",
        "schedule" => "Schedule",
        _ => "Helper",
    }
}

/// What a kind is for, in the app's words.
pub fn type_what(t: &str) -> &'static str {
    match t {
        "switch" => "On or off, set by people or rules (Vacation, Guests).",
        "timer" => "Counts down; rules act when it finishes (the bathroom fan).",
        "counter" => "Counts up or down (how often the deck door opened).",
        "number" => "A value people set within limits (target humidity).",
        "select" => "One of a list: Home, Away, Vacation, Guest.",
        "text" => "Words a rule or a person sets.",
        "button" => "Something to press; rules react to the press.",
        "datetime" => "A moment people set (the wake-up time).",
        "group" => "Several devices as one: a deck door is open if any is.",
        "threshold" => "On while a reading is past a value (office humidity high).",
        "schedule" => "On during blocks of the week (weekdays 8 to 5).",
        _ => "",
    }
}

/// How a setting is typed and sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Int,
    Number,
    /// `yes` or `no`; Space flips it.
    Toggle,
    /// One of these; Space and ←/→ step through them.
    Choice(&'static [&'static str]),
    /// Comma separated, sent as a list of strings.
    List,
    /// Minutes typed, seconds sent (`duration_secs`).
    Minutes,
    /// A schedule's blocks: `Mon-Fri 08:00-17:00; Sat 10:00-12:00`.
    Blocks,
}

#[derive(Debug, Clone)]
pub struct HelperField {
    /// The config key core reads.
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub value: String,
    /// What it held when the editor opened; only changes are sent.
    pub initial: String,
    pub required: bool,
    pub help: &'static str,
}

impl HelperField {
    fn new(key: &'static str, label: &'static str, kind: FieldKind, value: &str) -> Self {
        Self {
            key,
            label,
            kind,
            value: value.to_string(),
            initial: value.to_string(),
            required: false,
            help: "",
        }
    }

    fn help(mut self, h: &'static str) -> Self {
        self.help = h;
        self
    }

    fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn changed(&self) -> bool {
        self.value.trim() != self.initial.trim()
    }

    /// The value as core takes it. `None`: left empty, so not sent.
    pub fn json(&self) -> Result<Option<Value>, String> {
        let v = self.value.trim();
        if v.is_empty() {
            return if self.required {
                Err(format!("{} is needed", self.label))
            } else {
                Ok(None)
            };
        }
        let bad = |what: &str| format!("{} must be {what}", self.label);
        Ok(Some(match self.kind {
            FieldKind::Text | FieldKind::Choice(_) => json!(v),
            FieldKind::Int => json!(v.parse::<i64>().map_err(|_| bad("a whole number"))?),
            FieldKind::Number => json!(v.parse::<f64>().map_err(|_| bad("a number"))?),
            FieldKind::Toggle => json!(v == "yes"),
            FieldKind::Minutes => {
                let m: f64 = v.parse().map_err(|_| bad("minutes"))?;
                if m < 0.0 {
                    return Err(bad("minutes"));
                }
                json!((m * 60.0).round() as u64)
            }
            FieldKind::List => {
                let items: Vec<Value> = v
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| json!(s))
                    .collect();
                if items.is_empty() && self.required {
                    return Err(format!("{} is needed", self.label));
                }
                Value::Array(items)
            }
            FieldKind::Blocks => parse_blocks(v)?,
        }))
    }

    /// Space or ←/→ on a toggle or a choice.
    pub fn step(&mut self, forward: bool) -> bool {
        match self.kind {
            FieldKind::Toggle => {
                self.value = if self.value == "yes" { "no" } else { "yes" }.into();
                true
            }
            FieldKind::Choice(opts) => {
                let i = opts.iter().position(|o| *o == self.value).unwrap_or(0);
                let n = opts.len();
                self.value = opts[if forward {
                    (i + 1) % n
                } else {
                    (i + n - 1) % n
                }]
                .into();
                true
            }
            _ => false,
        }
    }
}

const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

fn day(s: &str) -> Option<usize> {
    let s = s.trim().to_lowercase();
    DAYS.iter().position(|d| d.to_lowercase() == s)
}

/// `Mon-Fri 08:00-17:00; Sat,Sun 10:00-12:00` as core's blocks.
pub fn parse_blocks(s: &str) -> Result<Value, String> {
    let mut out = Vec::new();
    for (n, part) in s.split(';').map(str::trim).enumerate() {
        if part.is_empty() {
            continue;
        }
        let n = n + 1;
        let (days, times) = part
            .rsplit_once(' ')
            .ok_or_else(|| format!("block {n}: days, then a time like 08:00-17:00"))?;
        let mut list = Vec::new();
        for d in days.split(',').map(str::trim).filter(|d| !d.is_empty()) {
            match d.split_once('-') {
                Some((a, b)) => {
                    let (a, b) = (day(a), day(b));
                    let (Some(a), Some(b)) = (a, b) else {
                        return Err(format!("block {n}: days are Mon, Tue, ... Sun"));
                    };
                    let mut i = a;
                    loop {
                        list.push(DAYS[i]);
                        if i == b {
                            break;
                        }
                        i = (i + 1) % 7;
                    }
                }
                None => list.push(
                    DAYS[day(d).ok_or_else(|| format!("block {n}: days are Mon, Tue, ... Sun"))?],
                ),
            }
        }
        let (start, end) = times
            .split_once('-')
            .ok_or_else(|| format!("block {n}: a time like 08:00-17:00"))?;
        let ok = |t: &str| chrono::NaiveTime::parse_from_str(t.trim(), "%H:%M").is_ok();
        if !ok(start) || !ok(end) {
            return Err(format!("block {n}: times are like 08:00"));
        }
        out.push(json!({ "days": list, "start": start.trim(), "end": end.trim() }));
    }
    Ok(Value::Array(out))
}

/// Core's blocks back as typed text.
pub fn format_blocks(v: &Value) -> String {
    let Some(list) = v.as_array() else {
        return String::new();
    };
    list.iter()
        .map(|b| {
            let days: Vec<&str> = b
                .get("days")
                .and_then(Value::as_array)
                .map(|d| d.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let s = |k: &str| b.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            format!("{} {}-{}", days.join(","), s("start"), s("end"))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// The settings for a kind: when making one (`creating`) or changing it.
pub fn fields_for(t: &str, creating: bool) -> Vec<HelperField> {
    use FieldKind as K;
    let f = HelperField::new;
    match t {
        "timer" => vec![
            f("duration_secs", "Runs for (min)", K::Minutes, "")
                .help("Unless a rule starts it for longer."),
            f("repeat", "Start again", K::Toggle, "no").help("When it finishes."),
        ],
        "counter" => vec![
            f("step", "Step", K::Int, if creating { "1" } else { "" })
                .help("How much each count adds."),
            f("min", "Lowest", K::Int, "").help("Empty for no limit."),
            f("max", "Highest", K::Int, "").help("Empty for no limit."),
        ],
        "number" => {
            let mut v = Vec::new();
            if creating {
                v.push(f("value", "Starts at", K::Number, "0"));
            }
            v.extend([
                f("min", "Lowest", K::Number, if creating { "0" } else { "" }),
                f(
                    "max",
                    "Highest",
                    K::Number,
                    if creating { "100" } else { "" },
                ),
                f("step", "Step", K::Number, if creating { "1" } else { "" }),
                f("unit", "Unit", K::Text, "").help("Like %"),
            ]);
            v
        }
        "select" => vec![
            f("options", "Choices", K::List, "")
                .required()
                .help("Comma separated: Home, Away, Vacation"),
        ],
        "text" => vec![f("max_length", "Longest", K::Int, "").help("In characters.")],
        "datetime" => vec![
            f("has_date", "Has a date", K::Toggle, "yes"),
            f("has_time", "Has a time", K::Toggle, "yes"),
        ],
        "group" => vec![
            f("members", "Members", K::List, "")
                .required()
                .help("Device ids, comma separated."),
            f("attribute", "Reading", K::Text, "on")
                .help("What each member reports: on, open, motion."),
            f("mode", "On when", K::Choice(&["any", "all"]), "any")
                .help("any: any member is; all: all are."),
            f("expect", "Counts as on when true", K::Toggle, "yes"),
        ],
        "threshold" => vec![
            f("source_device_id", "Device", K::Text, "")
                .required()
                .help("The device id whose reading counts."),
            f("source_attribute", "Reading", K::Text, "value").help("Like humidity."),
            f("threshold", "Above", K::Number, "0"),
            f("hysteresis", "Back below by", K::Number, "0")
                .help("How far it must fall before it turns off again."),
        ],
        "schedule" => vec![
            f("blocks", "On during", K::Blocks, "").help("Mon-Fri 08:00-17:00; Sat 10:00-12:00"),
        ],
        _ => Vec::new(),
    }
}

/// A setting's current value read from the helper's attributes.
fn value_of(d: &DeviceState, field: &HelperField) -> String {
    let a = &d.attributes;
    let key = if field.key == "members" {
        "member_ids"
    } else {
        field.key
    };
    let num = |v: &Value| match v.as_f64() {
        Some(x) if x.fract() == 0.0 => format!("{}", x as i64),
        Some(x) => format!("{x}"),
        None => String::new(),
    };
    match field.kind {
        FieldKind::Minutes => {
            let own = a
                .get("default_secs")
                .and_then(Value::as_f64)
                .or_else(|| a.get("duration_secs").and_then(Value::as_f64));
            own.map(|s| num(&json!(s / 60.0))).unwrap_or_default()
        }
        FieldKind::Toggle => match a.get(key).and_then(Value::as_bool) {
            Some(true) => "yes".into(),
            Some(false) => "no".into(),
            None => field.value.clone(),
        },
        FieldKind::List => a
            .get(key)
            .and_then(Value::as_array)
            .map(|l| {
                l.iter()
                    .map(|x| {
                        x.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| x.to_string())
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default(),
        FieldKind::Blocks => a.get(key).map(format_blocks).unwrap_or_default(),
        FieldKind::Int | FieldKind::Number => a.get(key).map(num).unwrap_or_default(),
        FieldKind::Text | FieldKind::Choice(_) => match a.get(key) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => field.value.clone(),
            Some(v) => v.to_string(),
        },
    }
}

/// `Deck Door Opens` as `deck_door_opens`; core adds the kind's prefix.
pub fn slug_of(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_string()
}

/// What `POST /glue` needs, and for a timer the settings core takes only
/// once it exists (sent as a `PATCH /glue/{id}` after).
#[derive(Debug, Clone, PartialEq)]
pub struct CreatePlan {
    pub id: String,
    pub name: String,
    pub helper_type: &'static str,
    pub config: Value,
    pub then_patch: Option<Value>,
}

/// The editor for `n` (a kind first, then its settings) and `Enter`.
#[derive(Debug, Clone)]
pub struct HelperEditor {
    /// The helper being changed; `None` when making one.
    pub target: Option<String>,
    /// `None` while the kind is being picked.
    pub helper_type: Option<&'static str>,
    pub type_sel: usize,
    pub name: String,
    pub initial_name: String,
    pub fields: Vec<HelperField>,
    /// 0 is the name; then each field.
    pub focus: usize,
    pub error: Option<String>,
}

impl HelperEditor {
    pub fn new_helper() -> Self {
        Self {
            target: None,
            helper_type: None,
            type_sel: 0,
            name: String::new(),
            initial_name: String::new(),
            fields: Vec::new(),
            focus: 0,
            error: None,
        }
    }

    pub fn edit(d: &DeviceState) -> Self {
        let t = HELPER_TYPES
            .iter()
            .copied()
            .find(|t| *t == d.device_type())
            .unwrap_or("switch");
        let mut fields = fields_for(t, false);
        for f in &mut fields {
            f.value = value_of(d, f);
            f.initial = f.value.clone();
        }
        Self {
            target: Some(d.device_id.clone()),
            helper_type: Some(t),
            type_sel: 0,
            name: d.display_name().to_string(),
            initial_name: d.display_name().to_string(),
            fields,
            focus: 0,
            error: None,
        }
    }

    /// Choose the kind under the cursor and go on to its settings.
    pub fn pick_type(&mut self) {
        let t = HELPER_TYPES[self.type_sel.min(HELPER_TYPES.len() - 1)];
        self.helper_type = Some(t);
        self.fields = fields_for(t, true);
        self.focus = 0;
    }

    /// The `POST /glue` body for a new helper.
    pub fn create_plan(&self) -> Result<CreatePlan, String> {
        let t = self.helper_type.ok_or("choose a kind first")?;
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Give it a name.".into());
        }
        let id = slug_of(name);
        if id.is_empty() {
            return Err("The name needs a letter or a number.".into());
        }
        let mut config = Map::new();
        for f in &self.fields {
            if let Some(v) = f.json()? {
                config.insert(f.key.to_string(), v);
            }
        }
        // Core takes a timer's settings only once it exists.
        let (config, then_patch) = if t == "timer" && !config.is_empty() {
            (Map::new(), Some(Value::Object(config)))
        } else {
            (config, None)
        };
        Ok(CreatePlan {
            id,
            name: name.to_string(),
            helper_type: t,
            config: Value::Object(config),
            then_patch,
        })
    }

    /// The `PATCH /glue/{id}` config: the settings that changed. A
    /// setting emptied is sent as null, which clears it.
    pub fn config_changes(&self) -> Result<Map<String, Value>, String> {
        let mut out = Map::new();
        for f in self.fields.iter().filter(|f| f.changed()) {
            out.insert(f.key.to_string(), f.json()?.unwrap_or(Value::Null));
        }
        Ok(out)
    }

    pub fn name_changed(&self) -> bool {
        self.name.trim() != self.initial_name.trim()
    }
}

/// A value typed for a number, text or date helper (`s`).
#[derive(Debug, Clone)]
pub struct SetValue {
    pub device_id: String,
    pub name: String,
    pub helper_type: String,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct HelpersTab {
    pub editor: Option<HelperEditor>,
    pub set_value: Option<SetValue>,
}

/// Something on the Helpers screen waiting for `y`.
#[derive(Debug, Clone)]
pub enum HelperConfirm {
    Delete { id: String, name: String },
}

/// A timer's own length in seconds: `default_secs`, else `duration_secs`.
pub fn timer_own_secs(d: &DeviceState) -> u64 {
    d.attributes
        .get("default_secs")
        .and_then(Value::as_u64)
        .or_else(|| d.attributes.get("duration_secs").and_then(Value::as_u64))
        .unwrap_or(0)
}

/// What a helper is now, for the State column. The bool says whether it
/// counts as on (for the color), when that means anything.
pub fn helper_state_line(d: &DeviceState) -> (String, Option<bool>) {
    use super::devices::{timer_remaining_secs, timer_state};
    use super::home::clock;
    let a = &d.attributes;
    let b = |k: &str| a.get(k).and_then(Value::as_bool);
    let text = |k: &str| match a.get(k) {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(Value::Null) | None => "-".into(),
        Some(Value::String(_)) => "-".into(),
        Some(v @ Value::Number(_)) => super::rule_words::value_text(v),
        Some(v) => v.to_string(),
    };
    let on_off = |on: bool| (if on { "on" } else { "off" }.to_string(), Some(on));
    match d.device_type() {
        "timer" => match timer_state(d) {
            "running" => (
                format!("running {} left", clock(timer_remaining_secs(d))),
                Some(true),
            ),
            "paused" => (
                format!("paused {} left", clock(timer_remaining_secs(d))),
                Some(false),
            ),
            "finished" => ("finished".into(), Some(false)),
            st => {
                // Idle, or cancelled: ready to start, with its own length.
                let st = if st.is_empty() { "idle" } else { st };
                match timer_own_secs(d) {
                    0 => (st.to_string(), Some(false)),
                    s => (format!("{st} {}", clock(s)), Some(false)),
                }
            }
        },
        "switch" => on_off(b("on").unwrap_or(false)),
        "counter" => (text("count"), None),
        "number" => {
            let mut s = text("value");
            if let Some(u) = a
                .get("unit")
                .and_then(Value::as_str)
                .filter(|u| !u.is_empty())
            {
                s = format!("{s} {u}");
            }
            (s, None)
        }
        "select" => (text("selected"), None),
        "text" | "datetime" => (text("value"), None),
        "button" => match a.get("last_pressed").and_then(Value::as_str) {
            Some(ts) => (format!("pressed {}", crate::ui::lists::age(ts)), None),
            None => ("-".into(), None),
        },
        "group" => {
            let on = b("on").unwrap_or(false);
            let n = |k: &str| a.get(k).and_then(Value::as_u64);
            match (n("active_count"), n("member_count")) {
                (Some(x), Some(y)) if y > 0 => (
                    format!("{} · {x} of {y}", if on { "on" } else { "off" }),
                    Some(on),
                ),
                _ => on_off(on),
            }
        }
        "threshold" => {
            let above = b("above").unwrap_or(false);
            (
                if above { "above" } else { "below" }.to_string(),
                Some(above),
            )
        }
        "schedule" => on_off(b("active").unwrap_or(false)),
        _ => (text("value"), None),
    }
}

/// The Actions column: each kind's keys.
pub fn helper_actions_hint(t: &str) -> &'static str {
    match t {
        "switch" => "Space toggle",
        "timer" => "Space start or pause · 0 cancel",
        "counter" => "+ up · - down · 0 reset",
        "number" => "←→ set · s set to",
        "select" => "Space next · ←→ choose",
        "text" | "datetime" => "s set",
        "button" => "Space press",
        "group" => "follows its members",
        "threshold" => "follows its reading",
        "schedule" => "follows the week",
        _ => "-",
    }
}

impl App {
    fn selected_helper_parts(&self) -> Option<(String, String, String)> {
        self.selected_helper().map(|d| {
            (
                d.device_id.clone(),
                d.display_name().to_string(),
                d.device_type().to_string(),
            )
        })
    }

    /// Space: the kind's main action. A timer with no length of its own
    /// says so instead of failing.
    pub fn helper_space(&mut self) {
        let Some(d) = self.selected_helper() else {
            return;
        };
        if d.device_type() == "timer"
            && !matches!(super::devices::timer_state(d), "running" | "paused")
            && timer_own_secs(d) == 0
        {
            self.status = format!(
                "{} has no length yet: Enter sets how long it runs",
                d.display_name()
            );
            return;
        }
        self.helper_main_action_selected();
    }

    /// Send `{"action": action, ...extra}` to the selected helper.
    pub fn helper_command(&mut self, action: &str, extra: Value, word: &str) {
        let Some((id, name, _)) = self.selected_helper_parts() else {
            return;
        };
        let mut body = json!({ "action": action });
        if let (Some(b), Value::Object(m)) = (body.as_object_mut(), extra) {
            b.extend(m);
        }
        self.command_device(id, body, format!("{name} {word}"));
    }

    /// ←/→ on a select: the previous or next choice.
    pub fn helper_select_step(&mut self, forward: bool) {
        if forward {
            self.helper_command("next", json!({}), "next choice");
        } else {
            self.helper_command("previous", json!({}), "previous choice");
        }
    }

    /// `s`: type a value for a number, text or date helper.
    pub fn open_helper_set_value(&mut self) {
        let Some((id, name, t)) = self.selected_helper_parts() else {
            return;
        };
        self.helpers_tab.set_value = Some(SetValue {
            device_id: id,
            name,
            helper_type: t,
            text: String::new(),
        });
    }

    fn send_helper_value(&mut self) {
        let Some(sv) = self.helpers_tab.set_value.take() else {
            return;
        };
        let raw = sv.text.trim();
        if raw.is_empty() {
            if sv.helper_type == "number" {
                self.status = "Type a number".into();
                self.helpers_tab.set_value = Some(sv);
                return;
            }
            self.command_device(
                sv.device_id.clone(),
                json!({ "action": "clear" }),
                format!("{} cleared", sv.name),
            );
            return;
        }
        let value = if sv.helper_type == "number" {
            match raw.parse::<f64>() {
                Ok(n) => json!(n),
                Err(_) => {
                    self.error = Some("that is not a number".into());
                    self.helpers_tab.set_value = Some(sv);
                    return;
                }
            }
        } else {
            json!(raw)
        };
        self.command_device(
            sv.device_id.clone(),
            json!({ "action": "set", "value": value }),
            format!("{} set to {raw}", sv.name),
        );
    }

    // ── New, edit, delete ────────────────────────────────────────────────

    pub fn open_helper_creator(&mut self) {
        self.helpers_tab.editor = Some(HelperEditor::new_helper());
    }

    pub fn open_helper_editor(&mut self) {
        if let Some(d) = self.selected_helper() {
            self.helpers_tab.editor = Some(HelperEditor::edit(d));
        }
    }

    pub fn ask_delete_helper(&mut self) {
        let Some((id, name, _)) = self.selected_helper_parts() else {
            return;
        };
        let using = self.rules_naming(&id);
        let message = match using.len() {
            0 => format!("{name} will be deleted from homeCore. No automations use it."),
            1 => format!(
                "{name} will be deleted. The automation {} uses it and is turned off until changed.",
                using[0]
            ),
            n => format!(
                "{name} will be deleted. {n} automations use it and are turned off until changed: {}.",
                using.join(", ")
            ),
        };
        self.ask(
            Confirm::new(
                format!("Delete {name}?"),
                message,
                ConfirmAction::Helpers(HelperConfirm::Delete {
                    id: id.clone(),
                    name,
                }),
            )
            .with_detail(format!("ID: {id}")),
        );
    }

    /// The names of the automations that name device `id` anywhere.
    pub fn rules_naming(&self, id: &str) -> Vec<String> {
        let quoted = format!("\"{id}\"");
        let deleted = format!("\"DELETED:{id}\"");
        self.rules
            .iter()
            .filter(|r| {
                let s = serde_json::to_string(r).unwrap_or_default();
                s.contains(&quoted) || s.contains(&deleted)
            })
            .map(|r| r.name.clone())
            .collect()
    }

    pub(crate) fn run_helper_confirm(&mut self, action: HelperConfirm) {
        match action {
            HelperConfirm::Delete { id, name } => {
                let c = self.client.clone();
                let target = id.clone();
                self.spawn_ok(
                    "delete helper",
                    async move { c.delete_glue(&target).await },
                    move |app, ()| {
                        app.devices.retain(|d| d.device_id != id);
                        app.status = format!("Deleted {name}");
                        app.save_cache(&[super::Section::Devices]);
                    },
                );
            }
        }
    }

    fn save_helper_editor(&mut self) {
        let Some(ed) = self.helpers_tab.editor.clone() else {
            return;
        };
        let c = self.client.clone();
        match ed.target.clone() {
            None => {
                let plan = match ed.create_plan() {
                    Ok(p) => p,
                    Err(e) => return self.helper_editor_error(e),
                };
                let full_id = format!("{}_{}", plan.helper_type, plan.id);
                if self.device_by_id(&full_id).is_some() {
                    return self.helper_editor_error(format!(
                        "There is already a {} called that.",
                        type_label(plan.helper_type).to_lowercase()
                    ));
                }
                self.status = format!("Making {}...", plan.name);
                self.spawn(
                    async move {
                        let made = c
                            .create_glue(&plan.id, &plan.name, plan.helper_type, plan.config)
                            .await?;
                        match plan.then_patch {
                            Some(cfg) => c.update_glue(&made.device_id, cfg).await,
                            None => Ok(made),
                        }
                    },
                    |app, res| match res {
                        Ok(dev) => {
                            app.status = format!(
                                "Made {}; rules call it {}",
                                dev.display_name(),
                                dev.device_id
                            );
                            let id = dev.device_id.clone();
                            app.upsert_helper(dev);
                            app.helpers_sel.select_id(id);
                            app.helpers_tab.editor = None;
                        }
                        Err(e) => app.helper_editor_error(e.to_string()),
                    },
                );
            }
            Some(id) => {
                let config = match ed.config_changes() {
                    Ok(c) => c,
                    Err(e) => return self.helper_editor_error(e),
                };
                let name = ed.name.trim().to_string();
                let rename = ed.name_changed();
                if config.is_empty() && !rename {
                    self.helpers_tab.editor = None;
                    self.status = "No changes".into();
                    return;
                }
                self.status = format!("Saving {name}...");
                let (target, new_name) = (id.clone(), name.clone());
                self.spawn(
                    async move {
                        if rename {
                            let mut m = Map::new();
                            m.insert("name".into(), json!(new_name));
                            c.update_device(&target, m).await?;
                        }
                        if config.is_empty() {
                            return Ok(None);
                        }
                        c.update_glue(&target, Value::Object(config))
                            .await
                            .map(Some)
                    },
                    move |app, res| match res {
                        Ok(dev) => {
                            if let Some(dev) = dev {
                                app.upsert_helper(dev);
                            }
                            if rename
                                && let Some(d) = app.devices.iter_mut().find(|d| d.device_id == id)
                            {
                                d.name_override = Some(name.clone());
                            }
                            app.helpers_tab.editor = None;
                            app.status = format!("Saved {name}");
                            app.save_cache(&[super::Section::Devices]);
                        }
                        Err(e) => app.helper_editor_error(e.to_string()),
                    },
                );
            }
        }
    }

    fn helper_editor_error(&mut self, e: String) {
        if let Some(ed) = self.helpers_tab.editor.as_mut() {
            ed.error = Some(e.clone());
        }
        self.error = Some(e);
    }

    fn upsert_helper(&mut self, dev: DeviceState) {
        match self
            .devices
            .iter_mut()
            .find(|d| d.device_id == dev.device_id)
        {
            Some(d) => {
                // Core's answer carries no person's rename; keep ours.
                let keep = d.name_override.clone();
                *d = dev;
                if d.name_override.is_none() {
                    d.name_override = keep;
                }
            }
            None => self.devices.push(dev),
        }
    }

    // ── Keys of the editor and the value box ─────────────────────────────

    /// Keys for the helper editor or the value box. False when neither
    /// is open.
    pub(crate) fn on_key_helpers_modal(&mut self, key: KeyEvent) -> bool {
        if let Some(sv) = self.helpers_tab.set_value.as_mut() {
            match key.code {
                KeyCode::Esc => {
                    self.helpers_tab.set_value = None;
                    self.status = "Cancelled".into();
                }
                KeyCode::Enter => self.send_helper_value(),
                _ => {
                    edit_text(&mut sv.text, key);
                }
            }
            return true;
        }
        let Some(ed) = self.helpers_tab.editor.as_mut() else {
            return false;
        };
        ed.error = None;
        if ed.helper_type.is_none() {
            let n = HELPER_TYPES.len();
            match key.code {
                KeyCode::Esc => {
                    self.helpers_tab.editor = None;
                    self.status = "Cancelled".into();
                }
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                    ed.type_sel = (ed.type_sel + 1) % n
                }
                KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                    ed.type_sel = (ed.type_sel + n - 1) % n
                }
                KeyCode::Enter => ed.pick_type(),
                _ => {}
            }
            return true;
        }
        let n = ed.fields.len() + 1;
        match key.code {
            KeyCode::Esc => {
                self.helpers_tab.editor = None;
                self.status = "Cancelled".into();
            }
            KeyCode::Tab | KeyCode::Down => ed.focus = (ed.focus + 1) % n,
            KeyCode::BackTab | KeyCode::Up => ed.focus = (ed.focus + n - 1) % n,
            KeyCode::Enter => self.save_helper_editor(),
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if ed.focus > 0 => {
                let forward = key.code != KeyCode::Left;
                let f = &mut ed.fields[ed.focus - 1];
                if !f.step(forward) && key.code == KeyCode::Char(' ') {
                    f.value.push(' ');
                }
            }
            _ => {
                let field = if ed.focus == 0 {
                    &mut ed.name
                } else {
                    let f = &mut ed.fields[ed.focus - 1];
                    if matches!(f.kind, FieldKind::Toggle | FieldKind::Choice(_)) {
                        return true;
                    }
                    &mut f.value
                };
                edit_text(field, key);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::model::Role;

    fn helper(ty: &str, attrs: Value) -> DeviceState {
        let mut d = device(&format!("{ty}_x"), "X");
        d.plugin_id = "core.glue".into();
        d.device_type = Some(ty.into());
        d.attributes = attrs.as_object().cloned().unwrap();
        d
    }

    fn making(t: &str, name: &str) -> HelperEditor {
        let mut ed = HelperEditor::new_helper();
        ed.type_sel = HELPER_TYPES.iter().position(|x| *x == t).unwrap();
        ed.pick_type();
        ed.name = name.into();
        ed
    }

    #[test]
    fn every_kind_core_has_is_offered() {
        assert_eq!(HELPER_TYPES.len(), 11);
        for t in HELPER_TYPES {
            assert_ne!(type_label(t), "Helper");
            assert!(!type_what(t).is_empty());
            assert_ne!(helper_actions_hint(t), "-");
        }
    }

    #[test]
    fn a_new_counter_sends_its_settings_with_the_create() {
        let mut ed = making("counter", "Deck Door Opens");
        ed.fields[2].value = "50".into();
        let p = ed.create_plan().unwrap();
        assert_eq!(p.id, "deck_door_opens");
        assert_eq!(p.helper_type, "counter");
        assert_eq!(p.config, json!({"step": 1, "max": 50}));
        assert!(p.then_patch.is_none());
    }

    #[test]
    fn a_new_timer_sends_its_length_after_it_exists() {
        let mut ed = making("timer", "Bathroom Fan");
        ed.fields[0].value = "2.5".into();
        let p = ed.create_plan().unwrap();
        assert_eq!(p.config, json!({}));
        assert_eq!(
            p.then_patch,
            Some(json!({"duration_secs": 150, "repeat": false}))
        );
    }

    #[test]
    fn a_select_needs_choices_and_a_group_sends_members() {
        let mut ed = making("select", "House");
        assert!(ed.create_plan().is_err());
        ed.fields[0].value = "Home, Away ,Vacation".into();
        assert_eq!(
            ed.create_plan().unwrap().config,
            json!({"options": ["Home", "Away", "Vacation"]})
        );
        let mut g = making("group", "Doors");
        g.fields[0].value = "a, b".into();
        let cfg = g.create_plan().unwrap().config;
        assert_eq!(cfg["members"], json!(["a", "b"]));
        assert_eq!(cfg["mode"], "any");
        assert_eq!(cfg["expect"], true);
    }

    #[test]
    fn a_name_is_needed_and_slugged() {
        let ed = making("switch", "  ");
        assert!(ed.create_plan().is_err());
        assert_eq!(slug_of("Guest Mode!"), "guest_mode");
        assert_eq!(slug_of("Ünï 2"), "n_2");
    }

    #[test]
    fn editing_sends_only_what_changed() {
        let d = helper(
            "number",
            json!({"value": 30, "min": 0, "max": 100, "step": 5, "unit": "%"}),
        );
        let mut ed = HelperEditor::edit(&d);
        assert_eq!(ed.config_changes().unwrap(), Map::new());
        let max = ed.fields.iter_mut().find(|f| f.key == "max").unwrap();
        assert_eq!(max.value, "100");
        max.value = "80".into();
        let unit = ed.fields.iter_mut().find(|f| f.key == "unit").unwrap();
        unit.value.clear();
        let c = ed.config_changes().unwrap();
        assert_eq!(Value::Object(c), json!({"max": 80.0, "unit": null}));
    }

    #[test]
    fn a_timer_edit_reads_its_own_length_in_minutes() {
        let d = helper(
            "timer",
            json!({"state": "idle", "duration_secs": 900, "default_secs": 600, "repeat": true}),
        );
        let ed = HelperEditor::edit(&d);
        assert_eq!(ed.fields[0].value, "10");
        assert_eq!(ed.fields[1].value, "yes");
    }

    #[test]
    fn schedule_blocks_round_trip() {
        let v = parse_blocks("Mon-Fri 08:00-17:00; Sat,Sun 10:00-12:00").unwrap();
        assert_eq!(v[0]["days"], json!(["Mon", "Tue", "Wed", "Thu", "Fri"]));
        assert_eq!(v[1]["start"], "10:00");
        assert_eq!(
            format_blocks(&v),
            "Mon,Tue,Wed,Thu,Fri 08:00-17:00; Sat,Sun 10:00-12:00"
        );
        assert!(parse_blocks("Funday 08:00-09:00").is_err());
        assert!(parse_blocks("Mon 8-9").is_err());
        assert_eq!(
            parse_blocks("Sat-Mon 22:00-02:00").unwrap()[0]["days"],
            json!(["Sat", "Sun", "Mon"])
        );
    }

    #[test]
    fn a_running_timer_counts_down_from_when_it_started() {
        let started = (chrono::Utc::now() - chrono::Duration::seconds(30)).to_rfc3339();
        let d = helper(
            "timer",
            json!({"state": "running", "duration_secs": 90, "started_at": started}),
        );
        let (s, on) = helper_state_line(&d);
        assert!(s == "running 1:00 left" || s == "running 0:59 left", "{s}");
        assert_eq!(on, Some(true));
        let idle = helper("timer", json!({"state": "idle", "default_secs": 600}));
        assert_eq!(helper_state_line(&idle).0, "idle 10:00");
    }

    #[test]
    fn state_lines_follow_each_kind() {
        let s = |t, a| helper_state_line(&helper(t, a)).0;
        assert_eq!(s("number", json!({"value": 30, "unit": "%"})), "30 %");
        assert_eq!(s("counter", json!({"count": 2})), "2");
        assert_eq!(s("select", json!({"selected": "Away"})), "Away");
        assert_eq!(
            s(
                "group",
                json!({"on": true, "active_count": 2, "member_count": 3})
            ),
            "on · 2 of 3"
        );
        assert_eq!(s("threshold", json!({"above": false})), "below");
        assert_eq!(s("schedule", json!({"active": true})), "on");
        assert_eq!(s("button", json!({"last_pressed": null})), "-");
    }

    #[test]
    fn the_editor_walks_fields_and_flips_toggles() {
        let mut a = signed_in(Role::Admin);
        a.open_helper_creator();
        a.on_key(key(KeyCode::Char('j')));
        a.on_key(key(KeyCode::Enter));
        let ed = a.helpers_tab.editor.as_ref().unwrap();
        assert_eq!(ed.helper_type, Some("timer"));
        for c in "Fan".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.on_key(key(KeyCode::Tab));
        a.on_key(key(KeyCode::Char('5')));
        a.on_key(key(KeyCode::Tab));
        a.on_key(key(KeyCode::Char(' ')));
        let ed = a.helpers_tab.editor.as_ref().unwrap();
        assert_eq!(ed.name, "Fan");
        assert_eq!(ed.fields[0].value, "5");
        assert_eq!(ed.fields[1].value, "yes");
        a.on_key(key(KeyCode::Esc));
        assert!(a.helpers_tab.editor.is_none());
    }

    #[test]
    fn delete_asks_and_names_the_automations_that_use_it() {
        let mut a = signed_in(Role::Admin);
        a.devices = vec![helper("switch", json!({"on": false}))];
        a.rules = vec![
            serde_json::from_value(json!({
                "id": "r", "name": "Night", "trigger": "ManualTrigger",
                "conditions": [{"DeviceState": {"device_id": "switch_x", "attribute": "on", "op": "Eq", "value": true}}]
            }))
            .unwrap(),
        ];
        a.screen = crate::screens::ScreenId::Helpers;
        a.refresh_views();
        a.on_key(key(KeyCode::Char('x')));
        let c = a.confirm.as_ref().expect("x asks first");
        assert!(c.message.contains("Night"), "{}", c.message);
    }
}
