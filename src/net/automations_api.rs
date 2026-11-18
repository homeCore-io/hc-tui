//! Automations beyond the list: run now, a dry run at a time, paused
//! ones, missing devices and sending a rule to another device.

use super::HomeCoreClient;
use super::http::seg;
use anyhow::Result;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// One check of a dry run (`POST /automations/{id}/test`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TestCheck {
    #[serde(default)]
    pub condition: Value,
    #[serde(default)]
    pub passed: bool,
    #[serde(default)]
    pub actual: Option<Value>,
    #[serde(default)]
    pub expected: Option<Value>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// What a dry run found. Nothing is sent to any device.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TestResult {
    #[serde(default)]
    pub conditions_pass: bool,
    #[serde(default)]
    pub conditions: Vec<TestCheck>,
    #[serde(default)]
    pub would_fire: bool,
    /// The rule's steps, `[{"action": {...}, "enabled": bool}]`.
    #[serde(default)]
    pub actions: Value,
    /// The time it was judged at, when one was asked for.
    #[serde(default)]
    pub at: Option<String>,
}

/// A rule naming devices homeCore does not have
/// (`GET /automations/stale-refs`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StaleRef {
    pub rule_id: String,
    #[serde(default)]
    pub rule_name: String,
    #[serde(default)]
    pub stale_device_ids: Vec<String>,
}

/// A device someone removed (`GET /devices/removed`): its name, for a
/// rule that still names it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RemovedDevice {
    pub device_id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Deserialize)]
struct Paused {
    #[serde(default)]
    paused: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Repointed {
    #[serde(default)]
    changed: Vec<Value>,
}

impl HomeCoreClient {
    /// `POST /automations/{id}/run`: as if its moment came, checks and
    /// all. Accepted before it runs; its history says how it went.
    pub async fn run_rule(&self, id: &str) -> Result<()> {
        let path = format!("/automations/{}/run", seg(id));
        self.send_empty(Method::POST, &path, Some(json!({ "checks": true })))
            .await
    }

    /// `POST /automations/{id}/test[?at=]`: a dry run, judged now or at
    /// `at` (`22:30` or `2026-11-18T07:00`).
    pub async fn test_rule(&self, id: &str, at: &str) -> Result<TestResult> {
        let mut path = format!("/automations/{}/test", seg(id));
        let at = at.trim();
        if !at.is_empty() {
            path.push_str(&format!("?at={}", urlencoding::encode(at)));
        }
        self.send_json(Method::POST, &path, None).await
    }

    /// `GET /automations/paused`: rules another rule has paused.
    pub async fn list_paused_rules(&self) -> Result<Vec<String>> {
        let p: Paused = self.get_json("/automations/paused").await?;
        Ok(p.paused)
    }

    /// `DELETE /automations/{id}/pause`: resume it by hand.
    pub async fn resume_rule(&self, id: &str) -> Result<()> {
        let path = format!("/automations/{}/pause", seg(id));
        self.send_empty(Method::DELETE, &path, None).await
    }

    pub async fn stale_refs(&self) -> Result<Vec<StaleRef>> {
        self.get_json("/automations/stale-refs").await
    }

    pub async fn removed_devices(&self) -> Result<Vec<RemovedDevice>> {
        self.get_json("/devices/removed").await
    }

    /// `POST /automations/repoint`: every rule naming `from` names `to`
    /// instead. Answers how many rules changed.
    pub async fn repoint_rules(&self, from: &str, to: &str) -> Result<usize> {
        let body = json!({ "from": from, "to": to });
        let r: Repointed = self
            .send_json(Method::POST, "/automations/repoint", Some(body))
            .await?;
        Ok(r.changed.len())
    }
}
