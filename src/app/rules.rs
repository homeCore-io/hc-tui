//! Rules tab: list, filters, bulk enable/disable, the read-only detail
//! view, the fire-history pane and the groups overlay.

use super::editors::{Confirm, ConfirmAction, RuleFilterBar, RuleFilterField};
use super::{App, ListSel, Load, Section};
use crate::model::{Rule, RuleFiring, RuleGroup};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashSet;

#[derive(Debug)]
pub struct RuleDetail {
    pub rule_id: String,
    pub ron: Load<String>,
    pub history: Load<Vec<RuleFiring>>,
    pub scroll: u16,
}

#[derive(Debug, Default)]
pub struct GroupsOverlay {
    pub items: Load<Vec<RuleGroup>>,
    pub sel: ListSel,
}

#[derive(Debug, Default)]
pub struct RulesTab {
    pub filter_tag: String,
    pub filter_trigger: String,
    pub filter_stale: bool,
    pub filter_bar: Option<RuleFilterBar>,
    /// Rules marked with Space for bulk enable/disable.
    pub marked: HashSet<String>,
    pub sel: ListSel,
    pub detail: Option<RuleDetail>,
    pub groups: Option<GroupsOverlay>,
    /// The hub variables overlay (`v`), when open.
    pub variables: Option<Load<Vec<(String, serde_json::Value)>>>,
    /// Runs, paused ones, missing devices, groups, a dry run and the
    /// device picker: see `automations.rs`.
    pub extra: super::automations::AutomationsExtra,
}

/// Whether `rule` passes the tag / trigger / stale filters. The trigger
/// filter is a case-insensitive substring of any trigger's variant name.
pub fn rule_matches(tab: &RulesTab, rule: &Rule, query: &str) -> bool {
    if !crate::text::matches_query(query, &[&rule.name, &rule.tags.join(" ")]) {
        return false;
    }
    if tab.filter_stale && !tab.extra.needs_fixing(rule) {
        return false;
    }
    if !tab.filter_tag.is_empty() && !rule.tags.iter().any(|t| t.contains(&tab.filter_tag)) {
        return false;
    }
    let want = tab.filter_trigger.trim().to_lowercase();
    if !want.is_empty() && want != "all" {
        let hit = rule
            .trigger_names()
            .iter()
            .any(|n| n.to_lowercase().contains(&want));
        if !hit {
            return false;
        }
    }
    true
}

impl App {
    pub(crate) fn compute_rule_view(&mut self) {
        self.compute_missing();
        let q = self
            .query(crate::screens::ScreenId::Automations)
            .to_string();
        self.views.rules = (0..self.rules.len())
            .filter(|&i| rule_matches(&self.rules_tab, &self.rules[i], &q))
            .collect();
    }

    pub(crate) fn rule_view_ids(&self) -> Vec<String> {
        self.views
            .rules
            .iter()
            .filter_map(|&i| self.rules.get(i))
            .map(|r| r.id.clone())
            .collect()
    }

    pub fn visible_rules(&self) -> impl Iterator<Item = &Rule> {
        self.views.rules.iter().filter_map(|&i| self.rules.get(i))
    }

    pub fn selected_rule(&self) -> Option<&Rule> {
        if let Some(id) = self.rules_tab.sel.id()
            && let Some(r) = self.visible_rules().find(|r| r.id == id)
        {
            return Some(r);
        }
        self.views
            .rules
            .get(self.rules_tab.sel.index())
            .and_then(|&i| self.rules.get(i))
    }

    fn bulk(&self) -> bool {
        !self.rules_tab.marked.is_empty()
    }

    pub fn move_rule(&mut self, delta: isize) {
        let ids = self.rule_view_ids();
        self.rules_tab.sel.move_by(delta, &ids);
    }

    pub fn open_rule_filter_bar(&mut self) {
        self.rules_tab.filter_bar = Some(RuleFilterBar {
            tag: self.rules_tab.filter_tag.clone(),
            trigger: self.rules_tab.filter_trigger.clone(),
            stale: self.rules_tab.filter_stale,
            active_field: RuleFilterField::Tag,
        });
    }

    pub fn toggle_needs_fixing_filter(&mut self) {
        self.rules_tab.filter_stale = !self.rules_tab.filter_stale;
        self.status = if self.rules_tab.filter_stale {
            "Showing automations that need fixing".to_string()
        } else {
            "Showing all automations".to_string()
        };
    }

    /// Esc on the list: clear the marks.
    pub fn rules_back(&mut self) -> bool {
        if self.bulk() {
            self.rules_tab.marked.clear();
            self.status = "Marks cleared".to_string();
            return true;
        }
        false
    }

    /// Space: turn the selected automation on or off. With marked ones,
    /// all of them: on when any is off, else off.
    pub fn toggle_rules(&mut self) {
        if self.bulk() {
            let any_off = self
                .rules
                .iter()
                .any(|r| self.rules_tab.marked.contains(&r.id) && !r.enabled);
            self.set_rules_enabled(any_off);
            return;
        }
        if let Some(id) = self.selected_rule().map(|r| r.id.clone()) {
            self.toggle_rule_id(&id);
        }
    }

    /// Turn one automation on or off, from wherever it is shown.
    pub fn toggle_rule_id(&mut self, id: &str) {
        let Some(rule) = self.rules.iter().find(|r| r.id == id).cloned() else {
            return;
        };
        let enabled = !rule.enabled;
        let verb = if enabled { "on" } else { "off" };
        let c = self.client.clone();
        self.spawn_ok(
            "update automation",
            async move { c.toggle_rule(&rule.id, enabled).await.map(|_| rule) },
            move |app, rule: Rule| {
                if let Some(r) = app.rules.iter_mut().find(|r| r.id == rule.id) {
                    r.enabled = enabled;
                }
                app.status = format!("{} {verb}", rule.name);
                app.save_cache(&[Section::Rules]);
            },
        );
    }

    pub(crate) fn on_key_rule_filter_bar(&mut self, key: KeyEvent) {
        let Some(bar) = self.rules_tab.filter_bar.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.rules_tab.filter_bar = None,
            KeyCode::Tab | KeyCode::Right | KeyCode::BackTab | KeyCode::Left => {
                bar.active_field = match bar.active_field {
                    RuleFilterField::Tag => RuleFilterField::Trigger,
                    RuleFilterField::Trigger => RuleFilterField::Tag,
                };
            }
            KeyCode::Enter => {
                let bar = bar.clone();
                self.rules_tab.filter_bar = None;
                self.rules_tab.filter_tag = bar.tag;
                self.rules_tab.filter_trigger = bar.trigger;
                self.rules_tab.filter_stale = bar.stale;
                self.rules_tab.sel.reset();
                self.status = "Automation filter applied".to_string();
            }
            KeyCode::Backspace => {
                match bar.active_field {
                    RuleFilterField::Tag => bar.tag.pop(),
                    RuleFilterField::Trigger => bar.trigger.pop(),
                };
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                match bar.active_field {
                    RuleFilterField::Tag => bar.tag.push(ch),
                    RuleFilterField::Trigger => bar.trigger.push(ch),
                }
            }
            _ => {}
        }
    }

    pub fn toggle_rule_mark(&mut self) {
        let Some(id) = self.selected_rule().map(|r| r.id.clone()) else {
            return;
        };
        if !self.rules_tab.marked.remove(&id) {
            self.rules_tab.marked.insert(id);
        }
        self.status = format!("{} automation(s) marked", self.rules_tab.marked.len());
    }

    /// Enable or disable the marked rules, or the selected one.
    fn set_rules_enabled(&mut self, enabled: bool) {
        let c = self.client.clone();
        let verb = if enabled { "Enabled" } else { "Disabled" };
        if self.bulk() {
            let ids: Vec<String> = self.rules_tab.marked.iter().cloned().collect();
            let n = ids.len();
            let ids2 = ids.clone();
            self.spawn_ok(
                "bulk update",
                async move { c.bulk_toggle_rules(&ids2, enabled).await },
                move |app, ()| {
                    for r in app.rules.iter_mut().filter(|r| ids.contains(&r.id)) {
                        r.enabled = enabled;
                    }
                    app.rules_tab.marked.clear();
                    app.status = format!("{verb} {n} automation(s)");
                    app.save_cache(&[Section::Rules]);
                },
            );
            return;
        }
        let Some(rule) = self.selected_rule().cloned() else {
            return;
        };
        self.spawn_ok(
            "update automation",
            async move { c.toggle_rule(&rule.id, enabled).await.map(|_| rule) },
            move |app, rule: Rule| {
                if let Some(r) = app.rules.iter_mut().find(|r| r.id == rule.id) {
                    r.enabled = enabled;
                }
                app.status = format!("{verb} {}", rule.name);
                app.save_cache(&[Section::Rules]);
            },
        );
    }

    pub fn clone_selected_rule(&mut self) {
        let Some(id) = self.selected_rule().map(|r| r.id.clone()) else {
            return;
        };
        let c = self.client.clone();
        self.spawn_ok(
            "copy automation",
            async move { c.clone_rule(&id).await },
            |app, cloned: Rule| {
                app.status = format!("Copied to {}", cloned.name);
                app.rules.push(cloned);
                app.save_cache(&[Section::Rules]);
            },
        );
    }

    pub fn ask_delete_rule(&mut self) {
        let Some(rule) = self.selected_rule() else {
            return;
        };
        let (id, name) = (rule.id.clone(), rule.name.clone());
        self.ask(
            Confirm::new(
                "Delete automation?",
                format!("{name} will be deleted from homeCore."),
                ConfirmAction::DeleteRule {
                    id: id.clone(),
                    name,
                },
            )
            .with_detail(format!("ID: {id}")),
        );
    }

    pub(crate) fn delete_rule(&mut self, id: String, name: String) {
        let c = self.client.clone();
        let target = id.clone();
        self.spawn_ok(
            "delete automation",
            async move { c.delete_rule(&target).await },
            move |app, ()| {
                app.rules.retain(|r| r.id != id);
                app.rules_tab.marked.remove(&id);
                app.status = format!("Deleted {name}");
                app.save_cache(&[Section::Rules]);
            },
        );
    }

    // ── Detail view ──────────────────────────────────────────────────────

    pub fn open_rule_detail(&mut self) {
        let Some(id) = self.selected_rule().map(|r| r.id.clone()) else {
            return;
        };
        self.rules_tab.detail = Some(RuleDetail {
            rule_id: id,
            ron: Load::Idle,
            history: Load::Idle,
            scroll: 0,
        });
        self.load_rule_detail();
    }

    /// Fetch RON and history separately, so either can fail on its own.
    pub fn load_rule_detail(&mut self) {
        let Some(d) = self.rules_tab.detail.as_mut() else {
            return;
        };
        d.ron = Load::Loading;
        d.history = Load::Loading;
        let id = d.rule_id.clone();

        let c = self.client.clone();
        let (want, check) = (id.clone(), id.clone());
        self.spawn(
            async move { c.get_rule_ron(&want).await },
            move |app, res| {
                if let Some(d) = app.rules_tab.detail.as_mut()
                    && d.rule_id == check
                {
                    d.ron = match res {
                        Ok(t) => Load::Ready(t),
                        Err(e) => Load::Failed(e.to_string()),
                    };
                }
            },
        );

        let c = self.client.clone();
        let (want, check) = (id.clone(), id);
        self.spawn(
            async move { c.get_rule_history(&want).await },
            move |app, res| {
                if let Some(d) = app.rules_tab.detail.as_mut()
                    && d.rule_id == check
                {
                    d.history = match res {
                        Ok(v) => Load::Ready(newest_first(v)),
                        Err(e) => Load::Failed(e.to_string()),
                    };
                }
            },
        );
    }

    pub fn scroll_rule_detail(&mut self, delta: i32) {
        if let Some(d) = self.rules_tab.detail.as_mut() {
            d.scroll = (d.scroll as i32 + delta).clamp(0, u16::MAX as i32) as u16;
        }
    }

    pub fn close_rule_detail(&mut self) {
        self.rules_tab.detail = None;
    }

    // ── Groups overlay ───────────────────────────────────────────────────

    pub fn open_groups(&mut self) {
        self.rules_tab.groups = Some(GroupsOverlay {
            items: Load::Loading,
            sel: ListSel::default(),
        });
        self.reload_groups();
    }

    fn reload_groups(&mut self) {
        let c = self.client.clone();
        self.spawn(async move { c.list_rule_groups().await }, |app, res| {
            if let Some(g) = app.rules_tab.groups.as_mut() {
                g.items = match res {
                    Ok(v) => Load::Ready(v),
                    Err(e) => Load::Failed(e.to_string()),
                };
            }
        });
    }

    pub fn selected_group(&self) -> Option<&RuleGroup> {
        let g = self.rules_tab.groups.as_ref()?;
        let items = g.items.ready()?;
        match g.sel.id() {
            Some(id) => items.iter().find(|x| x.id == id),
            None => items.get(g.sel.index()),
        }
    }

    pub fn move_group(&mut self, delta: isize) {
        let Some(g) = self.rules_tab.groups.as_mut() else {
            return;
        };
        let ids: Vec<String> = g
            .items
            .ready()
            .map(|v| v.iter().map(|x| x.id.clone()).collect())
            .unwrap_or_default();
        g.sel.move_by(delta, &ids);
    }

    pub fn close_groups(&mut self) {
        self.rules_tab.groups = None;
    }

    /// `v`: the hub variables rules have set, as they are now.
    pub fn open_variables(&mut self) {
        self.rules_tab.variables = Some(Load::Loading);
        self.reload_variables();
    }

    pub fn close_variables(&mut self) {
        self.rules_tab.variables = None;
    }

    /// Ask again, keeping what is shown until the answer comes.
    pub fn reload_variables(&mut self) {
        if self.rules_tab.variables.is_none() {
            return;
        }
        let c = self.client.clone();
        self.spawn(async move { c.list_hub_variables().await }, |app, res| {
            if app.rules_tab.variables.is_some() {
                app.rules_tab.variables = Some(match res {
                    Ok(v) => Load::Ready(v),
                    Err(e) => Load::Failed(e.to_string()),
                });
            }
        });
    }

    pub fn ask_delete_group(&mut self) {
        if let Some(grp) = self.selected_group() {
            let (id, name) = (grp.id.clone(), grp.name.clone());
            self.ask(Confirm::new(
                "Delete group?",
                format!("The group {name} will be deleted. Its automations stay."),
                ConfirmAction::DeleteRuleGroup { id, name },
            ));
        }
    }

    pub fn set_group_enabled(&mut self, enabled: bool) {
        let Some(grp) = self.selected_group() else {
            return;
        };
        let (id, name) = (grp.id.clone(), grp.name.clone());
        let c = self.client.clone();
        let verb = if enabled { "on" } else { "off" };
        self.spawn_ok(
            "update group",
            async move { c.set_rule_group_enabled(&id, enabled).await },
            move |app, ()| {
                app.status = format!("Group {name} {verb}");
                app.start_sync();
            },
        );
    }

    pub(crate) fn delete_rule_group(&mut self, id: String, name: String) {
        let c = self.client.clone();
        let target = id.clone();
        self.spawn_ok(
            "delete group",
            async move { c.delete_rule_group(&target).await },
            move |app, ()| {
                if let Some(g) = app.rules_tab.groups.as_mut()
                    && let Load::Ready(items) = &mut g.items
                {
                    items.retain(|x| x.id != id);
                }
                app.status = format!("Deleted group '{name}'");
            },
        );
    }

    pub(crate) fn resolve_group_selection(&mut self) {
        if let Some(g) = self.rules_tab.groups.as_mut() {
            let ids: Vec<String> = g
                .items
                .ready()
                .map(|v| v.iter().map(|x| x.id.clone()).collect())
                .unwrap_or_default();
            g.sel.resolve(&ids);
        }
    }
}

/// Core returns history oldest first; show newest first.
fn newest_first(mut v: Vec<RuleFiring>) -> Vec<RuleFiring> {
    v.reverse();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rule(trigger: serde_json::Value, triggers: Vec<serde_json::Value>) -> Rule {
        serde_json::from_value(json!({
            "id": "r", "name": "r", "enabled": true, "priority": 0,
            "trigger": trigger, "triggers": triggers
        }))
        .unwrap()
    }

    #[test]
    fn trigger_filter_reads_the_variant_name() {
        let mut tab = RulesTab {
            filter_trigger: "devicestate".into(),
            ..Default::default()
        };
        let r = rule(json!({"DeviceStateChanged": {"device_id": "x"}}), vec![]);
        assert!(rule_matches(&tab, &r, ""));
        let manual = rule(json!("ManualTrigger"), vec![]);
        assert!(!rule_matches(&tab, &manual, ""));
        // Extra triggers count too.
        let multi = rule(
            json!("ManualTrigger"),
            vec![json!({"DeviceStateChanged": {}})],
        );
        assert!(rule_matches(&tab, &multi, ""));
        tab.filter_trigger = "manual".into();
        assert!(rule_matches(&tab, &manual, ""));
    }

    #[test]
    fn stale_filter() {
        let tab = RulesTab {
            filter_stale: true,
            ..Default::default()
        };
        let mut r = rule(json!("ManualTrigger"), vec![]);
        assert!(!rule_matches(&tab, &r, ""));
        r.error = Some("parse error".into());
        assert!(rule_matches(&tab, &r, ""));
    }
}
