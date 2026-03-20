use anyhow::{anyhow, Context, Result};
use reqwest::{Client, Method, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

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
    pub name: String,
    pub plugin_id: String,
    pub area: Option<String>,
    pub available: bool,
    pub attributes: Map<String, Value>,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    pub name: String,
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
    pub device_id: Option<String>,
    #[serde(default)]
    pub rule_name: Option<String>,
    #[serde(default)]
    pub event_type_custom: Option<String>,
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
        Self::parse_json(resp).await
    }

    pub async fn list_scenes(&self) -> Result<Vec<Scene>> {
        let resp = self.request(Method::GET, "/scenes").await?;
        Self::parse_json(resp).await
    }

    pub async fn list_areas(&self) -> Result<Vec<Area>> {
        let resp = self.request(Method::GET, "/areas").await?;
        Self::parse_json(resp).await
    }

    pub async fn list_automations(&self) -> Result<Vec<Rule>> {
        let resp = self.request(Method::GET, "/automations").await?;
        Self::parse_json(resp).await
    }

    pub async fn list_events(&self, limit: usize) -> Result<Vec<EventEntry>> {
        let path = format!("/events?limit={limit}");
        let resp = self.request(Method::GET, &path).await?;
        Self::parse_json(resp).await
    }

    pub async fn list_plugins(&self) -> Result<Vec<PluginRecord>> {
        let resp = self.request(Method::GET, "/plugins").await?;
        Self::parse_json(resp).await
    }

    pub async fn list_users(&self) -> Result<Vec<UserInfo>> {
        let resp = self.request(Method::GET, "/auth/users").await?;
        Self::parse_json(resp).await
    }

    pub async fn activate_scene(&self, scene_id: &str) -> Result<()> {
        let path = format!("/scenes/{scene_id}/activate");
        let resp = self.request(Method::POST, &path).await?;
        Self::parse_empty(resp).await
    }

    pub async fn set_device_on(&self, device_id: &str, on: bool) -> Result<()> {
        let path = format!("/devices/{device_id}/state");
        let body = json!({ "on": on });
        let resp = self.request_with_json(Method::PATCH, &path, body).await?;
        Self::parse_empty(resp).await
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

        let text = resp.text().await.context("failed to read login response body")?;
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
}
