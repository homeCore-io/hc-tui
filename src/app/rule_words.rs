//! Automations in words: what starts one ("sunset -15m", "Kitchen
//! Motion on"), what it checks and what it does, from core's JSON.
//!
//! Core sends each trigger, check and step as an externally tagged enum:
//! `{"SunEvent": {"event": "Sunset", "offset_minutes": -15}}`, or a bare
//! string for one with no fields (`"ManualTrigger"`).

use crate::model::variant_name;
use serde_json::Value;

/// How a device id reads: its name, or `missing: …` for one homeCore no
/// longer has.
pub type NameOf<'a> = &'a dyn Fn(&str) -> String;

fn fields(v: &Value) -> &Value {
    match v {
        Value::Object(m) if m.len() == 1 => m.values().next().unwrap_or(&Value::Null),
        _ => &Value::Null,
    }
}

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

/// `CivilDusk` as `civil dusk`, `button_1` as `button 1`.
pub fn words(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c == '_' || c == '-' {
            out.push(' ');
        } else if c.is_uppercase() {
            if i > 0 && !out.ends_with(' ') {
                out.push(' ');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A value as a person reads it: `true` as `yes`, strings bare.
pub fn value_text(v: &Value) -> String {
    match v {
        Value::Bool(true) => "yes".into(),
        Value::Bool(false) => "no".into(),
        // A time of day reads as 22:30, not 22:30:00.
        Value::String(s)
            if s.len() >= 8
                && chrono::NaiveTime::parse_from_str(s.get(..8).unwrap_or(""), "%H:%M:%S")
                    .is_ok() =>
        {
            s[..5].to_string()
        }
        Value::String(s) => s.clone(),
        Value::Null => "nothing".into(),
        Value::Number(n) => match n.as_f64() {
            Some(x) if x.fract() == 0.0 => format!("{}", x as i64),
            _ => n.to_string(),
        },
        other => other.to_string(),
    }
}

/// An attribute reaching a value: `on`, `off`, `open`, `closed`, else
/// `state finished`.
fn state_words(attr: &str, to: &Value) -> String {
    match (attr, to) {
        ("on", Value::Bool(b)) => if *b { "on" } else { "off" }.into(),
        ("open", Value::Bool(b)) => if *b { "open" } else { "closed" }.into(),
        ("locked", Value::Bool(b)) => if *b { "locked" } else { "unlocked" }.into(),
        ("motion" | "occupancy", Value::Bool(b)) => if *b { "motion" } else { "no motion" }.into(),
        ("water_detected" | "wet", Value::Bool(b)) => if *b { "wet" } else { "dry" }.into(),
        (a, v) => format!("{} {}", words(a), value_text(v)),
    }
}

/// `600` as `10m`, `90` as `1m 30s`, `7200` as `2h`.
pub fn duration_words(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let mut parts = Vec::new();
    if h > 0 {
        parts.push(format!("{h}h"));
    }
    if m > 0 {
        parts.push(format!("{m}m"));
    }
    if s > 0 || parts.is_empty() {
        parts.push(format!("{s}s"));
    }
    parts.join(" ")
}

fn hhmm(t: &str) -> String {
    t.get(..5).unwrap_or(t).to_string()
}

fn device_list(v: &Value, name_of: NameOf<'_>) -> Option<String> {
    let mut ids: Vec<&str> = Vec::new();
    if let Some(id) = s(v, "device_id").filter(|x| !x.is_empty()) {
        ids.push(id);
    }
    if let Some(list) = v.get("device_ids").and_then(Value::as_array) {
        for id in list.iter().filter_map(Value::as_str) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    match ids.as_slice() {
        [] => None,
        [one] => Some(name_of(one)),
        [first, rest @ ..] => Some(format!("{} +{}", name_of(first), rest.len())),
    }
}

fn filter_words(f: &Value, name_of: NameOf<'_>) -> String {
    if let Some(d) = device_list(f, name_of) {
        return d;
    }
    if s(f, "rule_id").is_some() {
        return "an alert rule".into();
    }
    if let Some(t) = s(f, "topic") {
        return words(t);
    }
    if let Some(k) = s(f, "kind") {
        return words(k);
    }
    if let Some(p) = s(f, "plugin_id") {
        return p.trim_start_matches("plugin.").to_string();
    }
    "any".into()
}

/// One trigger in a few words, for the list.
pub fn trigger_words(t: &Value, name_of: NameOf<'_>) -> String {
    let Some(kind) = variant_name(t) else {
        return "-".into();
    };
    let f = fields(t);
    let num = |k: &str| f.get(k).and_then(Value::as_i64);
    let after = |base: String| match f.get("for_duration_secs").and_then(Value::as_u64) {
        Some(secs) if secs > 0 => format!("{base} {}", duration_words(secs)),
        _ => base,
    };
    match kind {
        "SunEvent" => {
            let ev = words(s(f, "event").unwrap_or("sun"));
            match num("offset_minutes").unwrap_or(0) {
                0 => ev,
                n if n < 0 => format!("{ev} -{}m", -n),
                n => format!("{ev} +{n}m"),
            }
        }
        "TimeOfDay" => {
            let mut out = format!("time {}", hhmm(s(f, "time").unwrap_or("?")));
            let days: Vec<&str> = f
                .get("days")
                .and_then(Value::as_array)
                .map(|d| d.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if !days.is_empty() && days.len() < 7 {
                out.push(' ');
                out.push_str(&days.join(","));
            }
            out
        }
        "DeviceStateChanged" => {
            let who = device_list(f, name_of).unwrap_or_else(|| "a device".into());
            let base = match (s(f, "attribute"), f.get("to")) {
                (Some(a), Some(to)) if !to.is_null() => format!("{who} {}", state_words(a, to)),
                (Some(a), _) if a != "on" && a != "state" => format!("{who} {}", words(a)),
                _ => who,
            };
            after(base)
        }
        "DeviceAvailabilityChanged" => {
            let who = device_list(f, name_of).unwrap_or_else(|| "a device".into());
            let base = match f.get("to").and_then(Value::as_bool) {
                Some(true) => format!("{who} online"),
                Some(false) => format!("{who} offline"),
                None => format!("{who} online or offline"),
            };
            after(base)
        }
        "ButtonEvent" => {
            let who = device_list(f, name_of).unwrap_or_else(|| "a button".into());
            let ev = words(s(f, "event").unwrap_or("pushed"));
            match num("button_number") {
                Some(n) => format!("{who} button {n} {ev}"),
                None => format!("{who} {ev}"),
            }
        }
        "NumericThreshold" => {
            let who = device_list(f, name_of).unwrap_or_else(|| "a device".into());
            let v = f.get("value").map(value_text).unwrap_or_default();
            after(format!(
                "{who} {} {} {v}",
                words(s(f, "attribute").unwrap_or("value")),
                words(s(f, "op").unwrap_or("above")),
            ))
        }
        "ManualTrigger" => "run by hand".into(),
        "SystemStarted" => "homeCore starts".into(),
        "Cron" => format!("cron {}", s(f, "expression").unwrap_or("?")),
        "Periodic" => {
            let n = f.get("every_n").and_then(Value::as_u64).unwrap_or(1);
            let unit = s(f, "unit").unwrap_or("Minutes").to_lowercase();
            if n == 1 {
                format!("every {}", unit.trim_end_matches('s'))
            } else {
                format!("every {n} {unit}")
            }
        }
        "MqttMessage" => format!("MQTT {}", s(f, "topic_pattern").unwrap_or("?")),
        "WebhookReceived" => format!("webhook {}", s(f, "path").unwrap_or("?")),
        "CustomEvent" => format!("event {}", s(f, "event_type").unwrap_or("?")),
        "HubVariableChanged" => match s(f, "name") {
            Some(n) => format!("value {n} changes"),
            None => "a value changes".into(),
        },
        "ModeChanged" => {
            let m = s(f, "mode_id")
                .map(name_of)
                .unwrap_or_else(|| "a mode".into());
            match f.get("to").and_then(Value::as_bool) {
                Some(true) => format!("{m} on"),
                Some(false) => format!("{m} off"),
                None => format!("{m} changes"),
            }
        }
        "CalendarEvent" => match s(f, "title_contains") {
            Some(t) => format!("calendar \"{t}\""),
            None => "a calendar event".into(),
        },
        "DeviceBatteryLow" | "DeviceBatteryRecovered" => {
            let who = s(f, "device_id")
                .map(name_of)
                .unwrap_or_else(|| "any device".into());
            if kind == "DeviceBatteryLow" {
                format!("{who} battery low")
            } else {
                format!("{who} battery back")
            }
        }
        "AlertRaised" | "AlertCleared" | "AlertAcknowledged" | "AlertActiveFor" => {
            let what = f
                .get("filter")
                .map(|x| filter_words(x, name_of))
                .unwrap_or_else(|| "any".into());
            match kind {
                "AlertRaised" => format!("alert: {what}"),
                "AlertCleared" => format!("alert cleared: {what}"),
                "AlertAcknowledged" => format!("alert seen: {what}"),
                _ => format!(
                    "alert {}m: {what}",
                    f.get("minutes").and_then(Value::as_u64).unwrap_or(0)
                ),
            }
        }
        other => words(other),
    }
}

/// Every trigger: the first, and how many more.
pub fn triggers_short(all: &[&Value], name_of: NameOf<'_>) -> String {
    match all {
        [] => "-".into(),
        [one] => trigger_words(one, name_of),
        [first, rest @ ..] => format!("{} +{}", trigger_words(first, name_of), rest.len()),
    }
}

fn op_words(op: &str) -> &'static str {
    match op {
        "Eq" => "is",
        "Ne" => "is not",
        "Gt" => "above",
        "Gte" => "at least",
        "Lt" => "below",
        "Lte" => "at most",
        "Contains" => "has",
        _ => "is",
    }
}

/// One check in a few words: `Vacation is off`.
pub fn condition_words(c: &Value, name_of: NameOf<'_>) -> String {
    let Some(kind) = variant_name(c) else {
        return "?".into();
    };
    let f = fields(c);
    match kind {
        "DeviceState" => {
            let who = s(f, "device_id").map(name_of).unwrap_or_default();
            let attr = s(f, "attribute").unwrap_or("");
            let op = s(f, "op").unwrap_or("Eq");
            let v = f.get("value").cloned().unwrap_or(Value::Null);
            if matches!(op, "Eq" | "Ne") && v.is_boolean() {
                let st = state_words(attr, &v);
                if op == "Eq" {
                    format!("{who} is {st}")
                } else {
                    format!("{who} is not {st}")
                }
            } else {
                format!("{who} {} {} {}", words(attr), op_words(op), value_text(&v))
            }
        }
        "ModeIs" => {
            let m = s(f, "mode_id").map(name_of).unwrap_or_default();
            let on = f.get("on").and_then(Value::as_bool).unwrap_or(true);
            format!("{m} is {}", if on { "on" } else { "off" })
        }
        "TimeWindow" => format!(
            "between {} and {}",
            hhmm(s(f, "start").unwrap_or("?")),
            hhmm(s(f, "end").unwrap_or("?"))
        ),
        "Or" | "And" => {
            let joiner = if kind == "Or" { " or " } else { " and " };
            let parts: Vec<String> = f
                .get("conditions")
                .and_then(Value::as_array)
                .map(|l| l.iter().map(|x| condition_words(x, name_of)).collect())
                .unwrap_or_default();
            format!("({})", parts.join(joiner))
        }
        "Not" => match f.get("condition") {
            Some(inner) => format!("not {}", condition_words(inner, name_of)),
            None => "not ?".into(),
        },
        "ScriptExpression" => "a script says yes".into(),
        "NoAlertActive" => {
            let what = f
                .get("filter")
                .map(|x| filter_words(x, name_of))
                .unwrap_or_else(|| "any".into());
            format!("no alert on {what}")
        }
        other => words(other),
    }
}

/// What a step sends a device: `on`, `off 60%`, `activate`.
fn state_text(state: &Value) -> String {
    let Some(m) = state.as_object() else {
        return value_text(state);
    };
    let mut parts = Vec::new();
    if let Some(a) = m.get("action").and_then(Value::as_str) {
        parts.push(words(a));
    }
    if let Some(on) = m.get("on").and_then(Value::as_bool) {
        parts.push(if on { "on" } else { "off" }.into());
    }
    if let Some(b) = m.get("brightness_pct") {
        parts.push(format!("{}%", value_text(b)));
    }
    for (k, v) in m {
        if matches!(
            k.as_str(),
            "action" | "on" | "brightness_pct" | "transition_ms" | "transition"
        ) {
            continue;
        }
        if parts.len() >= 3 {
            break;
        }
        parts.push(format!("{} {}", words(k), value_text(v)));
    }
    parts.join(" ")
}

/// One step in a few words: `Porch Light on 60%`, `wait 1m`.
pub fn action_words(step: &Value, name_of: NameOf<'_>) -> String {
    // A rule's step is `{"action": {...}, "enabled": bool}`; the inner
    // lists of Parallel and the like hold the bare action.
    let (action, enabled) = match step.get("action") {
        Some(a) if step.get("enabled").is_some() || a.is_object() => (
            a,
            step.get("enabled").and_then(Value::as_bool).unwrap_or(true),
        ),
        _ => (step, true),
    };
    let Some(kind) = variant_name(action) else {
        return "?".into();
    };
    let f = fields(action);
    let list = |k: &str| -> Vec<String> {
        f.get(k)
            .and_then(Value::as_array)
            .map(|l| l.iter().map(|x| action_words(x, name_of)).collect())
            .unwrap_or_default()
    };
    let text = match kind {
        "SetDeviceState" => {
            let who = s(f, "device_id").map(name_of).unwrap_or_default();
            let st = f.get("state").map(state_text).unwrap_or_default();
            format!("{who} {st}").trim().to_string()
        }
        "Delay" => {
            let secs = f
                .get("duration_secs")
                .and_then(Value::as_u64)
                .or_else(|| {
                    f.get("duration_ms")
                        .and_then(Value::as_u64)
                        .map(|m| m / 1000)
                })
                .unwrap_or(0);
            format!("wait {}", duration_words(secs))
        }
        "Notify" => format!(
            "tell {}: {}",
            s(f, "channel").unwrap_or("all"),
            crate::text::take_chars(s(f, "message").unwrap_or(""), 40)
        ),
        "Parallel" => format!("together: {}", list("actions").join(", ")),
        "Conditional" => {
            let then = list("then_actions");
            format!("if a check holds: {}", then.join(", "))
        }
        "ActivateScene" => format!(
            "scene {}",
            s(f, "scene_id").or(s(f, "scene_name")).unwrap_or("?")
        ),
        "LogMessage" => "write to the log".into(),
        "RunScript" => "run a script".into(),
        "PauseRule" => "pause an automation".into(),
        "ResumeRule" => "resume an automation".into(),
        "SetMode" => {
            let m = s(f, "mode_id").map(name_of).unwrap_or_default();
            match f.get("on").and_then(Value::as_bool) {
                Some(on) => format!("{m} {}", if on { "on" } else { "off" }),
                None => m,
            }
        }
        "PressAlert" => "press the alert's button".into(),
        other => words(other),
    };
    if enabled {
        text
    } else {
        format!("{text} (off)")
    }
}

/// Every device id a rule names (`device_id`, `device_ids`, `members`,
/// `source_device_id`), anywhere in it.
pub fn device_refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                match (k.as_str(), x) {
                    ("device_id" | "source_device_id", Value::String(id)) => {
                        if !out.contains(id) {
                            out.push(id.clone());
                        }
                    }
                    ("device_ids", Value::Array(l)) => {
                        for id in l.iter().filter_map(Value::as_str) {
                            if !out.iter().any(|o| o == id) {
                                out.push(id.to_string());
                            }
                        }
                    }
                    _ => device_refs(x, out),
                }
            }
        }
        Value::Array(l) => l.iter().for_each(|x| device_refs(x, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn names(id: &str) -> String {
        match id {
            "motion" => "Kitchen Motion".into(),
            "garage" => "Garage Door".into(),
            "porch" => "Porch Light".into(),
            "vac" => "Vacation".into(),
            "mode_night" => "Night".into(),
            other if other.starts_with("DELETED:") => format!("missing: {}", &other[8..]),
            other => other.into(),
        }
    }

    fn t(v: Value) -> String {
        trigger_words(&v, &names)
    }

    #[test]
    fn triggers_read_like_the_app() {
        assert_eq!(
            t(json!({"SunEvent": {"event": "Sunset", "offset_minutes": -15}})),
            "sunset -15m"
        );
        assert_eq!(
            t(json!({"SunEvent": {"event": "CivilDawn", "offset_minutes": 10}})),
            "civil dawn +10m"
        );
        assert_eq!(
            t(json!({"TimeOfDay": {"time": "17:00:00", "days": []}})),
            "time 17:00"
        );
        assert_eq!(
            t(json!({"TimeOfDay": {"time": "07:30:00", "days": ["Sat", "Sun"]}})),
            "time 07:30 Sat,Sun"
        );
        assert_eq!(
            t(json!({"DeviceStateChanged": {"device_id": "motion", "attribute": "on"}})),
            "Kitchen Motion"
        );
        assert_eq!(
            t(
                json!({"DeviceStateChanged": {"device_id": "garage", "attribute": "open", "to": true, "for_duration_secs": 600}})
            ),
            "Garage Door open 10m"
        );
        assert_eq!(
            t(
                json!({"DeviceStateChanged": {"device_id": "garage", "device_ids": ["garage", "porch"], "attribute": "button_1", "to": "press"}})
            ),
            "Garage Door +1 button 1 press"
        );
        assert_eq!(t(json!("ManualTrigger")), "run by hand");
        assert_eq!(t(json!("SystemStarted")), "homeCore starts");
        assert_eq!(
            t(
                json!({"ButtonEvent": {"device_id": "porch", "button_number": 2, "event": "DoubleTapped"}})
            ),
            "Porch Light button 2 double tapped"
        );
        assert_eq!(
            t(json!({"Periodic": {"every_n": 5, "unit": "Minutes"}})),
            "every 5 minutes"
        );
        assert_eq!(
            t(json!({"AlertRaised": {"filter": {"kind": "wet", "plugin_id": "plugin.yolink"}}})),
            "alert: wet"
        );
        assert_eq!(
            t(json!({"ModeChanged": {"mode_id": "mode_night", "to": true}})),
            "Night on"
        );
        assert_eq!(
            t(json!({"DeviceStateChanged": {"device_id": "DELETED:fan", "attribute": "on"}})),
            "missing: fan"
        );
        assert_eq!(t(json!({"SomethingNew": {}})), "something new");
    }

    #[test]
    fn several_triggers_count_the_rest() {
        let a = json!("ManualTrigger");
        let b = json!("SystemStarted");
        assert_eq!(triggers_short(&[&a, &b], &names), "run by hand +1");
        assert_eq!(triggers_short(&[], &names), "-");
    }

    #[test]
    fn checks_and_steps_read_like_the_app() {
        let c = |v: Value| condition_words(&v, &names);
        assert_eq!(
            c(
                json!({"DeviceState": {"device_id": "vac", "attribute": "on", "op": "Eq", "value": false}})
            ),
            "Vacation is off"
        );
        assert_eq!(
            c(
                json!({"DeviceState": {"device_id": "motion", "attribute": "humidity", "op": "Gte", "value": 35}})
            ),
            "Kitchen Motion humidity at least 35"
        );
        assert_eq!(
            c(json!({"TimeWindow": {"start": "06:00:00", "end": "12:00:00"}})),
            "between 06:00 and 12:00"
        );
        assert_eq!(
            c(json!({"ModeIs": {"mode_id": "mode_night", "on": true}})),
            "Night is on"
        );
        let a = |v: Value| action_words(&v, &names);
        assert_eq!(
            a(
                json!({"action": {"SetDeviceState": {"device_id": "porch", "state": {"on": true, "brightness_pct": 60}}}, "enabled": true})
            ),
            "Porch Light on 60%"
        );
        assert_eq!(
            a(json!({"action": {"Delay": {"duration_secs": 60}}, "enabled": false})),
            "wait 1m (off)"
        );
        assert_eq!(
            a(json!({"action": {"Parallel": {"actions": [
                {"SetDeviceState": {"device_id": "porch", "state": {"action": "activate"}}},
                {"SetDeviceState": {"device_id": "garage", "state": {"on": false}}}
            ]}}, "enabled": true})),
            "together: Porch Light activate, Garage Door off"
        );
    }

    #[test]
    fn device_refs_find_every_device_a_rule_names() {
        let rule = json!({
            "trigger": {"DeviceStateChanged": {"device_id": "a", "device_ids": ["a", "b"]}},
            "actions": [{"action": {"Parallel": {"actions": [{"SetDeviceState": {"device_id": "DELETED:c"}}]}}}]
        });
        let mut out = Vec::new();
        device_refs(&rule, &mut out);
        out.sort();
        assert_eq!(out, vec!["DELETED:c", "a", "b"]);
    }

    #[test]
    fn durations_are_short() {
        assert_eq!(duration_words(600), "10m");
        assert_eq!(duration_words(90), "1m 30s");
        assert_eq!(duration_words(7200), "2h");
        assert_eq!(duration_words(0), "0s");
    }
}

/// A criteria mode's definition in words: when it goes on, when it goes
/// off, and how often it is checked. `None` when it has none.
pub fn criteria_words(definition: &Value, name_of: NameOf<'_>) -> Option<(String, String, String)> {
    let c = definition.get("criteria")?;
    let on = condition_words(c.get("on_condition")?, name_of);
    let off = match c.get("off_behavior").and_then(Value::as_str) {
        Some("explicit") => match c.get("off_condition").filter(|v| !v.is_null()) {
            Some(oc) => condition_words(oc, name_of),
            None => "when its off condition holds".to_string(),
        },
        _ => "when that stops being true".to_string(),
    };
    let every = match c.get("reevaluate_every_n_minutes").and_then(Value::as_u64) {
        Some(0) | None => "as things change".to_string(),
        Some(1) => "as things change, and every minute".to_string(),
        Some(n) => format!("as things change, and every {n} minutes"),
    };
    Some((on, off, every))
}

#[cfg(test)]
mod criteria_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_criteria_mode_reads_as_when_it_goes_on_and_off() {
        let d = json!({"criteria": {
            "on_condition": {"DeviceState": {"device_id": "t1", "attribute": "temperature", "op": "Gt", "value": 22}},
            "off_behavior": "inverse",
            "reevaluate_every_n_minutes": 5
        }, "generated_rule_ids": []});
        let (on, off, every) = criteria_words(&d, &|_| "Indoor Sensor".to_string()).unwrap();
        assert!(on.contains("Indoor Sensor"), "{on}");
        assert_eq!(off, "when that stops being true");
        assert_eq!(every, "as things change, and every 5 minutes");
        assert!(criteria_words(&json!(null), &|s| s.to_string()).is_none());
    }
}
