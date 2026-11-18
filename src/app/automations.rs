//! Automations beyond the list: each one's recent runs and last run,
//! the ones another rule paused, the devices they name that homeCore no
//! longer has, groups, run now, a dry run at a time, and sending a rule
//! to another device.

use super::editors::{Confirm, ConfirmAction, edit_text};
use super::rule_words::{action_words, condition_words, device_refs, value_text};
use super::{App, Load};
use crate::model::{Rule, RuleFiring, RuleGroup};
use crate::net::automations_api::TestResult;
use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// A dry run: the time box, then what it found.
#[derive(Debug, Clone)]
pub struct TestPane {
    pub rule_id: String,
    pub name: String,
    /// What is being typed: `22:30`, `2026-11-18T07:00`, empty for now.
    pub input: String,
    /// The box is open (before the first run, or after `t` again).
    pub editing: bool,
    pub asked: String,
    pub result: Load<TestResult>,
    pub scroll: u16,
}

/// `F`: a device to send a rule's missing one to.
#[derive(Debug, Clone)]
pub struct FixPicker {
    pub rule_id: String,
    pub name: String,
    /// The ids it names that homeCore has no device for.
    pub missing: Vec<String>,
    /// Which of them is being fixed.
    pub which: usize,
    pub query: String,
    pub sel: usize,
}

#[derive(Debug, Default)]
pub struct AutomationsExtra {
    /// Each rule's recent runs, newest first (core keeps the last 20).
    pub runs: HashMap<String, Vec<RuleFiring>>,
    pub runs_loaded: bool,
    /// Rules another rule has paused.
    pub paused: HashSet<String>,
    /// `GET /automations/stale-refs`: rule id to the ids it lacks.
    pub stale: HashMap<String, Vec<String>>,
    /// Removed devices' names, for a rule that still names one.
    pub removed: HashMap<String, String>,
    pub groups: Vec<RuleGroup>,
    /// Missing devices per rule, worked out with each view.
    pub missing: HashMap<String, Vec<String>>,
    pub test: Option<TestPane>,
    pub fix: Option<FixPicker>,
}

impl AutomationsExtra {
    /// It can't run as written: core says why, or it names a device
    /// homeCore does not have.
    pub fn needs_fixing(&self, r: &Rule) -> bool {
        r.error.is_some() || self.missing.get(&r.id).is_some_and(|m| !m.is_empty())
    }
}

/// Something on Automations waiting for `y`.
#[derive(Debug, Clone)]
pub enum AutomationConfirm {
    Run {
        id: String,
        name: String,
    },
    Repoint {
        from: String,
        to: String,
        to_name: String,
    },
}

/// How a line of a run or a test reads: good, bad, or quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Good,
    Bad,
    Quiet,
}

/// A timestamp for a short column: `2h ago`, `yesterday`, `Nov 3`,
/// `Dec 2025`.
pub fn when_text(ts: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    use chrono::{Datelike, Local};
    let Ok(t) = chrono::DateTime::parse_from_rfc3339(ts) else {
        return "-".into();
    };
    let t = t.with_timezone(&chrono::Utc);
    let secs = (now - t).num_seconds().max(0);
    let (lt, ln) = (t.with_timezone(&Local), now.with_timezone(&Local));
    match secs {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 && lt.date_naive() == ln.date_naive() => format!("{}h ago", s / 3600),
        _ if ln.date_naive().pred_opt() == Some(lt.date_naive()) => "yesterday".into(),
        _ if lt.year() == ln.year() => lt.format("%b %-d").to_string(),
        _ => lt.format("%b %Y").to_string(),
    }
}

/// One recent run as a line: when, the outcome, and why it stopped or
/// what it did.
pub fn run_line(
    f: &RuleFiring,
    rule: Option<&Rule>,
    name_of: super::rule_words::NameOf<'_>,
    utc: bool,
) -> (String, Tone) {
    let at = match chrono::DateTime::parse_from_rfc3339(&f.timestamp) {
        Ok(t) if utc => t
            .with_timezone(&chrono::Utc)
            .format("%b %-d %H:%M")
            .to_string(),
        Ok(t) => t
            .with_timezone(&chrono::Local)
            .format("%b %-d %H:%M")
            .to_string(),
        Err(_) => crate::text::take_chars(&f.timestamp, 12),
    };
    if f.outcome.fired() {
        let ok = f.actions_ok();
        let failed = f.actions_failed();
        let checks = if f.conditions.is_empty() {
            "no checks".to_string()
        } else {
            "conditions passed".to_string()
        };
        let mut s = format!(
            "{at:<13} ran       {checks:<18} {ok} action{}",
            if ok == 1 { "" } else { "s" }
        );
        if failed > 0 {
            s.push_str(&format!(", {failed} failed"));
        }
        s.push_str(&format!("  {} ms", f.eval_ms));
        return (s, if failed > 0 { Tone::Bad } else { Tone::Good });
    }
    let word = match f.outcome.kind.as_str() {
        "cooldown" => "resting",
        "paused" => "paused",
        _ => "skipped",
    };
    let why = match f.outcome.kind.as_str() {
        "condition_failed" => {
            let i = f.outcome.at_index.unwrap_or(0);
            let trace = f.conditions.get(i);
            match (
                trace.and_then(|c| c.reason.clone()),
                rule.and_then(|r| r.conditions.get(i)),
            ) {
                (Some(r), _) if !r.is_empty() => r,
                (_, Some(c)) => format!("not so: {}", condition_words(c, name_of)),
                _ => format!("check {} said no", i + 1),
            }
        }
        "cooldown" => match f.outcome.remaining_secs {
            Some(s) => format!("after its last run, {s} s left"),
            None => "after its last run".into(),
        },
        "paused" => "paused by another rule".into(),
        "required_expression_failed" | "trigger_gate_failed" => "its own condition said no".into(),
        _ => f
            .outcome
            .reason
            .clone()
            .unwrap_or_else(|| f.outcome_label()),
    };
    (format!("{at:<13} {word:<9} {why}"), Tone::Quiet)
}

/// A dry run in lines: the headline, each check and, when it would
/// run, each step it would take. Nothing was sent.
pub fn test_lines(
    rule: Option<&Rule>,
    res: &TestResult,
    when: &str,
    name_of: super::rule_words::NameOf<'_>,
) -> Vec<(String, Tone)> {
    let then = if when.trim().is_empty() {
        "now".to_string()
    } else {
        format!("at {}", when.trim())
    };
    let unsure = |c: &crate::net::automations_api::TestCheck| {
        c.reason
            .as_deref()
            .is_some_and(|r| r.starts_with("can't tell"))
    };
    let mut out = Vec::new();
    let headline = if res.would_fire {
        if res.conditions.is_empty() {
            "It has no checks, so it would run.".to_string()
        } else {
            format!("Every check passes {then}, so it would run.")
        }
    } else if res.conditions.iter().any(unsure)
        && res.conditions.iter().all(|c| c.passed || unsure(c))
    {
        format!("It can't be told {then}: a check depends on what the running rule keeps.")
    } else {
        format!("It would not run {then}: a check says no.")
    };
    out.push((
        headline,
        if res.would_fire {
            Tone::Good
        } else {
            Tone::Bad
        },
    ));
    for (i, c) in res.conditions.iter().enumerate() {
        let what = rule
            .and_then(|r| r.conditions.get(i))
            .or(Some(&c.condition).filter(|v| !v.is_null()))
            .map(|v| condition_words(v, name_of))
            .unwrap_or_else(|| c.reason.clone().unwrap_or_default());
        let (mark, tone) = if unsure(c) {
            ("? can't tell in a test".to_string(), Tone::Quiet)
        } else if c.passed {
            ("✓ yes".to_string(), Tone::Good)
        } else {
            let mut m = "✗ no".to_string();
            match (&c.actual, &c.reason) {
                (Some(a), _) if !a.is_null() => m.push_str(&format!(", it is {}", value_text(a))),
                (_, Some(r)) if !r.is_empty() => m.push_str(&format!(" ({r})")),
                _ => {}
            }
            (m, Tone::Bad)
        };
        out.push((format!("Check   {what}   {mark}"), tone));
    }
    if res.would_fire {
        let steps: Vec<Value> = match &res.actions {
            Value::Array(a) if !a.is_empty() => a.clone(),
            _ => rule.map(|r| r.actions.clone()).unwrap_or_default(),
        };
        for (i, a) in steps.iter().enumerate() {
            if a.get("enabled").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            out.push((
                format!("Step {}  {}", i + 1, action_words(a, name_of)),
                Tone::Quiet,
            ));
        }
    }
    out.push((
        "A dry run: nothing was sent to any device.".into(),
        Tone::Quiet,
    ));
    out
}

impl App {
    // ── Names ────────────────────────────────────────────────────────────

    /// A device id as a rule's words show it: its name, or `missing: …`.
    pub fn rule_device_name(&self, id: &str) -> String {
        let plain = id.strip_prefix("DELETED:").unwrap_or(id);
        let deleted = id.starts_with("DELETED:");
        if !deleted {
            if let Some(d) = self.device_by_id(id) {
                return d.display_name().to_string();
            }
            // A rule may name a device by its name in rules.
            if let Some(d) = self.devices.iter().find(|d| {
                d.canonical_name
                    .as_deref()
                    .is_some_and(|c| c == id || c.ends_with(&format!(".{id}")))
            }) {
                return d.display_name().to_string();
            }
            if !self.rules_tab.extra.removed.contains_key(id) {
                return id.to_string();
            }
        }
        let name = self
            .rules_tab
            .extra
            .removed
            .get(plain)
            .cloned()
            .unwrap_or_else(|| plain.to_string());
        format!("missing: {name}")
    }

    /// Work out, for each rule, the devices it names that homeCore does
    /// not have: deleted ones (`DELETED:` in the rule) and the ones
    /// core's stale-refs lists. A rule may name a device by its name in
    /// rules rather than its id, so an id missing from the device list
    /// alone proves nothing; core resolves those.
    pub(crate) fn compute_missing(&mut self) {
        let have: HashSet<&str> = self.devices.iter().map(|d| d.device_id.as_str()).collect();
        let mut out = HashMap::new();
        for r in &self.rules {
            let mut refs = Vec::new();
            let v = serde_json::json!([r.trigger, r.triggers, r.conditions, r.actions]);
            device_refs(&v, &mut refs);
            let mut missing: Vec<String> = refs
                .into_iter()
                .filter(|id| id.starts_with("DELETED:"))
                .collect();
            if let Some(s) = self.rules_tab.extra.stale.get(&r.id) {
                for id in s {
                    if !missing.contains(id) && !have.contains(id.as_str()) {
                        missing.push(id.clone());
                    }
                }
            }
            if !missing.is_empty() {
                out.insert(r.id.clone(), missing);
            }
        }
        self.rules_tab.extra.missing = out;
    }

    /// The Trigger column: what starts it, or the device it lacks.
    pub fn rule_trigger_text(&self, r: &Rule) -> String {
        if let Some(m) = self
            .rules_tab
            .extra
            .missing
            .get(&r.id)
            .and_then(|m| m.first())
        {
            let n = self.rule_device_name(m);
            return if n.starts_with("missing: ") {
                n
            } else {
                format!("missing: {n}")
            };
        }
        let all: Vec<&Value> = r.trigger.iter().chain(r.triggers.iter()).collect();
        super::rule_words::triggers_short(&all, &|id| self.rule_device_name(id))
    }

    /// The Last run column.
    pub fn rule_last_run(&self, r: &Rule) -> String {
        if self.rules_tab.extra.paused.contains(&r.id) {
            return "paused".into();
        }
        let Some(runs) = self.rules_tab.extra.runs.get(&r.id) else {
            return if self.rules_tab.extra.runs_loaded {
                "-".into()
            } else {
                "...".into()
            };
        };
        match runs.iter().find(|f| f.outcome.fired()) {
            Some(f) => when_text(&f.timestamp, chrono::Utc::now()),
            None => "-".into(),
        }
    }

    /// The Group column: the first group it is in, else its tags.
    pub fn rule_group(&self, r: &Rule) -> String {
        self.rules_tab
            .extra
            .groups
            .iter()
            .find(|g| g.rule_ids.contains(&r.id))
            .map(|g| g.name.clone())
            .unwrap_or_else(|| r.tags.join(","))
    }

    pub fn rule_paused(&self, id: &str) -> bool {
        self.rules_tab.extra.paused.contains(id)
    }

    // ── Loading ──────────────────────────────────────────────────────────

    /// What the list shows besides the rules: runs, paused ones, missing
    /// devices and groups. Each call fails on its own.
    pub fn load_automation_extras(&mut self) {
        let c = self.client.clone();
        self.spawn(async move { c.list_paused_rules().await }, |app, res| {
            if let Ok(ids) = res {
                app.rules_tab.extra.paused = ids.into_iter().collect();
            }
        });
        let c = self.client.clone();
        self.spawn(async move { c.stale_refs().await }, |app, res| {
            if let Ok(list) = res {
                app.rules_tab.extra.stale = list
                    .into_iter()
                    .map(|s| (s.rule_id, s.stale_device_ids))
                    .collect();
            }
        });
        let c = self.client.clone();
        self.spawn(async move { c.removed_devices().await }, |app, res| {
            if let Ok(list) = res {
                app.rules_tab.extra.removed =
                    list.into_iter().map(|d| (d.device_id, d.name)).collect();
            }
        });
        let c = self.client.clone();
        self.spawn(async move { c.list_rule_groups().await }, |app, res| {
            if let Ok(g) = res {
                app.rules_tab.extra.groups = g;
            }
        });
        self.load_all_runs();
    }

    /// Every rule's history, six at a time.
    fn load_all_runs(&mut self) {
        use futures_util::StreamExt;
        let ids: Vec<String> = self.rules.iter().map(|r| r.id.clone()).collect();
        let c = self.client.clone();
        self.spawn(
            async move {
                let got: Vec<(String, anyhow::Result<Vec<RuleFiring>>)> =
                    futures_util::stream::iter(ids)
                        .map(|id| {
                            let c = c.clone();
                            async move {
                                let r = c.get_rule_history(&id).await;
                                (id, r)
                            }
                        })
                        .buffer_unordered(6)
                        .collect()
                        .await;
                Ok(got)
            },
            |app, res: anyhow::Result<Vec<(String, anyhow::Result<Vec<RuleFiring>>)>>| {
                if let Ok(list) = res {
                    for (id, r) in list {
                        if let Ok(mut v) = r {
                            v.reverse();
                            app.rules_tab.extra.runs.insert(id, v);
                        }
                    }
                }
                app.rules_tab.extra.runs_loaded = true;
            },
        );
    }

    /// The rules again, then what the list shows of them, after a change
    /// that touched several (core announces only some).
    pub fn reload_rules(&mut self) {
        let c = self.client.clone();
        self.spawn(async move { c.list_rules().await }, |app, res| {
            if let Ok(v) = res {
                app.rules = v;
                app.save_cache(&[super::Section::Rules]);
            }
            app.load_automation_extras();
        });
    }

    /// One rule's history again (after it fired, or ran by hand).
    pub fn reload_rule_runs(&mut self, id: &str) {
        let c = self.client.clone();
        let id = id.to_string();
        self.spawn(
            {
                let id = id.clone();
                async move { c.get_rule_history(&id).await }
            },
            move |app, res| {
                if let Ok(mut v) = res {
                    v.reverse();
                    app.rules_tab.extra.runs.insert(id, v);
                }
            },
        );
    }

    /// A live `rule_fired` or `rule_changed`: its runs and the paused
    /// list, once the Automations screen has loaded them.
    pub(crate) fn on_rule_event(&mut self, ty: &str, event: &Value) {
        if !self.rules_tab.extra.runs_loaded {
            return;
        }
        let id = event
            .get("rule_id")
            .and_then(Value::as_str)
            .or_else(|| event.pointer("/rule/id").and_then(Value::as_str))
            .map(str::to_string);
        if ty == "rule_fired"
            && let Some(id) = id
        {
            self.reload_rule_runs(&id);
        }
        let c = self.client.clone();
        self.spawn(async move { c.list_paused_rules().await }, |app, res| {
            if let Ok(ids) = res {
                app.rules_tab.extra.paused = ids.into_iter().collect();
            }
        });
    }

    // ── Keys ─────────────────────────────────────────────────────────────

    /// The rule R, t, u and F act on: the open detail's, else the
    /// selected one.
    fn target_rule(&self) -> Option<&Rule> {
        match &self.rules_tab.detail {
            Some(d) => self.rules.iter().find(|r| r.id == d.rule_id),
            None => self.selected_rule(),
        }
    }

    /// `R`: run it now, after a yes.
    pub fn ask_run_rule(&mut self) {
        let Some(r) = self.target_rule() else {
            return;
        };
        let (id, name) = (r.id.clone(), r.name.clone());
        let off = if r.enabled {
            ""
        } else {
            " It runs even though it is off."
        };
        self.ask(Confirm::new(
            format!("Run {name} now?"),
            format!(
                "Its checks decide, as if its moment came, and its steps go to the devices.{off}"
            ),
            ConfirmAction::Automations(AutomationConfirm::Run { id, name }),
        ));
    }

    /// `u`: resume one another rule paused.
    pub fn resume_target_rule(&mut self) {
        let Some(r) = self.target_rule() else {
            return;
        };
        let (id, name) = (r.id.clone(), r.name.clone());
        if !self.rule_paused(&id) {
            self.status = format!("{name} is not paused");
            return;
        }
        let c = self.client.clone();
        let target = id.clone();
        self.spawn_ok(
            "resume automation",
            async move { c.resume_rule(&target).await },
            move |app, ()| {
                app.rules_tab.extra.paused.remove(&id);
                app.status = format!("{name} resumed");
            },
        );
    }

    /// `t`: a dry run, at a time typed.
    pub fn open_rule_test(&mut self) {
        let Some(r) = self.target_rule() else {
            return;
        };
        self.rules_tab.extra.test = Some(TestPane {
            rule_id: r.id.clone(),
            name: r.name.clone(),
            input: String::new(),
            editing: true,
            asked: String::new(),
            result: Load::Idle,
            scroll: 0,
        });
    }

    fn run_rule_test(&mut self) {
        let Some(t) = self.rules_tab.extra.test.as_mut() else {
            return;
        };
        t.editing = false;
        t.asked = t.input.trim().to_string();
        t.result = Load::Loading;
        t.scroll = 0;
        let (id, at) = (t.rule_id.clone(), t.asked.clone());
        let c = self.client.clone();
        let check = id.clone();
        self.spawn(
            async move { c.test_rule(&id, &at).await },
            move |app, res| {
                if let Some(t) = app.rules_tab.extra.test.as_mut()
                    && t.rule_id == check
                {
                    t.result = match res {
                        Ok(r) => Load::Ready(r),
                        Err(e) => Load::Failed(e.to_string()),
                    };
                }
            },
        );
    }

    /// `F`: send a missing device's place in the rule to another device.
    pub fn open_rule_fix(&mut self) {
        let Some(r) = self.target_rule() else {
            return;
        };
        let missing = self
            .rules_tab
            .extra
            .missing
            .get(&r.id)
            .cloned()
            .unwrap_or_default();
        if missing.is_empty() {
            self.status = format!("{} names no missing device", r.name);
            return;
        }
        self.rules_tab.extra.fix = Some(FixPicker {
            rule_id: r.id.clone(),
            name: r.name.clone(),
            missing,
            which: 0,
            query: String::new(),
            sel: 0,
        });
    }

    /// The devices the picker offers for its query.
    pub fn fix_candidates(&self) -> Vec<(String, String)> {
        let Some(p) = &self.rules_tab.extra.fix else {
            return Vec::new();
        };
        let q = p.query.trim().to_lowercase();
        let mut v: Vec<(String, String)> = self
            .devices
            .iter()
            .filter(|d| !d.is_scene())
            .filter(|d| {
                q.is_empty()
                    || d.display_name().to_lowercase().contains(&q)
                    || d.device_id.to_lowercase().contains(&q)
            })
            .map(|d| (d.device_id.clone(), d.display_name().to_string()))
            .collect();
        v.sort_by_cached_key(|(_, n)| n.to_lowercase());
        v
    }

    fn choose_fix(&mut self) {
        let Some(p) = self.rules_tab.extra.fix.clone() else {
            return;
        };
        let list = self.fix_candidates();
        let Some((to, to_name)) = list.get(p.sel).cloned() else {
            self.status = "No device matches".into();
            return;
        };
        let from = p.missing[p.which.min(p.missing.len() - 1)].clone();
        let others: Vec<String> = self
            .rules
            .iter()
            .filter(|r| r.id != p.rule_id)
            .filter(|r| {
                let s = serde_json::to_string(r).unwrap_or_default();
                let plain = from.trim_start_matches("DELETED:");
                s.contains(&format!("\"{plain}\"")) || s.contains(&format!("\"DELETED:{plain}\""))
            })
            .map(|r| r.name.clone())
            .collect();
        let plain = from.trim_start_matches("DELETED:").to_string();
        let message = if others.is_empty() {
            format!(
                "Every place {} names {plain} will name {to_name} ({to}). A rule turned off because the device was deleted is turned on again.",
                p.name
            )
        } else {
            format!(
                "{} and {} more name {plain}: {}. Every place they do will name {to_name} ({to}), and any turned off because it was deleted are turned on again.",
                p.name,
                others.len(),
                others.join(", ")
            )
        };
        self.rules_tab.extra.fix = None;
        self.ask(Confirm::new(
            format!("Send it to {to_name} instead?"),
            message,
            ConfirmAction::Automations(AutomationConfirm::Repoint { from, to, to_name }),
        ));
    }

    pub(crate) fn run_automation_confirm(&mut self, action: AutomationConfirm) {
        let c = self.client.clone();
        match action {
            AutomationConfirm::Run { id, name } => {
                let target = id.clone();
                self.spawn_ok(
                    "run automation",
                    async move { c.run_rule(&target).await },
                    move |app, ()| {
                        app.status = format!("{name} asked to run; Recent runs shows how it went");
                        app.reload_rule_runs(&id);
                    },
                );
            }
            AutomationConfirm::Repoint { from, to, to_name } => {
                self.spawn_ok(
                    "change automations",
                    async move { c.repoint_rules(&from, &to).await },
                    move |app, n| {
                        app.status = match n {
                            1 => format!("1 automation now names {to_name}"),
                            n => format!("{n} automations now name {to_name}"),
                        };
                        app.reload_rules();
                    },
                );
            }
        }
    }

    /// Keys for the dry run and the device picker. False when neither is
    /// open.
    pub(crate) fn on_key_automations_modal(&mut self, key: KeyEvent) -> bool {
        if let Some(t) = self.rules_tab.extra.test.as_mut() {
            if t.editing {
                match key.code {
                    KeyCode::Esc => self.rules_tab.extra.test = None,
                    KeyCode::Enter => self.run_rule_test(),
                    _ => {
                        edit_text(&mut t.input, key);
                    }
                }
                return true;
            }
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                    self.rules_tab.extra.test = None
                }
                KeyCode::Char('t') => t.editing = true,
                KeyCode::Down | KeyCode::Char('j') => t.scroll = t.scroll.saturating_add(1),
                KeyCode::Up | KeyCode::Char('k') => t.scroll = t.scroll.saturating_sub(1),
                _ => {}
            }
            return true;
        }
        if self.rules_tab.extra.fix.is_some() {
            let n = self.fix_candidates().len();
            let Some(p) = self.rules_tab.extra.fix.as_mut() else {
                return true;
            };
            match key.code {
                KeyCode::Esc => self.rules_tab.extra.fix = None,
                KeyCode::Enter => self.choose_fix(),
                KeyCode::Down => p.sel = (p.sel + 1).min(n.saturating_sub(1)),
                KeyCode::Up => p.sel = p.sel.saturating_sub(1),
                KeyCode::Tab => {
                    p.which = (p.which + 1) % p.missing.len().max(1);
                }
                _ => {
                    if edit_text(&mut p.query, key) {
                        p.sel = 0;
                    }
                }
            }
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::model::Role;
    use serde_json::json;

    fn names(id: &str) -> String {
        match id {
            "vac" => "Vacation".into(),
            "porch" => "Porch Light".into(),
            other => other.into(),
        }
    }

    fn rule() -> Rule {
        serde_json::from_value(json!({
            "id": "r", "name": "Porch light at sunset", "enabled": true,
            "trigger": {"SunEvent": {"event": "Sunset", "offset_minutes": -15}},
            "conditions": [{"DeviceState": {"device_id": "vac", "attribute": "on", "op": "Eq", "value": false}}],
            "actions": [{"action": {"SetDeviceState": {"device_id": "porch", "state": {"on": true, "brightness_pct": 60}}}, "enabled": true}]
        }))
        .unwrap()
    }

    fn firing(v: Value) -> RuleFiring {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_test_that_passes_lists_its_steps_and_sends_nothing() {
        let res: TestResult = serde_json::from_value(json!({
            "conditions_pass": true, "would_fire": true,
            "conditions": [{"condition": {}, "passed": true, "actual": false}],
            "actions": [{"action": {"SetDeviceState": {"device_id": "porch", "state": {"on": true}}}, "enabled": true}]
        }))
        .unwrap();
        let lines = test_lines(Some(&rule()), &res, "22:30", &names);
        assert_eq!(lines[0].0, "Every check passes at 22:30, so it would run.");
        assert!(lines[1].0.contains("Vacation is off") && lines[1].0.contains("yes"));
        assert_eq!(lines[1].1, Tone::Good);
        assert_eq!(lines[2].0, "Step 1  Porch Light on");
        assert!(lines.last().unwrap().0.contains("nothing was sent"));
    }

    #[test]
    fn a_test_that_fails_says_which_check_and_runs_no_steps() {
        let res: TestResult = serde_json::from_value(json!({
            "conditions_pass": false, "would_fire": false,
            "conditions": [{"condition": {}, "passed": false, "actual": true}]
        }))
        .unwrap();
        let lines = test_lines(Some(&rule()), &res, "", &names);
        assert_eq!(lines[0].0, "It would not run now: a check says no.");
        assert!(lines[1].0.ends_with("✗ no, it is yes"), "{}", lines[1].0);
        assert!(!lines.iter().any(|l| l.0.starts_with("Step")));
    }

    #[test]
    fn recent_runs_say_what_happened() {
        let ran = firing(json!({
            "timestamp": "2026-11-16T16:31:00Z", "trigger_type": "SunEvent",
            "outcome": {"type": "fired"}, "eval_ms": 4,
            "conditions": [{"condition_type": "DeviceState", "passed": true}],
            "actions": [{"index": 0, "action_type": "SetDeviceState", "outcome": {"status": "ok"}}]
        }));
        let (s, tone) = run_line(&ran, Some(&rule()), &names, true);
        assert_eq!(
            s,
            "Nov 16 16:31  ran       conditions passed  1 action  4 ms"
        );
        assert_eq!(tone, Tone::Good);
        let skipped = firing(json!({
            "timestamp": "2026-11-15T16:32:00Z", "trigger_type": "SunEvent",
            "outcome": {"type": "condition_failed", "at_index": 0},
            "conditions": [{"condition_type": "DeviceState", "passed": false}]
        }));
        let (s, tone) = run_line(&skipped, Some(&rule()), &names, true);
        assert_eq!(s, "Nov 15 16:32  skipped   not so: Vacation is off");
        assert_eq!(tone, Tone::Quiet);
    }

    #[test]
    fn last_run_reads_short() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-17T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(when_text("2026-11-17T11:59:30Z", now), "just now");
        assert_eq!(when_text("2026-11-17T11:00:00Z", now), "1h ago");
        assert_eq!(when_text("2025-12-01T12:00:00Z", now), "Dec 2025");
        assert_eq!(when_text("nope", now), "-");
    }

    #[test]
    fn missing_devices_mark_a_rule_as_needing_fixing() {
        let mut a = signed_in(Role::Admin);
        a.devices = vec![device("porch", "Porch Light"), device("vac", "Vacation")];
        let mut r = rule();
        r.actions.push(json!({"action": {"SetDeviceState": {"device_id": "DELETED:fan", "state": {"on": false}}}, "enabled": true}));
        a.rules = vec![r];
        a.rules_tab
            .extra
            .removed
            .insert("fan".into(), "Office Fan".into());
        a.refresh_views();
        let r = &a.rules[0];
        assert!(a.rules_tab.extra.needs_fixing(r));
        assert_eq!(a.rule_trigger_text(r), "missing: Office Fan");
        a.rules[0].actions.pop();
        a.refresh_views();
        assert!(!a.rules_tab.extra.needs_fixing(&a.rules[0]));
        assert_eq!(a.rule_trigger_text(&a.rules[0]), "sunset -15m");
    }

    #[test]
    fn t_opens_a_time_box_and_esc_closes_it() {
        let mut a = signed_in(Role::Admin);
        a.rules = vec![rule()];
        a.screen = crate::screens::ScreenId::Automations;
        a.refresh_views();
        a.on_key(key(KeyCode::Char('t')));
        for c in "22:30".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        assert_eq!(a.rules_tab.extra.test.as_ref().unwrap().input, "22:30");
        a.on_key(key(KeyCode::Esc));
        assert!(a.rules_tab.extra.test.is_none());
    }

    #[test]
    fn run_now_asks_first() {
        let mut a = signed_in(Role::Admin);
        a.rules = vec![rule()];
        a.screen = crate::screens::ScreenId::Automations;
        a.refresh_views();
        a.on_key(key(KeyCode::Char('R')));
        assert!(a.confirm.is_some(), "R asks before running");
    }
}
