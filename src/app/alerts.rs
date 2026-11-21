//! Alerts: what homeCore says needs looking at. Open and Acknowledged
//! alerts come from `GET /alerts` and stay live from the event stream;
//! Rules are the alert rules (made in the app; here turned on or off and
//! deleted); Ignored lists what someone chose not to see.

use super::editors::{Confirm, ConfirmAction};
use super::{App, ListSel, Load};
use crate::model::Alert;
use crate::net::alerts_api::{AlertIgnore, AlertRule};
use crate::screens::ScreenId;
use crate::text::matches_query;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlertsSub {
    #[default]
    Open,
    Acknowledged,
    Rules,
    /// The kinds of alert plugins raise, each on or off.
    Kinds,
    Ignored,
}

impl AlertsSub {
    pub const ALL: [AlertsSub; 5] = [
        AlertsSub::Open,
        AlertsSub::Acknowledged,
        AlertsSub::Rules,
        AlertsSub::Kinds,
        AlertsSub::Ignored,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Acknowledged => "Acknowledged",
            Self::Rules => "Rules",
            Self::Kinds => "Plugin kinds",
            Self::Ignored => "Ignored",
        }
    }
}

#[derive(Debug, Default)]
pub struct AlertsTab {
    pub sub: AlertsSub,
    pub rules: Load<Vec<AlertRule>>,
    pub rules_sel: ListSel,
    pub ignores: Load<Vec<AlertIgnore>>,
    pub ignores_sel: ListSel,
    pub kinds: Load<Vec<crate::net::alerts_api::AlertKind>>,
    pub kinds_sel: ListSel,
}

/// Something on Alerts waiting for `y`.
#[derive(Debug, Clone)]
pub enum AlertConfirm {
    Press {
        id: String,
        label: String,
    },
    /// `delete_automations`: what becomes of the automations only this
    /// rule starts — deleted, or kept turned off.
    DeleteRule {
        id: String,
        name: String,
        delete_automations: bool,
    },
    RemoveIgnore {
        id: String,
        what: String,
    },
}

/// A kind of alert in words.
pub fn kind_words(kind: &str) -> &'static str {
    match kind {
        "wet" => "something wet",
        "smoke" => "smoke",
        "offline" => "offline",
        "jammed" => "a jammed lock",
        "low_battery" => "low battery",
        "rule" => "an alert rule's alert",
        "plugin" => "a plugin's alert",
        _ => "an alert",
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// An ignore in a sentence: `Low battery on Front Lock, until Nov 25`.
pub fn describe_ignore(
    i: &AlertIgnore,
    name_of: &dyn Fn(&str) -> String,
    rule_of: &dyn Fn(&str) -> Option<String>,
) -> String {
    let device = i.device_id.as_deref().map(name_of);
    let rule = i
        .rule_id
        .as_deref()
        .map(|r| rule_of(r).unwrap_or_else(|| "a deleted rule".into()));
    let mut s = if let Some(rule) = rule {
        match &device {
            Some(d) => format!("\"{rule}\" on {d}"),
            None => format!("Everything \"{rule}\" raises"),
        }
    } else if let Some(p) = &i.plugin_id {
        let what = match &i.topic {
            Some(t) => t.replace('_', " "),
            None => format!("{} alerts", p.trim_start_matches("plugin.")),
        };
        let mut s = capital(&what);
        if let Some(k) = i.key.as_deref().filter(|k| !k.is_empty()) {
            s.push_str(&format!(" about {k}"));
        }
        if let Some(d) = &device {
            s.push_str(&format!(" on {d}"));
        }
        s
    } else if let Some(k) = &i.kind {
        let k = kind_words(k);
        match &device {
            Some(d) => format!("{} on {d}", capital(k)),
            None if k == "smoke" => "Every smoke alert".into(),
            None => format!("Every {k}"),
        }
    } else {
        match &device {
            Some(d) => format!("Everything about {d}"),
            None => "Nothing".into(),
        }
    };
    if let Some(u) = &i.until {
        let when = chrono::DateTime::parse_from_rfc3339(u)
            .map(|t| {
                t.with_timezone(&chrono::Local)
                    .format("%b %-d %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|_| u.clone());
        s.push_str(&format!(", until {when}"));
    }
    s
}

/// An alert rule in a sentence: `Open after 22:00 for 10 min`.
pub fn describe_rule(r: &AlertRule) -> String {
    let mut s = match &r.state_label {
        Some(l) if !l.is_empty() => l.clone(),
        _ => {
            let v = super::rule_words::value_text(&r.value);
            match r.op.as_deref() {
                Some("above") => format!("{} above {v}", r.attribute),
                Some("below") => format!("{} below {v}", r.attribute),
                Some("ne") => format!("{} not {v}", r.attribute),
                _ => format!("{} is {v}", r.attribute),
            }
        }
    };
    let hm = |t: &str| t.get(..5).unwrap_or(t).to_string();
    match (r.after.as_deref(), r.until.as_deref()) {
        (Some(a), Some(u)) => s.push_str(&format!(" between {} and {}", hm(a), hm(u))),
        (Some(a), None) => s.push_str(&format!(" after {}", hm(a))),
        (None, Some(u)) => s.push_str(&format!(" before {}", hm(u))),
        _ => {}
    }
    if let Some(m) = r.for_minutes {
        s.push_str(&format!(" for {m} min"));
    }
    s
}

/// The ignore `i` makes for an alert: everything about its device.
pub fn ignore_for_device(a: &Alert) -> AlertIgnore {
    AlertIgnore {
        device_id: Some(a.device_id.clone()),
        ..Default::default()
    }
}

/// The label of an alert's button, when it has one.
pub fn action_label(a: &Alert) -> Option<String> {
    let act = a.action.as_ref()?;
    act.get("label")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| act.get("id").and_then(Value::as_str).map(str::to_string))
}

impl App {
    /// Open alerts: the sidebar badge.
    pub fn open_alerts(&self) -> usize {
        self.alerts.iter().filter(|a| a.is_open()).count()
    }

    pub(crate) fn compute_alert_view(&mut self) {
        let q = self.query(ScreenId::Alerts).to_string();
        let want_open = self.alerts_tab.sub != AlertsSub::Acknowledged;
        let mut idx: Vec<usize> = (0..self.alerts.len())
            .filter(|&i| {
                let a = &self.alerts[i];
                a.is_open() == want_open && matches_query(&q, &[&a.message, &a.device_id])
            })
            .collect();
        // Most serious first, then newest.
        let rank = |a: &Alert| match a.severity.as_str() {
            "critical" => 0,
            "error" => 1,
            _ => 2,
        };
        idx.sort_by(|&a, &b| {
            let (x, y) = (&self.alerts[a], &self.alerts[b]);
            rank(x).cmp(&rank(y)).then(y.raised_at.cmp(&x.raised_at))
        });
        self.views.alerts = idx;
        let ids = self.alert_rule_ids();
        self.alerts_tab.rules_sel.resolve(&ids);
        let ids = self.alert_ignore_ids();
        self.alerts_tab.ignores_sel.resolve(&ids);
    }

    pub(crate) fn alert_view_ids(&self) -> Vec<String> {
        self.views
            .alerts
            .iter()
            .filter_map(|&i| self.alerts.get(i))
            .map(|a| a.id.clone())
            .collect()
    }

    fn alert_rule_ids(&self) -> Vec<String> {
        self.visible_alert_rules().map(|r| r.id.clone()).collect()
    }

    fn alert_ignore_ids(&self) -> Vec<String> {
        self.alerts_tab
            .ignores
            .ready()
            .map(|v| v.iter().map(|i| i.id.clone()).collect())
            .unwrap_or_default()
    }

    pub fn visible_alerts(&self) -> impl Iterator<Item = &Alert> {
        self.views.alerts.iter().filter_map(|&i| self.alerts.get(i))
    }

    pub fn visible_alert_rules(&self) -> impl Iterator<Item = &AlertRule> {
        let q = self.query(ScreenId::Alerts).to_string();
        self.alerts_tab
            .rules
            .ready()
            .into_iter()
            .flatten()
            .filter(move |r| matches_query(&q, &[&r.name]))
    }

    pub fn selected_alert(&self) -> Option<&Alert> {
        let id = self.alerts_sel.id()?;
        self.visible_alerts().find(|a| a.id == id)
    }

    pub fn selected_alert_rule(&self) -> Option<&AlertRule> {
        let id = self.alerts_tab.rules_sel.id()?;
        self.visible_alert_rules().find(|r| r.id == id)
    }

    pub fn selected_alert_ignore(&self) -> Option<&AlertIgnore> {
        let id = self.alerts_tab.ignores_sel.id()?;
        self.alerts_tab.ignores.ready()?.iter().find(|i| i.id == id)
    }

    /// The alert rule with this id, once loaded.
    pub fn alert_rule(&self, id: &str) -> Option<&AlertRule> {
        self.alerts_tab.rules.ready()?.iter().find(|r| r.id == id)
    }

    pub fn move_alert(&mut self, delta: isize) {
        match self.alerts_tab.sub {
            AlertsSub::Open | AlertsSub::Acknowledged => {
                let ids = self.alert_view_ids();
                self.alerts_sel.move_by(delta, &ids);
            }
            AlertsSub::Rules => {
                let ids = self.alert_rule_ids();
                self.alerts_tab.rules_sel.move_by(delta, &ids);
            }
            AlertsSub::Ignored => {
                let ids = self.alert_ignore_ids();
                self.alerts_tab.ignores_sel.move_by(delta, &ids);
            }
            AlertsSub::Kinds => {
                let ids = self.alert_kind_ids();
                self.alerts_tab.kinds_sel.move_by(delta, &ids);
            }
        }
    }

    fn alert_kind_ids(&self) -> Vec<String> {
        self.alerts_tab
            .kinds
            .ready()
            .map(|v| v.iter().map(|k| k.key()).collect())
            .unwrap_or_default()
    }

    pub fn selected_alert_kind(&self) -> Option<&crate::net::alerts_api::AlertKind> {
        let id = self.alerts_tab.kinds_sel.id()?;
        self.alerts_tab
            .kinds
            .ready()?
            .iter()
            .find(|k| k.key() == id)
    }

    pub fn load_alert_kinds(&mut self) {
        if self.alerts_tab.kinds.ready().is_none() {
            self.alerts_tab.kinds = Load::Loading;
        }
        let c = self.client.clone();
        self.spawn(async move { c.list_alert_kinds().await }, |app, res| {
            app.alerts_tab.kinds = match res {
                Ok(mut v) => {
                    v.sort_by(|a, b| (&a.plugin_id, &a.label).cmp(&(&b.plugin_id, &b.label)));
                    Load::Ready(v)
                }
                Err(e) => Load::Failed(e.to_string()),
            };
            let ids = app.alert_kind_ids();
            app.alerts_tab.kinds_sel.move_by(0, &ids);
        });
    }

    /// Space on a kind: turn it off, or back on. Where it notifies is kept.
    pub fn toggle_alert_kind(&mut self) {
        let Some(k) = self.selected_alert_kind().cloned() else {
            return;
        };
        self.save_alert_kind(k.clone(), Some(!k.enabled), k.notify.clone());
    }

    /// `d` on a kind: back to what the plugin chose, on or off and where.
    pub fn reset_alert_kind(&mut self) {
        let Some(k) = self.selected_alert_kind().cloned() else {
            return;
        };
        self.save_alert_kind(k, None, None);
    }

    fn save_alert_kind(
        &mut self,
        k: crate::net::alerts_api::AlertKind,
        enabled: Option<bool>,
        notify: Option<String>,
    ) {
        let c = self.client.clone();
        let label = k.label.clone();
        self.spawn_ok(
            "save alert kind",
            async move {
                c.set_alert_kind(&k.plugin_id, &k.topic, enabled, notify.as_deref())
                    .await
            },
            move |app, ()| {
                app.status = match enabled {
                    Some(true) => format!("{label}: on"),
                    Some(false) => format!("{label}: off"),
                    None => format!("{label}: back to the plugin's choice"),
                };
                app.load_alert_kinds();
                // A kind turned off clears its alerts.
                app.reload_alerts();
            },
        );
    }

    /// ←/→: the next or previous sub-tab.
    pub fn step_alerts_sub(&mut self, delta: isize) {
        let all = AlertsSub::ALL;
        let i = all
            .iter()
            .position(|s| *s == self.alerts_tab.sub)
            .unwrap_or(0) as isize;
        let n = all.len() as isize;
        self.alerts_tab.sub = all[((i + delta) % n + n) as usize % all.len()];
        if matches!(self.alerts_tab.sub, AlertsSub::Rules | AlertsSub::Ignored) {
            self.load_alert_lists();
        }
        if self.alerts_tab.sub == AlertsSub::Kinds {
            self.load_alert_kinds();
        }
    }

    /// A live `alert_raised` or `alert_acknowledged`.
    pub(crate) fn upsert_alert(&mut self, alert: Alert) {
        match self.alerts.iter_mut().find(|a| a.id == alert.id) {
            Some(a) => *a = alert,
            None => self.alerts.push(alert),
        }
    }

    /// The alerts again: an ignore hides or shows some without saying so.
    pub fn reload_alerts(&mut self) {
        let c = self.client.clone();
        self.spawn(async move { c.list_alerts().await }, |app, res| {
            if let Ok(v) = res {
                app.alerts = v;
            }
        });
    }

    /// The alert rules and the ignore list. Each fails on its own.
    pub fn load_alert_lists(&mut self) {
        if self.can("automations:read") {
            if self.alerts_tab.rules.ready().is_none() {
                self.alerts_tab.rules = Load::Loading;
            }
            let c = self.client.clone();
            self.spawn(async move { c.list_alert_rules().await }, |app, res| {
                app.alerts_tab.rules = match res {
                    Ok(v) => Load::Ready(v),
                    Err(e) => Load::Failed(e.to_string()),
                };
            });
            if self.alerts_tab.ignores.ready().is_none() {
                self.alerts_tab.ignores = Load::Loading;
            }
            let c = self.client.clone();
            self.spawn(async move { c.list_alert_ignores().await }, |app, res| {
                app.alerts_tab.ignores = match res {
                    Ok(v) => Load::Ready(v),
                    Err(e) => Load::Failed(e.to_string()),
                };
            });
        }
    }

    // ── Open and Acknowledged ────────────────────────────────────────────

    /// `a`: seen. It stays, marked, until it clears.
    pub fn acknowledge_selected_alert(&mut self) {
        let Some(a) = self.selected_alert() else {
            return;
        };
        if !a.is_open() {
            self.status = "Already acknowledged".into();
            return;
        }
        let id = a.id.clone();
        let c = self.client.clone();
        self.spawn_ok(
            "acknowledge",
            async move { c.acknowledge_alert(&id).await },
            |app, alert: Alert| {
                app.status = format!("Acknowledged: {}", alert.message);
                app.upsert_alert(alert);
            },
        );
    }

    /// `p`: run the alert's button, asking first when it says to.
    pub fn press_selected_alert(&mut self) {
        let Some(a) = self.selected_alert() else {
            return;
        };
        let Some(label) = action_label(a) else {
            self.status = "This alert has no action".into();
            return;
        };
        let id = a.id.clone();
        let confirm = a
            .action
            .as_ref()
            .and_then(|x| x.get("confirm"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if confirm {
            let message = format!(
                "{label} for {}. It interrupts what the device is doing.",
                a.message
            );
            self.ask(Confirm::new(
                format!("{label}?"),
                message,
                ConfirmAction::Alerts(AlertConfirm::Press { id, label }),
            ));
            return;
        }
        self.press_alert(id, label);
    }

    fn press_alert(&mut self, id: String, label: String) {
        let c = self.client.clone();
        self.spawn_ok(
            "run the alert's action",
            async move { c.press_alert(&id).await },
            move |app, ()| app.status = format!("{label}: sent; the alert clears when it is done"),
        );
    }

    /// `i`: stop seeing alerts about this alert's device.
    pub fn ignore_selected_alert_device(&mut self) {
        let Some(a) = self.selected_alert() else {
            return;
        };
        let ignore = ignore_for_device(a);
        let name = self
            .device_by_id(&a.device_id)
            .map(|d| d.display_name().to_string())
            .unwrap_or_else(|| a.device_id.clone());
        let c = self.client.clone();
        self.spawn_ok(
            "ignore",
            async move { c.create_alert_ignore(&ignore).await },
            move |app, made: AlertIgnore| {
                app.status = format!("Ignoring alerts about {name}; Ignored brings them back");
                if let Load::Ready(v) = &mut app.alerts_tab.ignores {
                    v.push(made);
                }
                app.reload_alerts();
            },
        );
    }

    // ── Rules ────────────────────────────────────────────────────────────

    /// Space on Rules: on or off.
    pub fn toggle_selected_alert_rule(&mut self) {
        let Some(mut rule) = self.selected_alert_rule().cloned() else {
            return;
        };
        rule.enabled = !rule.enabled;
        let word = if rule.enabled { "on" } else { "off" };
        let c = self.client.clone();
        self.spawn_ok(
            "update alert rule",
            async move { c.put_alert_rule(&rule).await },
            move |app, saved: AlertRule| {
                app.status = format!("{} {word}", saved.name);
                if let Load::Ready(v) = &mut app.alerts_tab.rules
                    && let Some(r) = v.iter_mut().find(|r| r.id == saved.id)
                {
                    *r = saved;
                }
            },
        );
    }

    /// `x` on a rule: find what it starts first, so the question can say,
    /// and offer deleting those automations as well as keeping them.
    pub fn ask_delete_alert_rule(&mut self) {
        let Some(r) = self.selected_alert_rule() else {
            return;
        };
        let (id, name) = (r.id.clone(), r.name.clone());
        let c = self.client.clone();
        let target = id.clone();
        self.status = format!("Checking what {name} starts...");
        self.spawn(
            async move { c.alert_rule_automations(&target).await },
            move |app, res| {
                app.status.clear();
                // Unable to ask: say nothing about them, keep them if any.
                let starts = res.unwrap_or_default();
                app.ask(delete_alert_rule_confirm(id, name, &starts));
            },
        );
    }

    // ── Ignored ──────────────────────────────────────────────────────────

    pub fn ignore_text(&self, i: &AlertIgnore) -> String {
        describe_ignore(
            i,
            &|d| {
                self.device_by_id(d)
                    .map(|x| x.display_name().to_string())
                    .unwrap_or_else(|| d.to_string())
            },
            &|r| self.alert_rule(r).map(|x| x.name.clone()),
        )
    }

    pub fn ask_remove_ignore(&mut self) {
        let Some(i) = self.selected_alert_ignore() else {
            return;
        };
        let (id, what) = (i.id.clone(), self.ignore_text(i));
        self.ask(Confirm::new(
            "Show these alerts again?",
            format!("{what} will show again from now."),
            ConfirmAction::Alerts(AlertConfirm::RemoveIgnore { id, what }),
        ));
    }

    pub(crate) fn run_alert_confirm(&mut self, action: AlertConfirm) {
        let c = self.client.clone();
        match action {
            AlertConfirm::Press { id, label } => self.press_alert(id, label),
            AlertConfirm::DeleteRule {
                id,
                name,
                delete_automations,
            } => {
                let target = id.clone();
                self.spawn_ok(
                    "delete alert rule",
                    async move { c.delete_alert_rule(&target, delete_automations).await },
                    move |app, ()| {
                        if let Load::Ready(v) = &mut app.alerts_tab.rules {
                            v.retain(|r| r.id != id);
                        }
                        app.status = format!("Deleted {name}");
                    },
                );
            }
            AlertConfirm::RemoveIgnore { id, what } => {
                let target = id.clone();
                self.spawn_ok(
                    "remove ignore",
                    async move { c.delete_alert_ignore(&target).await },
                    move |app, ()| {
                        if let Load::Ready(v) = &mut app.alerts_tab.ignores {
                            v.retain(|i| i.id != id);
                        }
                        app.status = format!("Showing again: {what}");
                        app.reload_alerts();
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::model::Role;
    use serde_json::json;

    fn alert(v: Value) -> Alert {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn core_alerts_parse_with_their_rule_topic_and_button() {
        let a = alert(json!({
            "id": "plugin:plugin.wled:firmware_update:wled_1:0.15",
            "kind": "plugin", "device_id": "wled_1", "message": "Eaves: firmware 0.15",
            "reason": "firmware 0.15", "severity": "info", "raised_at": "2026-11-17T18:20:24Z",
            "plugin_id": "plugin.wled", "topic": "firmware_update", "key": "0.15",
            "action": {"label": "Update", "id": "install_firmware", "confirm": true},
            "device_plugin_id": "plugin.wled"
        }));
        assert!(a.is_open());
        assert_eq!(a.topic.as_deref(), Some("firmware_update"));
        assert_eq!(action_label(&a).as_deref(), Some("Update"));
        let r = alert(json!({
            "id": "rule:x:garage", "kind": "rule", "device_id": "garage", "rule_id": "x",
            "message": "Garage Door open", "reason": "Open", "severity": "critical",
            "raised_at": "2026-11-17T18:20:24Z",
            "acknowledged": {"by": "john", "at": "2026-11-17T18:21:00Z"}
        }));
        assert!(!r.is_open());
        assert_eq!(r.rule_id.as_deref(), Some("x"));
        assert_eq!(action_label(&r), None);
    }

    #[test]
    fn open_and_acknowledged_split_and_serious_first() {
        let mut a = signed_in(Role::Admin);
        a.alerts = vec![
            alert(
                json!({"id": "1", "message": "Low", "severity": "warning", "raised_at": "2026-11-17T10:00:00Z"}),
            ),
            alert(
                json!({"id": "2", "message": "Smoke", "severity": "critical", "raised_at": "2026-11-17T09:00:00Z"}),
            ),
            alert(
                json!({"id": "3", "message": "Seen", "acknowledged": {"by": "x", "at": "2026-11-17T09:00:00Z"}}),
            ),
        ];
        assert_eq!(a.open_alerts(), 2);
        a.refresh_views();
        assert_eq!(a.alert_view_ids(), vec!["2", "1"]);
        a.alerts_tab.sub = AlertsSub::Acknowledged;
        a.refresh_views();
        assert_eq!(a.alert_view_ids(), vec!["3"]);
    }

    #[test]
    fn live_events_update_the_list_and_the_badge() {
        let mut a = signed_in(Role::Admin);
        a.apply_event(json!({"type": "alert_raised", "alert": {"id": "wet:x", "kind": "wet", "device_id": "x", "message": "Wet"}}));
        assert_eq!(a.open_alerts(), 1);
        a.apply_event(json!({"type": "alert_acknowledged", "alert": {"id": "wet:x", "kind": "wet", "device_id": "x", "message": "Wet", "acknowledged": {"by": "j", "at": "2026-11-17T10:00:00Z"}}}));
        assert_eq!(a.open_alerts(), 0);
        assert_eq!(a.alerts.len(), 1);
        a.apply_event(json!({"type": "alert_cleared", "alert_id": "wet:x"}));
        assert!(a.alerts.is_empty());
        let badge = crate::screens::screen(ScreenId::Alerts).badge(&a);
        assert_eq!(badge, Some(0));
    }

    #[test]
    fn ignores_and_rules_read_as_sentences() {
        let name = |d: &str| {
            if d == "lock" {
                "Front Lock".to_string()
            } else {
                d.to_string()
            }
        };
        let rule = |r: &str| (r == "r1").then(|| "Door left open".to_string());
        let i: AlertIgnore =
            serde_json::from_value(json!({"id": "i", "kind": "low_battery", "device_id": "lock"}))
                .unwrap();
        assert_eq!(
            describe_ignore(&i, &name, &rule),
            "Low battery on Front Lock"
        );
        let i: AlertIgnore =
            serde_json::from_value(json!({"id": "i", "device_id": "lock"})).unwrap();
        assert_eq!(
            describe_ignore(&i, &name, &rule),
            "Everything about Front Lock"
        );
        let i: AlertIgnore = serde_json::from_value(json!({"id": "i", "rule_id": "r1"})).unwrap();
        assert_eq!(
            describe_ignore(&i, &name, &rule),
            "Everything \"Door left open\" raises"
        );
        let r: AlertRule = serde_json::from_value(json!({
            "id": "r", "name": "n", "device_ids": ["d"], "attribute": "open", "value": true,
            "state_label": "Open", "after": "22:00", "for_minutes": 10, "severity": "warning"
        }))
        .unwrap();
        assert_eq!(describe_rule(&r), "Open after 22:00 for 10 min");
    }

    #[test]
    fn an_alert_rule_goes_back_whole_when_switched() {
        let r: AlertRule = serde_json::from_value(json!({
            "id": "r", "name": "n", "enabled": true, "device_ids": ["d"], "attribute": "temperature",
            "value": 50, "op": "above", "severity": "warning", "something_new": 1
        }))
        .unwrap();
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["op"], "above");
        assert_eq!(
            back["something_new"], 1,
            "fields hc-tui does not know survive"
        );
    }

    #[test]
    fn ignoring_names_the_device_only() {
        let a = alert(json!({"id": "1", "kind": "offline", "device_id": "wled_1"}));
        let i = serde_json::to_value(ignore_for_device(&a)).unwrap();
        assert_eq!(i, json!({"id": "", "device_id": "wled_1"}));
    }

    #[test]
    fn arrows_walk_the_sub_tabs() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _g = rt.enter();
        let mut a = signed_in(Role::Admin);
        a.screen = ScreenId::Alerts;
        a.on_key(key(crossterm::event::KeyCode::Right));
        assert_eq!(a.alerts_tab.sub, AlertsSub::Acknowledged);
        a.on_key(key(crossterm::event::KeyCode::Left));
        a.on_key(key(crossterm::event::KeyCode::Left));
        assert_eq!(a.alerts_tab.sub, AlertsSub::Ignored);
    }
}

/// The question `x` on an alert rule asks, given what the rule starts.
///
/// Automations with another trigger run on whatever is chosen, so only the
/// ones this rule alone starts are named: `y` keeps them, turned off, and
/// `d` deletes them too. A rule that starts none is a plain yes or no.
pub(crate) fn delete_alert_rule_confirm(
    id: String,
    name: String,
    starts: &[crate::net::alerts_api::AlertRuleStarts],
) -> Confirm {
    let only: Vec<&str> = starts
        .iter()
        .filter(|s| !s.keeps_running)
        .map(|s| s.name.as_str())
        .collect();
    let action = |delete_automations| {
        ConfirmAction::Alerts(AlertConfirm::DeleteRule {
            id: id.clone(),
            name: name.clone(),
            delete_automations,
        })
    };
    if only.is_empty() {
        return Confirm::new(
            "Delete alert rule?",
            format!("{name} will be deleted and its alerts clear."),
            action(false),
        );
    }
    let what = if only.len() == 1 {
        "1 automation runs only on it".to_string()
    } else {
        format!("{} automations run only on it", only.len())
    };
    Confirm::new(
        "Delete alert rule?",
        format!("{name} will be deleted and its alerts clear. {what}."),
        action(false),
    )
    .with_detail(only.join(", "))
    .with_yes("Delete, keep them turned off")
    .with_alt('d', "Delete them too", action(true))
}

#[cfg(test)]
mod delete_rule_tests {
    use super::*;
    use crate::net::alerts_api::AlertRuleStarts;

    fn starts(name: &str, keeps_running: bool) -> AlertRuleStarts {
        AlertRuleStarts {
            name: name.into(),
            keeps_running,
        }
    }

    fn deletes(a: &ConfirmAction) -> Option<bool> {
        match a {
            ConfirmAction::Alerts(AlertConfirm::DeleteRule {
                delete_automations, ..
            }) => Some(*delete_automations),
            _ => None,
        }
    }

    #[test]
    fn a_rule_that_starts_nothing_is_a_plain_question() {
        let c = delete_alert_rule_confirm("r".into(), "Leak".into(), &[starts("Lights", true)]);
        assert!(
            c.alt.is_none(),
            "an automation with another trigger runs on"
        );
        assert_eq!(deletes(&c.action), Some(false));
    }

    #[test]
    fn automations_it_alone_starts_are_named_and_either_kept_or_deleted() {
        let c = delete_alert_rule_confirm(
            "r".into(),
            "Leak".into(),
            &[starts("Shut valve", false), starts("Lights", true)],
        );
        assert!(c.message.contains("1 automation runs only on it"));
        assert_eq!(c.detail.as_deref(), Some("Shut valve"));
        assert_eq!(deletes(&c.action), Some(false), "y keeps them, turned off");
        let (key, _, alt) = c.alt.as_ref().unwrap();
        assert_eq!(*key, 'd');
        assert_eq!(deletes(alt), Some(true), "d deletes them too");
    }
}
