use anyhow::{Context, Result, anyhow};
use reqwest::{Client, Method, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    #[serde(alias = "admin", alias = "Admin", alias = "ADMIN")]
    Admin,
    #[serde(alias = "user", alias = "User", alias = "USER")]
    User,
    #[serde(
        alias = "read_only",
        alias = "readonly",
        alias = "readOnly",
        alias = "ReadOnly",
        alias = "READ_ONLY"
    )]
    ReadOnly,
}

impl Role {
    pub fn is_admin(&self) -> bool {
        matches!(self, Self::Admin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub role: Role,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    pub device_id: String,
    #[serde(default)]
    pub canonical_name: Option<String>,
    pub name: String,
    pub plugin_id: String,
    #[serde(default)]
    pub device_type: Option<String>,
    pub area: Option<String>,
    pub available: bool,
    pub attributes: Map<String, Value>,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    pub name: String,
    /// Populated for device-backed (e.g. hue) scenes only.
    #[serde(skip)]
    pub plugin_id: Option<String>,
    #[serde(skip)]
    pub area: Option<String>,
    #[serde(skip)]
    pub active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Area {
    pub id: String,
    pub name: String,
    pub device_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub trigger: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleFiring {
    pub timestamp: String,
    pub conditions_passed: bool,
    pub actions_ran: usize,
    pub eval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub rule_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub version: String,
    pub uptime_seconds: u64,
    pub started_at: String,
    pub rules_total: usize,
    pub rules_enabled: usize,
    pub devices_total: usize,
    pub plugins_active: usize,
    pub state_db_bytes: u64,
    pub history_db_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
    #[serde(default)]
    pub fields: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRecord {
    pub plugin_id: String,
    pub registered_at: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEntry {
    #[serde(rename = "type")]
    pub event_type: String,
    pub timestamp: String,
    #[serde(default)]
    pub plugin_id: Option<String>,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub rule_name: Option<String>,
    #[serde(default)]
    pub event_type_custom: Option<String>,
    #[serde(default)]
    pub event_detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeConfig {
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub on_event: Option<String>,
    #[serde(default)]
    pub off_event: Option<String>,
    #[serde(default)]
    pub on_offset_minutes: Option<i32>,
    #[serde(default)]
    pub off_offset_minutes: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeRecord {
    pub config: ModeConfig,
    pub state: Option<DeviceState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatterNode {
    pub node_id: String,
    pub commissioned_at_unix: u64,
    pub last_interview_unix: u64,
    pub endpoint: u16,
    #[serde(default)]
    pub clusters: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MatterNodesResponse {
    #[serde(default)]
    nodes: Vec<MatterNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoginBody<'a> {
    pub username: &'a str,
    pub password: &'a str,
}

#[derive(Clone)]
pub struct HomeCoreClient {
    http: Client,
    base_url: String,
    token: Option<String>,
}

impl HomeCoreClient {
    pub fn new(base_url: String) -> Self {
        let mut normalized = base_url.trim_end_matches('/').to_string();
        if normalized.ends_with("/api/v1") {
            normalized.truncate(normalized.len() - "/api/v1".len());
        }
        Self {
            http: Client::new(),
            base_url: normalized,
            token: None,
        }
    }

    pub fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    pub fn ws_events_url(&self) -> String {
        let mut ws_base = self.base_url.clone();
        if ws_base.starts_with("https://") {
            ws_base = ws_base.replacen("https://", "wss://", 1);
        } else if ws_base.starts_with("http://") {
            ws_base = ws_base.replacen("http://", "ws://", 1);
        }
        format!("{}/api/v1/events/stream", ws_base.trim_end_matches('/'))
    }

    pub fn ws_logs_url(&self) -> String {
        let mut ws_base = self.base_url.clone();
        if ws_base.starts_with("https://") {
            ws_base = ws_base.replacen("https://", "wss://", 1);
        } else if ws_base.starts_with("http://") {
            ws_base = ws_base.replacen("http://", "ws://", 1);
        }
        format!("{}/api/v1/logs/stream", ws_base.trim_end_matches('/'))
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<LoginResponse> {
        let url = self.endpoint("/auth/login");
        let resp = self
            .http
            .post(url)
            .json(&LoginBody { username, password })
            .send()
            .await
            .context("login request failed")?;
        Self::parse_login_response(resp).await
    }

    pub async fn me(&self) -> Result<UserInfo> {
        let resp = self.request(Method::GET, "/auth/me").await?;
        Self::parse_json(resp).await
    }

    pub async fn list_devices(&self) -> Result<Vec<DeviceState>> {
        let resp = self.request(Method::GET, "/devices").await?;
        Self::parse_devices_response(resp).await
    }

    pub async fn list_scenes(&self) -> Result<Vec<Scene>> {
        let resp = self.request(Method::GET, "/scenes").await?;
        Self::parse_json(resp).await
    }

    pub async fn list_areas(&self) -> Result<Vec<Area>> {
        let resp = self.request(Method::GET, "/areas").await?;
        Self::parse_areas_response(resp).await
    }

    pub async fn list_automations(&self) -> Result<Vec<Rule>> {
        let resp = self.request(Method::GET, "/automations").await?;
        Self::parse_json(resp).await
    }

    #[allow(dead_code)]
    pub async fn list_automations_filtered(
        &self,
        tag: Option<&str>,
        trigger: Option<&str>,
        stale: bool,
    ) -> Result<Vec<Rule>> {
        let mut params: Vec<String> = Vec::new();
        if let Some(t) = tag {
            if !t.is_empty() {
                params.push(format!("tag={}", urlencoding::encode(t)));
            }
        }
        if let Some(tr) = trigger {
            if !tr.is_empty() && tr != "all" {
                params.push(format!("trigger={}", urlencoding::encode(tr)));
            }
        }
        if stale {
            params.push("stale=true".to_string());
        }
        let path = if params.is_empty() {
            "/automations".to_string()
        } else {
            format!("/automations?{}", params.join("&"))
        };
        let resp = self.request(Method::GET, &path).await?;
        Self::parse_json(resp).await
    }

    pub async fn get_automation_history(&self, id: &str) -> Result<Vec<RuleFiring>> {
        let path = format!("/automations/{id}/history");
        let resp = self.request(Method::GET, &path).await?;
        Self::parse_json(resp).await
    }

    pub async fn clone_automation(&self, id: &str) -> Result<Rule> {
        let path = format!("/automations/{id}/clone");
        let resp = self.request(Method::POST, &path).await?;
        Self::parse_json(resp).await
    }

    pub async fn delete_automation(&self, id: &str) -> Result<()> {
        let path = format!("/automations/{id}");
        let resp = self.request(Method::DELETE, &path).await?;
        Self::parse_empty(resp).await
    }

    pub async fn toggle_automation(&self, id: &str, enabled: bool) -> Result<()> {
        let path = format!("/automations/{id}");
        let body = json!({ "enabled": enabled });
        let resp = self.request_with_json(Method::PATCH, &path, body).await?;
        Self::parse_empty(resp).await
    }

    pub async fn bulk_toggle_automations(&self, ids: &[String], enabled: bool) -> Result<()> {
        let body = json!({ "ids": ids, "enabled": enabled });
        let resp = self
            .request_with_json(Method::PATCH, "/automations", body)
            .await?;
        Self::parse_empty(resp).await
    }

    pub async fn list_automation_groups(&self) -> Result<Vec<RuleGroup>> {
        let resp = self.request(Method::GET, "/automations/groups").await?;
        Self::parse_json(resp).await
    }

    #[allow(dead_code)]
    pub async fn create_automation_group(&self, name: &str) -> Result<RuleGroup> {
        let body = json!({ "name": name, "rule_ids": [] });
        let resp = self
            .request_with_json(Method::POST, "/automations/groups", body)
            .await?;
        Self::parse_json(resp).await
    }

    pub async fn delete_automation_group(&self, id: &str) -> Result<()> {
        let path = format!("/automations/groups/{id}");
        let resp = self.request(Method::DELETE, &path).await?;
        Self::parse_empty(resp).await
    }

    pub async fn enable_automation_group(&self, id: &str) -> Result<()> {
        let path = format!("/automations/groups/{id}/enable");
        let resp = self.request(Method::POST, &path).await?;
        Self::parse_empty(resp).await
    }

    pub async fn disable_automation_group(&self, id: &str) -> Result<()> {
        let path = format!("/automations/groups/{id}/disable");
        let resp = self.request(Method::POST, &path).await?;
        Self::parse_empty(resp).await
    }

    pub async fn get_system_status(&self) -> Result<SystemStatus> {
        let resp = self.request(Method::GET, "/system/status").await?;
        Self::parse_json(resp).await
    }

    pub async fn list_events(&self, limit: usize) -> Result<Vec<EventEntry>> {
        let path = format!("/events?limit={limit}");
        let resp = self.request(Method::GET, &path).await?;
        Self::parse_events_response(resp).await
    }

    pub async fn list_plugins(&self) -> Result<Vec<PluginRecord>> {
        let resp = self.request(Method::GET, "/plugins").await?;
        Self::parse_json(resp).await
    }

    pub async fn matter_commission(&self, payload: Value) -> Result<()> {
        let resp = self
            .request_with_json(Method::POST, "/plugins/matter/commission", payload)
            .await?;
        Self::parse_empty(resp).await
    }

    pub async fn list_matter_nodes(&self) -> Result<Vec<MatterNode>> {
        let resp = self.request(Method::GET, "/plugins/matter/nodes").await?;
        let body: MatterNodesResponse = Self::parse_json(resp).await?;
        Ok(body.nodes)
    }

    pub async fn matter_reinterview(&self, node_id: &str) -> Result<()> {
        let body = json!({ "node_id": node_id });
        let resp = self
            .request_with_json(Method::POST, "/plugins/matter/reinterview", body)
            .await?;
        Self::parse_empty(resp).await
    }

    pub async fn matter_remove_node(&self, node_id: &str) -> Result<()> {
        let path = format!("/plugins/matter/nodes/{node_id}");
        let resp = self.request(Method::DELETE, &path).await?;
        Self::parse_empty(resp).await
    }

    pub async fn list_users(&self) -> Result<Vec<UserInfo>> {
        let resp = self.request(Method::GET, "/auth/users").await?;
        Self::parse_json(resp).await
    }

    pub async fn create_area(&self, name: &str) -> Result<Area> {
        let body = serde_json::json!({ "name": name });
        let resp = self.request_with_json(Method::POST, "/areas", body).await?;
        Self::parse_area_response(resp).await
    }

    pub async fn rename_area(&self, id: &str, name: &str) -> Result<Area> {
        let path = format!("/areas/{id}");
        let body = serde_json::json!({ "name": name });
        let resp = self.request_with_json(Method::PATCH, &path, body).await?;
        Self::parse_area_response(resp).await
    }

    pub async fn delete_area(&self, id: &str) -> Result<()> {
        let path = format!("/areas/{id}");
        let resp = self.request(Method::DELETE, &path).await?;
        Self::parse_empty(resp).await
    }

    pub async fn set_area_devices(&self, id: &str, device_ids: &[String]) -> Result<Area> {
        let path = format!("/areas/{id}/devices");
        let body = serde_json::json!({ "device_ids": device_ids });
        let resp = self.request_with_json(Method::PUT, &path, body).await?;
        Self::parse_area_response(resp).await
    }

    pub async fn delete_device(&self, device_id: &str) -> Result<()> {
        let path = format!("/devices/{device_id}");
        let resp = self.request(Method::DELETE, &path).await?;
        Self::parse_empty(resp).await
    }

    pub async fn list_switches(&self) -> Result<Vec<DeviceState>> {
        let resp = self.request(Method::GET, "/switches").await?;
        Self::parse_json(resp).await
    }

    pub async fn create_switch(&self, id: &str, label: &str) -> Result<DeviceState> {
        let body = json!({ "id": id, "label": label });
        let resp = self
            .request_with_json(Method::POST, "/switches", body)
            .await?;
        Self::parse_json(resp).await
    }

    pub async fn list_timers(&self) -> Result<Vec<DeviceState>> {
        let resp = self.request(Method::GET, "/timers").await?;
        Self::parse_json(resp).await
    }

    pub async fn create_timer(&self, id: &str, label: &str) -> Result<DeviceState> {
        let body = json!({ "id": id, "label": label });
        let resp = self
            .request_with_json(Method::POST, "/timers", body)
            .await?;
        Self::parse_json(resp).await
    }

    pub async fn list_modes(&self) -> Result<Vec<ModeRecord>> {
        let resp = self.request(Method::GET, "/modes").await?;
        Self::parse_json(resp).await
    }

    pub async fn create_mode(&self, id: &str, name: &str, kind: &str) -> Result<ModeConfig> {
        let body = json!({ "id": id, "name": name, "kind": kind });
        let resp = self.request_with_json(Method::POST, "/modes", body).await?;
        Self::parse_json(resp).await
    }

    pub async fn delete_mode(&self, id: &str) -> Result<()> {
        let path = format!("/modes/{id}");
        let resp = self.request(Method::DELETE, &path).await?;
        Self::parse_empty(resp).await
    }

    pub async fn create_user(
        &self,
        username: &str,
        password: &str,
        role: &Role,
    ) -> Result<UserInfo> {
        let body = serde_json::json!({ "username": username, "password": password, "role": role });
        let resp = self
            .request_with_json(Method::POST, "/auth/users", body)
            .await?;
        Self::parse_json(resp).await
    }

    pub async fn delete_user(&self, id: &str) -> Result<()> {
        let path = format!("/auth/users/{id}");
        let resp = self.request(Method::DELETE, &path).await?;
        Self::parse_empty(resp).await
    }

    pub async fn set_user_role(&self, id: &str, role: &Role) -> Result<UserInfo> {
        let path = format!("/auth/users/{id}/role");
        let body = serde_json::json!({ "role": role });
        let resp = self.request_with_json(Method::PATCH, &path, body).await?;
        Self::parse_json(resp).await
    }

    pub async fn change_password(&self, current_password: &str, new_password: &str) -> Result<()> {
        let body = serde_json::json!({ "current_password": current_password, "new_password": new_password });
        let resp = self
            .request_with_json(Method::POST, "/auth/change-password", body)
            .await?;
        Self::parse_empty(resp).await
    }

    pub async fn deregister_plugin(&self, plugin_id: &str) -> Result<()> {
        let path = format!("/plugins/{plugin_id}");
        let resp = self.request(Method::DELETE, &path).await?;
        Self::parse_empty(resp).await
    }

    pub async fn discover_plugin_bridges(&self, plugin_id: &str) -> Result<()> {
        // Try known endpoint patterns so TUI can work across HomeCore API variants.
        let paths = [
            format!("/plugins/{plugin_id}/discover"),
            format!("/plugins/{plugin_id}/bridges/discover"),
            "/plugins/hue/bridges/discover".to_string(),
        ];

        let mut last_not_found: Option<String> = None;

        for path in paths {
            let resp = self.request(Method::POST, &path).await?;
            let status = resp.status();
            if status.is_success() {
                return Ok(());
            }

            let msg = Self::extract_error_message(resp).await;
            if status.as_u16() == 404 {
                last_not_found = Some(format!("{}: {}", status, msg));
                continue;
            }

            return Err(anyhow!("{}: {}", status, msg));
        }

        Err(anyhow!(
            "discover endpoint not available: {}",
            last_not_found.unwrap_or_else(|| "no supported discover endpoint".to_string())
        ))
    }

    pub async fn activate_scene(&self, scene_id: &str) -> Result<()> {
        let path = format!("/scenes/{scene_id}/activate");
        let resp = self.request(Method::POST, &path).await?;
        Self::parse_empty(resp).await
    }

    pub async fn activate_device_scene(&self, device_id: &str) -> Result<()> {
        let path = format!("/devices/{device_id}/state");
        let body = json!({ "action": "activate_scene" });
        let resp = self.request_with_json(Method::PATCH, &path, body).await?;
        Self::parse_empty(resp).await
    }

    pub async fn activate_lutron_device_scene(&self, device_id: &str) -> Result<()> {
        let path = format!("/devices/{device_id}/state");
        let body = json!({ "activate": true });
        let resp = self.request_with_json(Method::PATCH, &path, body).await?;
        Self::parse_empty(resp).await
    }

    pub async fn set_device_on(&self, device_id: &str, on: bool) -> Result<()> {
        let path = format!("/devices/{device_id}/state");
        let body = json!({ "on": on });
        let resp = self.request_with_json(Method::PATCH, &path, body).await?;
        Self::parse_empty(resp).await
    }

    pub async fn set_device_brightness(&self, device_id: &str, brightness: i64) -> Result<()> {
        let path = format!("/devices/{device_id}/state");
        let body = json!({ "brightness": brightness });
        let resp = self.request_with_json(Method::PATCH, &path, body).await?;
        Self::parse_empty(resp).await
    }

    pub async fn set_device_brightness_pct(&self, device_id: &str, brightness: f64) -> Result<()> {
        let path = format!("/devices/{device_id}/state");
        let body = json!({ "brightness_pct": brightness });
        let resp = self.request_with_json(Method::PATCH, &path, body).await?;
        Self::parse_empty(resp).await
    }

    pub async fn set_device_locked(&self, device_id: &str, locked: bool) -> Result<()> {
        let path = format!("/devices/{device_id}/state");
        let body = json!({ "locked": locked });
        let resp = self.request_with_json(Method::PATCH, &path, body).await?;
        Self::parse_empty(resp).await
    }

    pub async fn patch_device_state(&self, device_id: &str, body: Value) -> Result<()> {
        let path = format!("/devices/{device_id}/state");
        let resp = self.request_with_json(Method::PATCH, &path, body).await?;
        Self::parse_empty(resp).await
    }

    pub async fn send_device_action(&self, device_id: &str, action: &str) -> Result<()> {
        self.patch_device_state(device_id, json!({ "action": action }))
            .await
    }

    pub async fn update_device_metadata(
        &self,
        device_id: &str,
        name: &str,
        area: Option<&str>,
        canonical_name: Option<&str>,
    ) -> Result<()> {
        let path = format!("/devices/{device_id}");
        let mut body = Map::new();
        body.insert("name".to_string(), Value::String(name.to_string()));
        body.insert(
            "area".to_string(),
            match area {
                Some(a) => Value::String(a.to_string()),
                None => Value::Null,
            },
        );
        body.insert(
            "canonical_name".to_string(),
            match canonical_name {
                Some(value) => Value::String(value.to_string()),
                None => Value::Null,
            },
        );
        let resp = self
            .request_with_json(Method::PATCH, &path, Value::Object(body))
            .await?;
        if resp.status().is_success() {
            return Ok(());
        }
        let message = Self::extract_error_message(resp).await;
        Err(anyhow!("failed to update device: {message}"))
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.base_url, path)
    }

    async fn request(&self, method: Method, path: &str) -> Result<Response> {
        let token = self
            .token
            .as_deref()
            .ok_or_else(|| anyhow!("not authenticated"))?;
        let resp = self
            .http
            .request(method, self.endpoint(path))
            .bearer_auth(token)
            .send()
            .await
            .with_context(|| format!("request failed: {path}"))?;
        Ok(resp)
    }

    async fn request_with_json(&self, method: Method, path: &str, body: Value) -> Result<Response> {
        let token = self
            .token
            .as_deref()
            .ok_or_else(|| anyhow!("not authenticated"))?;
        let resp = self
            .http
            .request(method, self.endpoint(path))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("request failed: {path}"))?;
        Ok(resp)
    }

    async fn parse_json<T: for<'a> Deserialize<'a>>(resp: Response) -> Result<T> {
        let status = resp.status();
        if status.is_success() {
            let text = resp.text().await.context("failed to read response body")?;
            return serde_json::from_str::<T>(&text).with_context(|| {
                let snippet = text.chars().take(300).collect::<String>();
                format!("failed to parse json response: {snippet}")
            });
        }
        let message = Self::extract_error_message(resp).await;
        Err(anyhow!("{}: {}", status, message))
    }

    async fn parse_empty(resp: Response) -> Result<()> {
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let message = Self::extract_error_message(resp).await;
        Err(anyhow!("{}: {}", status, message))
    }

    async fn extract_error_message(resp: Response) -> String {
        let text = resp.text().await.unwrap_or_default();
        if text.trim().is_empty() {
            return "request failed".to_string();
        }

        if let Ok(body) = serde_json::from_str::<Value>(&text) {
            if let Some(error) = body.get("error").and_then(Value::as_str) {
                return error.to_string();
            }
            if let Some(message) = body.get("message").and_then(Value::as_str) {
                return message.to_string();
            }
        }

        text
    }

    async fn parse_login_response(resp: Response) -> Result<LoginResponse> {
        let status = resp.status();
        if !status.is_success() {
            let message = Self::extract_error_message(resp).await;
            return Err(anyhow!("{}: {}", status, message));
        }

        let text = resp
            .text()
            .await
            .context("failed to read login response body")?;
        let parsed = serde_json::from_str::<Value>(&text).with_context(|| {
            let snippet = text.chars().take(300).collect::<String>();
            format!("failed to parse login json: {snippet}")
        })?;

        let body = parsed.get("data").unwrap_or(&parsed);
        let token = body
            .get("token")
            .or_else(|| body.get("access_token"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("login response missing token/access_token field"))?
            .to_string();

        let user_value = body
            .get("user")
            .ok_or_else(|| anyhow!("login response missing user field"))?
            .clone();

        let user = serde_json::from_value::<UserInfo>(user_value)
            .context("failed to parse user object from login response")?;

        Ok(LoginResponse { token, user })
    }

    async fn parse_devices_response(resp: Response) -> Result<Vec<DeviceState>> {
        let status = resp.status();
        if !status.is_success() {
            let message = Self::extract_error_message(resp).await;
            return Err(anyhow!("{}: {}", status, message));
        }

        let text = resp
            .text()
            .await
            .context("failed to read devices response body")?;
        let parsed = serde_json::from_str::<Value>(&text).with_context(|| {
            let snippet = text.chars().take(300).collect::<String>();
            format!("failed to parse devices json: {snippet}")
        })?;
        let arr = parsed
            .as_array()
            .ok_or_else(|| anyhow!("devices payload was not a JSON array"))?;

        let mut devices = Vec::with_capacity(arr.len());
        for item in arr {
            let obj = match item.as_object() {
                Some(v) => v,
                None => continue,
            };
            let device_id = obj
                .get("device_id")
                .or_else(|| obj.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if device_id.is_empty() {
                continue;
            }

            let name = obj
                .get("name")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .unwrap_or_else(|| device_id.clone());
            let canonical_name = obj
                .get("canonical_name")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let plugin_id = obj
                .get("plugin_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let device_type = obj
                .get("device_type")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let area = obj
                .get("area")
                .or_else(|| obj.get("room"))
                .or_else(|| obj.get("area_id"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let available = obj
                .get("available")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let attributes = obj
                .get("attributes")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let last_seen = obj
                .get("last_seen")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .unwrap_or_default();

            devices.push(DeviceState {
                device_id,
                canonical_name,
                name,
                plugin_id,
                device_type,
                area,
                available,
                attributes,
                last_seen,
            });
        }

        Ok(devices)
    }

    async fn parse_events_response(resp: Response) -> Result<Vec<EventEntry>> {
        let status = resp.status();
        if !status.is_success() {
            let message = Self::extract_error_message(resp).await;
            return Err(anyhow!("{}: {}", status, message));
        }

        let text = resp
            .text()
            .await
            .context("failed to read events response body")?;
        let parsed = serde_json::from_str::<Value>(&text).with_context(|| {
            let snippet = text.chars().take(300).collect::<String>();
            format!("failed to parse events json: {snippet}")
        })?;
        let arr = parsed
            .as_array()
            .ok_or_else(|| anyhow!("events payload was not a JSON array"))?;

        let mut events = Vec::with_capacity(arr.len());
        for item in arr {
            let obj = match item.as_object() {
                Some(v) => v,
                None => continue,
            };

            let event_obj = obj
                .get("event")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();

            let event_type = obj
                .get("type")
                .and_then(Value::as_str)
                .or_else(|| event_obj.get("type").and_then(Value::as_str))
                .or_else(|| obj.get("event_type").and_then(Value::as_str))
                .unwrap_or("unknown")
                .to_string();

            let timestamp = obj
                .get("timestamp")
                .and_then(Value::as_str)
                .or_else(|| event_obj.get("timestamp").and_then(Value::as_str))
                .unwrap_or("")
                .to_string();

            let device_id = obj
                .get("device_id")
                .and_then(Value::as_str)
                .or_else(|| event_obj.get("device_id").and_then(Value::as_str))
                .map(ToString::to_string);

            let plugin_id = obj
                .get("plugin_id")
                .and_then(Value::as_str)
                .or_else(|| event_obj.get("plugin_id").and_then(Value::as_str))
                .map(ToString::to_string);

            let rule_name = obj
                .get("rule_name")
                .and_then(Value::as_str)
                .or_else(|| event_obj.get("rule_name").and_then(Value::as_str))
                .map(ToString::to_string);

            let event_type_custom = obj
                .get("event_type")
                .and_then(Value::as_str)
                .or_else(|| event_obj.get("event_type").and_then(Value::as_str))
                .map(ToString::to_string);

            let event_detail = summarize_event_detail(obj, &event_obj);

            events.push(EventEntry {
                event_type,
                timestamp,
                plugin_id,
                device_id,
                rule_name,
                event_type_custom,
                event_detail,
            });
        }

        Ok(events)
    }

    async fn parse_areas_response(resp: Response) -> Result<Vec<Area>> {
        let status = resp.status();
        if !status.is_success() {
            let message = Self::extract_error_message(resp).await;
            return Err(anyhow!("{}: {}", status, message));
        }

        let text = resp
            .text()
            .await
            .context("failed to read areas response body")?;
        let parsed = serde_json::from_str::<Value>(&text).with_context(|| {
            let snippet = text.chars().take(300).collect::<String>();
            format!("failed to parse areas json: {snippet}")
        })?;

        let arr = parsed
            .as_array()
            .or_else(|| parsed.get("areas").and_then(Value::as_array))
            .or_else(|| parsed.get("data").and_then(Value::as_array))
            .ok_or_else(|| anyhow!("areas payload was not a JSON array"))?;

        let areas = arr
            .iter()
            .filter_map(Self::parse_area_value)
            .collect::<Vec<_>>();
        Ok(areas)
    }

    async fn parse_area_response(resp: Response) -> Result<Area> {
        let status = resp.status();
        if !status.is_success() {
            let message = Self::extract_error_message(resp).await;
            return Err(anyhow!("{}: {}", status, message));
        }

        let text = resp
            .text()
            .await
            .context("failed to read area response body")?;
        let parsed = serde_json::from_str::<Value>(&text).with_context(|| {
            let snippet = text.chars().take(300).collect::<String>();
            format!("failed to parse area json: {snippet}")
        })?;

        let value = parsed.get("data").unwrap_or(&parsed);
        Self::parse_area_value(value).ok_or_else(|| anyhow!("failed to parse area object"))
    }

    fn parse_area_value(value: &Value) -> Option<Area> {
        let obj = value.as_object()?;

        let id = obj
            .get("id")
            .or_else(|| obj.get("area_id"))
            .and_then(Value::as_str)
            .map(ToString::to_string)?;

        let name = obj
            .get("name")
            .or_else(|| obj.get("label"))
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| id.clone());

        let device_ids = if let Some(arr) = obj
            .get("device_ids")
            .or_else(|| obj.get("deviceIds"))
            .and_then(Value::as_array)
        {
            arr.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        } else if let Some(arr) = obj.get("devices").and_then(Value::as_array) {
            arr.iter()
                .filter_map(|v| {
                    v.as_str()
                        .map(ToString::to_string)
                        .or_else(|| {
                            v.get("device_id")
                                .and_then(Value::as_str)
                                .map(ToString::to_string)
                        })
                        .or_else(|| v.get("id").and_then(Value::as_str).map(ToString::to_string))
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        Some(Area {
            id,
            name,
            device_ids,
        })
    }
}

fn summarize_event_detail(
    root: &serde_json::Map<String, Value>,
    event_obj: &serde_json::Map<String, Value>,
) -> Option<String> {
    let get_str = |k: &str| {
        root.get(k)
            .and_then(Value::as_str)
            .or_else(|| event_obj.get(k).and_then(Value::as_str))
    };
    let get_i64 = |k: &str| {
        root.get(k)
            .and_then(Value::as_i64)
            .or_else(|| event_obj.get(k).and_then(Value::as_i64))
    };
    let get_u64 = |k: &str| {
        root.get(k)
            .and_then(Value::as_u64)
            .or_else(|| event_obj.get(k).and_then(Value::as_u64))
    };
    let get_f64 = |k: &str| {
        root.get(k)
            .and_then(Value::as_f64)
            .or_else(|| event_obj.get(k).and_then(Value::as_f64))
    };

    let event_type = get_str("type")
        .or_else(|| get_str("event_type"))
        .unwrap_or("unknown");

    match event_type {
        "device_button" => get_str("event").map(|e| format!("button_event={e}")),
        "device_rotary" => {
            let action = get_str("action");
            let direction = get_str("direction");
            let steps = get_i64("steps");
            let mut parts = Vec::new();
            if let Some(v) = action {
                parts.push(format!("action={v}"));
            }
            if let Some(v) = direction {
                parts.push(format!("direction={v}"));
            }
            if let Some(v) = steps {
                parts.push(format!("steps={v}"));
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        }
        "plugin_metrics" => {
            let fallback = get_u64("eventstream_fallback_refresh_total");
            let applied = get_u64("eventstream_incremental_applied_total");
            let ratio = get_f64("eventstream_fallback_ratio_pct");
            let recent_fallback = get_u64("eventstream_fallback_refresh_recent");
            let recent_applied = get_u64("eventstream_incremental_applied_recent");
            let recent_ratio = get_f64("eventstream_fallback_ratio_recent_pct");

            let mut parts = Vec::new();
            if let (Some(f), Some(a), Some(r)) = (fallback, applied, ratio) {
                parts.push(format!(
                    "fallback={f} incremental={a} fallback_ratio={r:.2}%"
                ));
            }
            if let (Some(f), Some(a), Some(r)) = (recent_fallback, recent_applied, recent_ratio) {
                parts.push(format!(
                    "recent_fallback={f} recent_incremental={a} recent_ratio={r:.2}%"
                ));
            }

            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" | "))
            }
        }
        "entertainment_action_applied" => {
            let action = get_str("action");
            let config_id = get_str("config_id");
            let active = root
                .get("active")
                .and_then(Value::as_bool)
                .or_else(|| event_obj.get("active").and_then(Value::as_bool));

            let mut parts = Vec::new();
            if let Some(v) = action {
                parts.push(format!("action={v}"));
            }
            if let Some(v) = config_id {
                parts.push(format!("config_id={v}"));
            }
            if let Some(v) = active {
                parts.push(format!("active={v}"));
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        }
        _ => None,
    }
}
