//! Alerts: acknowledging one, pressing its button, alert rules and the
//! ignore list.

use super::HomeCoreClient;
use super::http::seg;
use crate::model::Alert;
use anyhow::Result;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// An alert rule (core's `AlertRule`): a device in a state, maybe only at
/// some hours, maybe only once it has lasted. The rest of its fields are
/// kept as they came, so turning it on or off sends it back whole.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub device_ids: Vec<String>,
    #[serde(default)]
    pub attribute: String,
    #[serde(default)]
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub for_minutes: Option<u32>,
    #[serde(default)]
    pub severity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, Value>,
}

fn yes() -> bool {
    true
}

/// Alerts someone chose not to see (core's `AlertIgnore`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AlertIgnore {
    #[serde(default)]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

impl HomeCoreClient {
    /// `POST /alerts/{id}/acknowledge`: seen. It stays until it clears.
    pub async fn acknowledge_alert(&self, id: &str) -> Result<Alert> {
        let path = format!("/alerts/{}/acknowledge", seg(id));
        self.send_json(Method::POST, &path, None).await
    }

    /// `POST /alerts/{id}/press`: run the alert's button.
    pub async fn press_alert(&self, id: &str) -> Result<()> {
        let path = format!("/alerts/{}/press", seg(id));
        self.send_empty(Method::POST, &path, None).await
    }

    pub async fn list_alert_rules(&self) -> Result<Vec<AlertRule>> {
        self.get_json("/alert-rules").await
    }

    /// `PUT /alert-rules/{id}`: the whole rule, replaced.
    pub async fn put_alert_rule(&self, rule: &AlertRule) -> Result<AlertRule> {
        let path = format!("/alert-rules/{}", seg(&rule.id));
        self.send_json(Method::PUT, &path, Some(json!(rule))).await
    }

    /// `DELETE /alert-rules/{id}?automations=disable`: automations it
    /// starts stay, turned off.
    pub async fn delete_alert_rule(&self, id: &str) -> Result<()> {
        let path = format!("/alert-rules/{}?automations=disable", seg(id));
        self.send_empty(Method::DELETE, &path, None).await
    }

    pub async fn list_alert_ignores(&self) -> Result<Vec<AlertIgnore>> {
        self.get_json("/alert-ignores").await
    }

    pub async fn create_alert_ignore(&self, ignore: &AlertIgnore) -> Result<AlertIgnore> {
        self.send_json(Method::POST, "/alert-ignores", Some(json!(ignore)))
            .await
    }

    /// `DELETE /alert-ignores/{id}`: what it hid shows again.
    pub async fn delete_alert_ignore(&self, id: &str) -> Result<()> {
        let path = format!("/alert-ignores/{}", seg(id));
        self.send_empty(Method::DELETE, &path, None).await
    }
}
