//! Parsing tests against JSON shaped like core's real responses
//! (`tests/fixtures`, trimmed and sanitized from a sandbox homeCore).

use crate::app::test_support::signed_in;
use crate::model::{
    Area, Capabilities, DeviceState, MatterNodesResponse, ModeRecord, PluginRecord, RequiresRole,
    Role, Rule, RuleFiring, SystemStatus, TempUnit, merge_scenes,
};
use crate::net::http::parse_body;

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../tests/fixtures/", $name))
    };
}

#[test]
fn devices_parse_with_effective_name_and_room() {
    let devices: Vec<DeviceState> = parse_body("/devices", fixture!("devices.json")).unwrap();
    assert_eq!(devices.len(), 5);

    let porch = devices
        .iter()
        .find(|d| d.device_id == "switch_porch_light")
        .unwrap();
    assert_eq!(porch.display_name(), "Front Porch");
    assert_eq!(porch.name, "Porch Light");
    assert_eq!(porch.room_name(), Some("garage"));

    let lamp = &devices[0];
    assert_eq!(lamp.display_name(), "Desk Lamp");
    assert_eq!(lamp.room_name(), Some("office"));
    assert_eq!(crate::app::devices::device_brightness(lamp), Some(63));

    let timer = devices
        .iter()
        .find(|d| d.device_id == "timer_bathroom")
        .unwrap();
    assert!(!timer.available);
    assert_eq!(
        timer.parent_device_id.as_deref(),
        Some("switch_bathroom_fan")
    );
    assert_eq!(crate::app::devices::timer_remaining_secs(timer), 120);

    let scenes = merge_scenes(vec![], &devices);
    assert_eq!(scenes.len(), 1);
    assert_eq!(scenes[0].id, "lutron_scene_evening");
    assert_eq!(scenes[0].source, crate::model::SceneSource::Device);
}

#[test]
fn thermostat_readings_convert_for_display() {
    let devices: Vec<DeviceState> = parse_body("/devices", fixture!("devices.json")).unwrap();
    let t = devices
        .iter()
        .find(|d| d.device_id == "thermostat_hall")
        .unwrap();
    let mut app = signed_in(Role::Admin);
    let status: SystemStatus =
        parse_body("/system/status", fixture!("system_status.json")).unwrap();
    app.set_system_status(status);
    assert_eq!(app.temp_unit, TempUnit::Fahrenheit);
    // hvac_mode wins over the temperature for the status word.
    assert_eq!(app.device_status(t), "Heat");
    let heat = t.attributes["heat_setpoint"].as_f64().unwrap();
    assert_eq!(app.temp_unit.format(heat), "68.0°F");
}

#[test]
fn rules_and_their_triggers_parse() {
    let rules: Vec<Rule> = parse_body("/automations", fixture!("rules.json")).unwrap();
    assert_eq!(rules.len(), 3);
    assert_eq!(rules[0].trigger_names(), vec!["DeviceStateChanged"]);
    assert_eq!(rules[0].trigger_label(), "DeviceStateChanged");
    assert_eq!(rules[1].trigger_label(), "ManualTrigger");
    assert_eq!(
        rules[2].trigger_names(),
        vec!["SunEvent", "ButtonEvent", "SystemStarted"]
    );
    assert_eq!(rules[2].trigger_label(), "SunEvent +2");
    assert!(rules[2].error.is_some());
}

#[test]
fn rule_history_matches_core_firings() {
    let h: Vec<RuleFiring> =
        parse_body("/automations/x/history", fixture!("rule_history.json")).unwrap();
    assert_eq!(h.len(), 2);
    assert!(!h[0].outcome.fired());
    assert_eq!(h[0].outcome_label(), "condition 1 failed");
    assert!(!h[0].conditions[0].passed);
    assert_eq!(h[0].trigger_type, "DeviceStateChanged");
    assert!(h[1].outcome.fired());
    assert_eq!(h[1].actions_ok(), 1);
    assert_eq!(h[1].actions_failed(), 1);
    assert_eq!(h[1].eval_ms, 4);
}

#[test]
fn matter_nodes_parse_with_extra_fields() {
    let body: MatterNodesResponse =
        parse_body("/plugins/matter/nodes", fixture!("matter_nodes.json")).unwrap();
    assert_eq!(body.nodes.len(), 2);
    assert_eq!(body.nodes[0].node_id, "1");
    assert_eq!(body.nodes[1].node_id, "42", "numeric ids become strings");
    assert_eq!(body.nodes[1].device_count, 1);
    assert_eq!(body.nodes[1].extra["name"], "Kitchen plug");
}

#[test]
fn plugins_parse_the_full_record() {
    let plugins: Vec<PluginRecord> = parse_body("/plugins", fixture!("plugins.json")).unwrap();
    let t = &plugins[0];
    assert_eq!(t.version.as_deref(), Some("0.1.23"));
    assert_eq!(t.installed_version.as_deref(), Some("0.1.13"));
    assert!(t.enabled && t.managed);
    assert_eq!(t.restart_count, 2);
    assert_eq!(t.device_count, 6);
    assert_eq!(t.log_level.as_deref(), Some("info"));
    let n = &plugins[1];
    assert!(!n.enabled);
    assert_eq!(n.notices.len(), 1);
    assert_eq!(n.notices[0].level, "warning");
    assert!(n.notices[0].remedy.is_some());
}

#[test]
fn capabilities_parse_roles_and_danger() {
    let caps: Capabilities =
        parse_body("/plugins/x/capabilities", fixture!("capabilities.json")).unwrap();
    assert_eq!(caps.actions.len(), 3);
    assert!(caps.actions[0].stream);
    assert_eq!(caps.actions[1].requires_role, RequiresRole::SignedIn);
    assert!(!caps.actions[1].danger);
    assert!(caps.actions[2].danger);
}

#[test]
fn event_log_rows_use_custom_event_types() {
    let items: Vec<serde_json::Value> = parse_body("/events", fixture!("events_log.json")).unwrap();
    let rows = crate::app::events::entries_from_log(&items);
    assert_eq!(rows[0].event_type, "webhook");
    assert!(rows[0].custom);
    assert_eq!(rows[1].rule_name.as_deref(), Some("Garage open alert"));
    assert_eq!(rows[2].device_id.as_deref(), Some("thermostat_hall"));
    assert_eq!(rows[2].event_detail.as_deref(), Some("changed=temperature"));
}

#[test]
fn areas_and_modes_parse() {
    let areas: Vec<Area> = parse_body("/areas", fixture!("areas.json")).unwrap();
    assert_eq!(areas[0].slug, "office");
    assert_eq!(areas[1].slug, "");
    let modes: Vec<ModeRecord> = parse_body("/modes", fixture!("modes.json")).unwrap();
    assert_eq!(modes[0].config.on_offset_minutes, Some(-15));
    assert!(modes[1].state.is_none());
}

#[test]
fn grouped_view_uses_room_names() {
    let mut app = signed_in(Role::Admin);
    app.devices = parse_body("/devices", fixture!("devices.json")).unwrap();
    app.areas = parse_body("/areas", fixture!("areas.json")).unwrap();
    app.refresh_views();
    let labels: Vec<&str> = app
        .views
        .device_groups
        .iter()
        .map(|(l, _)| l.as_str())
        .collect();
    // Rooms resolve to area names (by slug, or by name when the slug is
    // missing). The scene device is not listed.
    assert_eq!(labels, vec!["Garage", "Office", "Unassigned"]);
    assert_eq!(app.views.devices.len(), 4);
}

#[test]
fn python_sdk_actions_with_null_fields_parse() {
    // The Python SDK sends `"danger": null` and friends.
    let caps: Capabilities = parse_body(
        "/plugins/plugin.mcp/capabilities",
        r#"{"spec": "1", "plugin_id": "plugin.mcp", "actions": [
            {"id": "chat", "label": "Ask", "stream": true, "danger": null,
             "cancelable": null, "concurrency": null, "requires_role": null,
             "params": {"type": "object", "properties": {"text": {"type": "string"}}}}
        ]}"#,
    )
    .unwrap();
    let a = &caps.actions[0];
    assert!(a.stream && !a.danger && !a.cancelable);
    assert_eq!(a.requires_role, RequiresRole::User);
}

#[test]
fn a_malformed_body_names_the_endpoint() {
    let err = parse_body::<Vec<DeviceState>>("/devices", "{\"oops\":1}").unwrap_err();
    assert!(err.to_string().starts_with("/devices:"), "{err}");
}
