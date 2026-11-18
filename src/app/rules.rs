//! Rules tab: list, filters, bulk enable/disable, the read-only detail
//! view, the fire-history pane and the groups overlay.

use super::editors::{Confirm, ConfirmAction, RuleFilterBar, RuleFilterField};
use super::{App, ListSel, Load, Section};
use crate::model::{Rule, RuleFiring, RuleGroup};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashSet;

#[derive(Debug)]
pub struct HistoryPane {
    pub rule_id: String,
    pub items: Load<Vec<RuleFiring>>,
}

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
    pub history: Option<HistoryPane>,
    pub detail: Option<RuleDetail>,
    pub groups: Option<GroupsOverlay>,
}

impl RulesTab {
    pub fn filter_active(&self) -> bool {
        self.filter_stale || !self.filter_tag.is_empty() || !self.filter_trigger.is_empty()
    }
}

/// Whether `rule` passes the tag / trigger / stale filters. The trigger
/// filter is a case-insensitive substring of any trigger's variant name.
pub fn rule_matches(tab: &RulesTab, rule: &Rule) -> bool {
    if tab.filter_stale && rule.error.is_none() {
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
    pub(super) fn compute_rule_view(&mut self) {
        self.views.rules = (0..self.rules.len())
            .filter(|&i| rule_matches(&self.rules_tab, &self.rules[i]))
            .collect();
    }

    pub(super) fn rule_view_ids(&self) -> Vec<String> {
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

    pub(super) fn on_key_rules(&mut self, key: KeyEvent) {
        let ids = self.rule_view_ids();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.rules_tab.sel.move_by(1, &ids),
            KeyCode::Up | KeyCode::Char('k') => self.rules_tab.sel.move_by(-1, &ids),
            KeyCode::PageDown => self.rules_tab.sel.move_by(10, &ids),
            KeyCode::PageUp => self.rules_tab.sel.move_by(-10, &ids),
            KeyCode::Enter => self.open_rule_detail(),
            KeyCode::Char('e') | KeyCode::Char('E') => self.set_rules_enabled(true),
            KeyCode::Char('d') | KeyCode::Char('D') => self.set_rules_enabled(false),
            KeyCode::Char('c') | KeyCode::Char('C') => self.clone_selected_rule(),
            KeyCode::Char(' ') => self.toggle_rule_mark(),
            KeyCode::Char('h') | KeyCode::Char('H') => self.open_fire_history(),
            KeyCode::Char('g') | KeyCode::Char('G') => self.open_groups(),
            KeyCode::Char('f') => {
                self.rules_tab.filter_bar = Some(RuleFilterBar {
                    tag: self.rules_tab.filter_tag.clone(),
                    trigger: self.rules_tab.filter_trigger.clone(),
                    stale: self.rules_tab.filter_stale,
                    active_field: RuleFilterField::Tag,
                });
            }
            KeyCode::Char('s') => {
                self.rules_tab.filter_stale = !self.rules_tab.filter_stale;
                self.status = if self.rules_tab.filter_stale {
                    "Filter: showing stale rules only".to_string()
                } else {
                    "Filter: showing all rules".to_string()
                };
            }
            KeyCode::Delete | KeyCode::Char('x') => self.ask_delete_rule(),
            KeyCode::Esc => {
                if self.rules_tab.history.is_some() {
                    self.rules_tab.history = None;
                } else if self.bulk() {
                    self.rules_tab.marked.clear();
                    self.status = "Selection cleared".to_string();
                }
            }
            _ => {}
        }
    }

    pub(super) fn on_key_rule_filter_bar(&mut self, key: KeyEvent) {
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
                self.status = "Rule filter applied".to_string();
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

    fn toggle_rule_mark(&mut self) {
        let Some(id) = self.selected_rule().map(|r| r.id.clone()) else {
            return;
        };
        if !self.rules_tab.marked.remove(&id) {
            self.rules_tab.marked.insert(id);
        }
        self.status = format!("{} rule(s) selected", self.rules_tab.marked.len());
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
                    app.status = format!("{verb} {n} rule(s)");
                    app.save_cache(&[Section::Rules]);
                },
            );
            return;
        }
        let Some(rule) = self.selected_rule().cloned() else {
            return;
        };
        self.spawn_ok(
            "update rule",
            async move { c.toggle_rule(&rule.id, enabled).await.map(|_| rule) },
            move |app, rule: Rule| {
                if let Some(r) = app.rules.iter_mut().find(|r| r.id == rule.id) {
                    r.enabled = enabled;
                }
                app.status = format!("{verb} rule '{}'", rule.name);
                app.save_cache(&[Section::Rules]);
            },
        );
    }

    fn clone_selected_rule(&mut self) {
        let Some(id) = self.selected_rule().map(|r| r.id.clone()) else {
            return;
        };
        let c = self.client.clone();
        self.spawn_ok(
            "clone rule",
            async move { c.clone_rule(&id).await },
            |app, cloned: Rule| {
                app.status = format!("Cloned -> \"{}\"", cloned.name);
                app.rules.push(cloned);
                app.save_cache(&[Section::Rules]);
            },
        );
    }

    fn ask_delete_rule(&mut self) {
        let Some(rule) = self.selected_rule() else {
            return;
        };
        let (id, name) = (rule.id.clone(), rule.name.clone());
        self.ask(
            Confirm::new(
                "Confirm Delete",
                format!("Delete rule: \"{name}\"?"),
                ConfirmAction::DeleteRule {
                    id: id.clone(),
                    name,
                },
            )
            .with_detail(format!("ID: {id}")),
        );
    }

    pub(super) fn delete_rule(&mut self, id: String, name: String) {
        let c = self.client.clone();
        let target = id.clone();
        self.spawn_ok(
            "delete rule",
            async move { c.delete_rule(&target).await },
            move |app, ()| {
                app.rules.retain(|r| r.id != id);
                app.rules_tab.marked.remove(&id);
                app.status = format!("Deleted rule '{name}'");
                app.save_cache(&[Section::Rules]);
            },
        );
    }

    // ── Fire history pane ────────────────────────────────────────────────

    fn open_fire_history(&mut self) {
        let Some(id) = self.selected_rule().map(|r| r.id.clone()) else {
            return;
        };
        self.rules_tab.history = Some(HistoryPane {
            rule_id: id.clone(),
            items: Load::Loading,
        });
        let c = self.client.clone();
        let want = id.clone();
        self.spawn(
            async move { c.get_rule_history(&want).await },
            move |app, res| {
                if let Some(h) = app.rules_tab.history.as_mut()
                    && h.rule_id == id
                {
                    h.items = match res {
                        Ok(v) => Load::Ready(newest_first(v)),
                        Err(e) => Load::Failed(e.to_string()),
                    };
                }
            },
        );
    }

    // ── Detail view ──────────────────────────────────────────────────────

    fn open_rule_detail(&mut self) {
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
    fn load_rule_detail(&mut self) {
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

    pub(super) fn on_key_rule_detail(&mut self, key: KeyEvent) {
        let Some(d) = self.rules_tab.detail.as_mut() else {
            return;
        };
        let scroll = |d: &mut RuleDetail, delta: i32| {
            d.scroll = (d.scroll as i32 + delta).clamp(0, u16::MAX as i32) as u16;
        };
        match key.code {
            KeyCode::Esc => self.rules_tab.detail = None,
            KeyCode::Char('j') | KeyCode::Down => scroll(d, 1),
            KeyCode::Char('k') | KeyCode::Up => scroll(d, -1),
            KeyCode::PageDown => scroll(d, 10),
            KeyCode::PageUp => scroll(d, -10),
            KeyCode::Home => d.scroll = 0,
            KeyCode::Char('r') => self.load_rule_detail(),
            KeyCode::Char('q') => self.should_quit = true,
            _ => {}
        }
    }

    // ── Groups overlay ───────────────────────────────────────────────────

    fn open_groups(&mut self) {
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

    pub(super) fn on_key_groups(&mut self, key: KeyEvent) {
        let Some(g) = self.rules_tab.groups.as_mut() else {
            return;
        };
        let ids: Vec<String> = g
            .items
            .ready()
            .map(|v| v.iter().map(|x| x.id.clone()).collect())
            .unwrap_or_default();
        match key.code {
            KeyCode::Esc => self.rules_tab.groups = None,
            KeyCode::Down | KeyCode::Char('j') => g.sel.move_by(1, &ids),
            KeyCode::Up | KeyCode::Char('k') => g.sel.move_by(-1, &ids),
            KeyCode::Char('e') | KeyCode::Char('E') => self.set_group_enabled(true),
            KeyCode::Char('d') | KeyCode::Char('D') => self.set_group_enabled(false),
            KeyCode::Delete | KeyCode::Char('x') => {
                if let Some(grp) = self.selected_group() {
                    let (id, name) = (grp.id.clone(), grp.name.clone());
                    self.ask(Confirm::new(
                        "Delete group",
                        format!("Delete rule group \"{name}\"? Its rules stay."),
                        ConfirmAction::DeleteRuleGroup { id, name },
                    ));
                }
            }
            _ => {}
        }
    }

    fn set_group_enabled(&mut self, enabled: bool) {
        let Some(grp) = self.selected_group() else {
            return;
        };
        let (id, name) = (grp.id.clone(), grp.name.clone());
        let c = self.client.clone();
        let verb = if enabled { "enabled" } else { "disabled" };
        self.spawn_ok(
            "update group",
            async move { c.set_rule_group_enabled(&id, enabled).await },
            move |app, ()| {
                app.status = format!("Group '{name}' {verb}");
                app.start_sync();
            },
        );
    }

    pub(super) fn delete_rule_group(&mut self, id: String, name: String) {
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

    pub(super) fn resolve_group_selection(&mut self) {
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
        assert!(rule_matches(&tab, &r));
        let manual = rule(json!("ManualTrigger"), vec![]);
        assert!(!rule_matches(&tab, &manual));
        // Extra triggers count too.
        let multi = rule(
            json!("ManualTrigger"),
            vec![json!({"DeviceStateChanged": {}})],
        );
        assert!(rule_matches(&tab, &multi));
        tab.filter_trigger = "manual".into();
        assert!(rule_matches(&tab, &manual));
    }

    #[test]
    fn stale_filter() {
        let tab = RulesTab {
            filter_stale: true,
            ..Default::default()
        };
        let mut r = rule(json!("ManualTrigger"), vec![]);
        assert!(!rule_matches(&tab, &r));
        r.error = Some("parse error".into());
        assert!(rule_matches(&tab, &r));
    }
}
