//! REST client for homeCore's `/api/v1`.
//!
//! One `reqwest::Client` is shared by every clone, with connect and
//! request timeouts. Streaming responses (SSE) use a second client that
//! has a connect timeout but no overall deadline.
//!
//! The session (access token, refresh token, expiry) lives behind a
//! shared lock so a refresh made by one task is seen by every clone. A
//! request refreshes first when the access token is about to expire, and
//! on a 401 it refreshes once and retries. When that fails the error is
//! [`Unauthorized`], which the app turns into a trip back to the login
//! screen.

use crate::model::{
    Alert, Area, AuditEntry, Capabilities, DeviceState, LoginResponse, MatterNode,
    MatterNodesResponse, ModeConfig, ModeRecord, PluginRecord, Role, Rule, RuleFiring, RuleGroup,
    Scene, SystemStatus, UserInfo,
};
use anyhow::{Context, Result, anyhow};
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod settings;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Backups stream a zip of the whole state; give them longer.
const BACKUP_TIMEOUT: Duration = Duration::from_secs(120);
/// Refresh this many seconds before the access token expires.
const REFRESH_MARGIN_SECS: i64 = 120;
/// Wait this long after a failed early refresh before trying again.
const REFRESH_RETRY_SECS: i64 = 30;

/// The session expired and could not be refreshed.
#[derive(Debug)]
pub struct Unauthorized;

impl std::fmt::Display for Unauthorized {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("session expired; sign in again")
    }
}

impl std::error::Error for Unauthorized {}

pub fn is_unauthorized(err: &anyhow::Error) -> bool {
    err.downcast_ref::<Unauthorized>().is_some()
}

/// Encode one path segment.
pub fn seg(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

#[derive(Clone, Default)]
pub struct Tokens {
    pub access: Option<String>,
    pub refresh: Option<String>,
    /// Unix seconds when `access` expires, when known.
    pub expires_at: Option<i64>,
    /// Bumped whenever the tokens change, so the app knows to save them.
    pub version: u64,
    /// After a refresh fails for a reason other than a rejected token, no
    /// early refresh is tried again before this unix second.
    pub retry_refresh_after: Option<i64>,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("access", &self.access.as_ref().map(|_| "<redacted>"))
            .field("refresh", &self.refresh.as_ref().map(|_| "<redacted>"))
            .field("expires_at", &self.expires_at)
            .field("version", &self.version)
            .field("retry_refresh_after", &self.retry_refresh_after)
            .finish()
    }
}

#[derive(Clone)]
pub struct HomeCoreClient {
    http: Client,
    stream_http: Client,
    base_url: Arc<str>,
    tokens: Arc<Mutex<Tokens>>,
    refresh_gate: Arc<tokio::sync::Mutex<()>>,
}

fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

impl HomeCoreClient {
    pub fn new(base_url: String) -> Self {
        let mut normalized = base_url.trim_end_matches('/').to_string();
        if normalized.ends_with("/api/v1") {
            normalized.truncate(normalized.len() - "/api/v1".len());
        }
        let http = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| Client::new());
        let stream_http = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            http,
            stream_http,
            base_url: normalized.into(),
            tokens: Arc::new(Mutex::new(Tokens::default())),
            refresh_gate: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.base_url, path)
    }

    /// `ws://` or `wss://` form of an API path, without any token.
    pub fn ws_url(&self, path: &str) -> String {
        let base = if let Some(rest) = self.base_url.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = self.base_url.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            self.base_url.to_string()
        };
        format!("{base}/api/v1{path}")
    }

    // ── Session ───────────────────────────────────────────────────────────

    fn lock(&self) -> std::sync::MutexGuard<'_, Tokens> {
        self.tokens.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn tokens(&self) -> Tokens {
        self.lock().clone()
    }

    pub fn token(&self) -> Option<String> {
        self.lock().access.clone()
    }

    pub fn session_version(&self) -> u64 {
        self.lock().version
    }

    /// Install tokens from a login or refresh response.
    pub fn set_session_from(&self, resp: &LoginResponse) {
        let mut t = self.lock();
        t.access = Some(resp.token.clone());
        if resp.refresh_token.is_some() {
            t.refresh = resp.refresh_token.clone();
        }
        t.expires_at = resp.expires_in.map(|s| now_unix() + s as i64);
        t.version += 1;
    }

    /// Install tokens restored from disk.
    pub fn set_tokens(&self, access: String, refresh: Option<String>, expires_at: Option<i64>) {
        let mut t = self.lock();
        t.access = Some(access);
        t.refresh = refresh;
        t.expires_at = expires_at;
        t.version += 1;
    }

    pub fn clear_session(&self) {
        let mut t = self.lock();
        t.access = None;
        t.refresh = None;
        t.expires_at = None;
        t.version += 1;
    }

    /// True when the access token expires within the refresh margin and a
    /// refresh token is available.
    pub fn needs_refresh(&self) -> bool {
        let t = self.lock();
        t.refresh.is_some()
            && t.retry_refresh_after.is_none_or(|at| now_unix() >= at)
            && t.expires_at
                .map(|exp| exp - now_unix() <= REFRESH_MARGIN_SECS)
                .unwrap_or(false)
    }

    /// `POST /auth/refresh`. Refresh tokens rotate and are single use, so
    /// concurrent callers wait on a gate; the second one sees the version
    /// moved and does nothing.
    pub async fn refresh_session(&self) -> Result<()> {
        let seen = self.session_version();
        let _gate = self.refresh_gate.lock().await;
        if self.session_version() != seen {
            return Ok(());
        }
        let Some(refresh) = self.lock().refresh.clone() else {
            return Err(Unauthorized.into());
        };
        let resp = match self
            .http
            .post(self.endpoint("/auth/refresh"))
            .json(&json!({ "refresh_token": refresh }))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                self.lock().retry_refresh_after = Some(now_unix() + REFRESH_RETRY_SECS);
                return Err(anyhow!("refresh request failed: {}", e.without_url()));
            }
        };
        if !resp.status().is_success() {
            // A rejected refresh token is final: forget it.
            if matches!(
                resp.status(),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            ) {
                self.lock().refresh = None;
                return Err(Unauthorized.into());
            }
            self.lock().retry_refresh_after = Some(now_unix() + REFRESH_RETRY_SECS);
            let status = resp.status();
            return Err(anyhow!("{status}: {}", error_message(resp).await));
        }
        let parsed: LoginResponse = resp.json().await.context("parsing refresh response")?;
        self.set_session_from(&parsed);
        self.lock().retry_refresh_after = None;
        Ok(())
    }

    // ── Plumbing ─────────────────────────────────────────────────────────

    fn authed(&self, rb: RequestBuilder) -> Result<RequestBuilder> {
        let token = self.token().ok_or(Unauthorized)?;
        Ok(rb.bearer_auth(token))
    }

    pub(crate) async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Response> {
        self.request_with(&self.http, method, path, body, None)
            .await
    }

    async fn request_with(
        &self,
        client: &Client,
        method: Method,
        path: &str,
        body: Option<&Value>,
        timeout: Option<Duration>,
    ) -> Result<Response> {
        if self.needs_refresh() {
            // A failed early refresh is not fatal: the token may still be
            // good for a moment, and a 401 below gets one more try.
            let _ = self.refresh_session().await;
        }
        let build = || -> Result<RequestBuilder> {
            let mut rb = client.request(method.clone(), self.endpoint(path));
            if let Some(b) = body {
                rb = rb.json(b);
            }
            if let Some(t) = timeout {
                rb = rb.timeout(t);
            }
            self.authed(rb)
        };
        let resp = build()?
            .send()
            .await
            .map_err(|e| anyhow!("request failed: {path}: {}", e.without_url()))?;
        if resp.status() != StatusCode::UNAUTHORIZED {
            return Ok(resp);
        }
        self.refresh_session().await?;
        let retry = build()?
            .send()
            .await
            .map_err(|e| anyhow!("request failed: {path}: {}", e.without_url()))?;
        if retry.status() == StatusCode::UNAUTHORIZED {
            return Err(Unauthorized.into());
        }
        Ok(retry)
    }

    pub(crate) async fn get_json<T: for<'a> Deserialize<'a>>(&self, path: &str) -> Result<T> {
        let resp = self.request(Method::GET, path, None).await?;
        parse_json(path, resp).await
    }

    pub(crate) async fn send_json<T: for<'a> Deserialize<'a>>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T> {
        let resp = self.request(method, path, body.as_ref()).await?;
        parse_json(path, resp).await
    }

    pub(crate) async fn send_empty(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<()> {
        let resp = self.request(method, path, body.as_ref()).await?;
        check_status(resp).await.map(|_| ())
    }

    // ── Auth ──────────────────────────────────────────────────────────────

    /// `POST /auth/login`. Does not touch the stored session; the caller
    /// installs the tokens once it decides to.
    pub async fn login(&self, username: &str, password: &str) -> Result<LoginResponse> {
        let resp = self
            .http
            .post(self.endpoint("/auth/login"))
            .json(&json!({ "username": username, "password": password }))
            .send()
            .await
            .map_err(|e| anyhow!("login request failed: {}", e.without_url()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(anyhow!("{status}: {}", error_message(resp).await));
        }
        let text = resp.text().await.context("reading login response")?;
        let value: Value = serde_json::from_str(&text).context("login response was not JSON")?;
        // Some proxies wrap the body in `data`.
        let body = value.get("data").cloned().unwrap_or(value);
        serde_json::from_value(body).context("parsing login response")
    }

    /// `POST /auth/logout`: revoke `refresh_token` server side. Core
    /// answers 204 whatever the token; the local session is the caller's
    /// to clear.
    pub async fn revoke(&self, refresh_token: String) -> Result<()> {
        let resp = self
            .http
            .post(self.endpoint("/auth/logout"))
            .json(&json!({ "refresh_token": refresh_token }))
            .send()
            .await
            .map_err(|e| anyhow!("logout request failed: {}", e.without_url()))?;
        check_status(resp).await.map(|_| ())
    }

    pub async fn me(&self) -> Result<UserInfo> {
        self.get_json("/auth/me").await
    }

    /// `POST /auth/change-password`. Core answers 204 and revokes every
    /// session, including this one.
    /// `PUT /auth/pin`: your own PIN for displays, with your password.
    /// 409 when someone else has it.
    pub async fn set_pin(&self, pin: &str, password: &str) -> Result<()> {
        self.send_empty(
            Method::PUT,
            "/auth/pin",
            Some(serde_json::json!({ "pin": pin, "password": password })),
        )
        .await
    }

    pub async fn change_password(&self, current_password: &str, new_password: &str) -> Result<()> {
        // A 401 here means the current password was wrong, not that the
        // session expired, so this request skips the refresh-and-retry.
        let body = json!({ "current_password": current_password, "new_password": new_password });
        let rb = self
            .http
            .post(self.endpoint("/auth/change-password"))
            .json(&body);
        let resp = self
            .authed(rb)?
            .send()
            .await
            .map_err(|e| anyhow!("change password failed: {}", e.without_url()))?;
        if resp.status().is_success() {
            return Ok(());
        }
        let status = resp.status();
        Err(anyhow!("{status}: {}", error_message(resp).await))
    }

    // ── Devices ──────────────────────────────────────────────────────────

    pub async fn list_devices(&self) -> Result<Vec<DeviceState>> {
        self.get_json("/devices").await
    }

    /// `PATCH /devices/{id}/state` with a command body.
    pub async fn patch_device_state(&self, device_id: &str, body: Value) -> Result<()> {
        let path = format!("/devices/{}/state", seg(device_id));
        self.send_empty(Method::PATCH, &path, Some(body)).await
    }

    pub async fn send_device_action(&self, device_id: &str, action: &str) -> Result<()> {
        self.patch_device_state(device_id, json!({ "action": action }))
            .await
    }

    /// `PATCH /devices/{id}` with only the fields that changed. `name`
    /// sets the override (null clears it); `area` sets the room override.
    pub async fn update_device(&self, device_id: &str, fields: Map<String, Value>) -> Result<()> {
        let path = format!("/devices/{}", seg(device_id));
        self.send_empty(Method::PATCH, &path, Some(Value::Object(fields)))
            .await
    }

    // ── Scenes ───────────────────────────────────────────────────────────

    pub async fn list_scenes(&self) -> Result<Vec<Scene>> {
        self.get_json("/scenes").await
    }

    pub async fn activate_scene(&self, scene_id: &str) -> Result<()> {
        let path = format!("/scenes/{}/activate", seg(scene_id));
        self.send_empty(Method::POST, &path, None).await
    }

    pub async fn list_rotations(&self) -> Result<Vec<crate::model::Rotation>> {
        self.get_json("/rotations").await
    }

    /// Play a rotation's choice for now; homeCore's answer says what played.
    pub async fn play_rotation(&self, rotation_id: &str) -> Result<Value> {
        let path = format!("/rotations/{}/play", seg(rotation_id));
        self.send_json(Method::POST, &path, None).await
    }

    /// Move a rotation on without playing; the answer says what's next.
    pub async fn advance_rotation(&self, rotation_id: &str) -> Result<Value> {
        let path = format!("/rotations/{}/advance", seg(rotation_id));
        self.send_json(Method::POST, &path, None).await
    }

    pub async fn export_scenes(&self) -> Result<Value> {
        self.get_json("/scenes/export").await
    }

    pub async fn import_scenes(&self, scenes: Value) -> Result<usize> {
        let body: Value = self
            .send_json(Method::POST, "/scenes/import", Some(scenes))
            .await?;
        Ok(body.get("imported").and_then(Value::as_u64).unwrap_or(0) as usize)
    }

    // ── Areas ────────────────────────────────────────────────────────────

    pub async fn list_areas(&self) -> Result<Vec<Area>> {
        self.get_json("/areas").await
    }

    pub async fn create_area(&self, name: &str) -> Result<Area> {
        self.send_json(Method::POST, "/areas", Some(json!({ "name": name })))
            .await
    }

    pub async fn rename_area(&self, id: &str, name: &str) -> Result<Area> {
        let path = format!("/areas/{}", seg(id));
        self.send_json(Method::PATCH, &path, Some(json!({ "name": name })))
            .await
    }

    pub async fn delete_area(&self, id: &str) -> Result<()> {
        let path = format!("/areas/{}", seg(id));
        self.send_empty(Method::DELETE, &path, None).await
    }

    /// `GET /areas/{id}/saved-lights`: what Room off saved, or `None` (204)
    /// when it has saved nothing for the room.
    pub async fn room_saved_lights(&self, id: &str) -> Result<Option<Value>> {
        let path = format!("/areas/{}/saved-lights", seg(id));
        let resp = self.request(Method::GET, &path, None).await?;
        if resp.status() == StatusCode::NO_CONTENT {
            return Ok(None);
        }
        parse_json(&path, resp).await.map(Some)
    }

    /// `POST /areas/{id}/merge {into}`: its devices move into `into`, which
    /// also answers to its names from now on; the room is deleted.
    pub async fn merge_area(&self, id: &str, into: &str) -> Result<()> {
        let path = format!("/areas/{}/merge", seg(id));
        let resp = self
            .request(Method::POST, &path, Some(&json!({ "into": into })))
            .await?;
        check_status(resp).await.map(|_| ())
    }

    /// `POST /areas/{id}/on` or `/off`: every light in the room.
    pub async fn room_switch(&self, id: &str, on: bool) -> Result<()> {
        let verb = if on { "on" } else { "off" };
        let path = format!("/areas/{}/{verb}", seg(id));
        self.send_empty(Method::POST, &path, None).await
    }

    // ── Automations ──────────────────────────────────────────────────────

    pub async fn list_rules(&self) -> Result<Vec<Rule>> {
        self.get_json("/automations").await
    }

    pub async fn get_rule_history(&self, id: &str) -> Result<Vec<RuleFiring>> {
        let path = format!("/automations/{}/history", seg(id));
        self.get_json(&path).await
    }

    /// `GET /automations/{id}/ron`, the on-disk RON as plain text.
    pub async fn get_rule_ron(&self, id: &str) -> Result<String> {
        let path = format!("/automations/{}/ron", seg(id));
        let resp = self.request(Method::GET, &path, None).await?;
        let resp = check_status(resp).await?;
        resp.text().await.context("reading rule RON body")
    }

    pub async fn clone_rule(&self, id: &str) -> Result<Rule> {
        let path = format!("/automations/{}/clone", seg(id));
        self.send_json(Method::POST, &path, None).await
    }

    pub async fn delete_rule(&self, id: &str) -> Result<()> {
        let path = format!("/automations/{}", seg(id));
        self.send_empty(Method::DELETE, &path, None).await
    }

    pub async fn toggle_rule(&self, id: &str, enabled: bool) -> Result<()> {
        let path = format!("/automations/{}", seg(id));
        self.send_empty(Method::PATCH, &path, Some(json!({ "enabled": enabled })))
            .await
    }

    pub async fn bulk_toggle_rules(&self, ids: &[String], enabled: bool) -> Result<()> {
        let body = json!({ "ids": ids, "enabled": enabled });
        self.send_empty(Method::PATCH, "/automations", Some(body))
            .await
    }

    /// `GET /automations/variables`: the hub variables rules have set, by
    /// name, sorted. In memory on homeCore: none after a restart until a rule
    /// sets one.
    pub async fn list_hub_variables(&self) -> Result<Vec<(String, Value)>> {
        let v: Value = self.get_json("/automations/variables").await?;
        let mut out: Vec<(String, Value)> = v
            .get("hub")
            .and_then(Value::as_object)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    pub async fn list_rule_groups(&self) -> Result<Vec<RuleGroup>> {
        self.get_json("/automations/groups").await
    }

    pub async fn delete_rule_group(&self, id: &str) -> Result<()> {
        let path = format!("/automations/groups/{}", seg(id));
        self.send_empty(Method::DELETE, &path, None).await
    }

    pub async fn set_rule_group_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let verb = if enabled { "enable" } else { "disable" };
        let path = format!("/automations/groups/{}/{verb}", seg(id));
        self.send_empty(Method::POST, &path, None).await
    }

    pub async fn export_rules(&self) -> Result<Value> {
        self.get_json("/automations/export").await
    }

    pub async fn import_rules(&self, rules: Value) -> Result<usize> {
        let body: Value = self
            .send_json(Method::POST, "/automations/import", Some(rules))
            .await?;
        Ok(body.get("imported").and_then(Value::as_u64).unwrap_or(0) as usize)
    }

    // ── System, events, audit ────────────────────────────────────────────

    pub async fn get_system_status(&self) -> Result<SystemStatus> {
        self.get_json("/system/status").await
    }

    /// `GET /events?limit=`, newest first. Each entry is
    /// `{seq, event_type, device_id?, event}`; the app turns them into
    /// rows.
    pub async fn list_events(&self, limit: usize) -> Result<Vec<Value>> {
        self.get_json(&format!("/events?limit={limit}")).await
    }

    /// `GET /audit`, with `filter` as extra query pairs (`event_type`,
    /// `result`), filtered by homeCore over the whole log.
    pub async fn list_audit(
        &self,
        limit: u32,
        offset: u32,
        filter: &[(&str, &str)],
    ) -> Result<Vec<AuditEntry>> {
        let mut path = format!("/audit?limit={limit}&offset={offset}");
        for (k, v) in filter {
            path.push_str(&format!("&{k}={}", urlencoding::encode(v)));
        }
        self.get_json(&path).await
    }

    /// Raw zip bytes from `POST /system/backup`.
    pub async fn backup_zip(&self) -> Result<Vec<u8>> {
        let resp = self
            .request_with(
                &self.http,
                Method::POST,
                "/system/backup",
                None,
                Some(BACKUP_TIMEOUT),
            )
            .await?;
        let resp = check_status(resp).await?;
        let bytes = resp.bytes().await.context("reading backup body")?;
        Ok(bytes.to_vec())
    }

    // ── Users ────────────────────────────────────────────────────────────

    pub async fn list_users(&self) -> Result<Vec<UserInfo>> {
        self.get_json("/auth/users").await
    }

    pub async fn create_user(
        &self,
        username: &str,
        password: &str,
        role: Role,
    ) -> Result<UserInfo> {
        let body = json!({ "username": username, "password": password, "role": role });
        self.send_json(Method::POST, "/auth/users", Some(body))
            .await
    }

    pub async fn delete_user(&self, id: &str) -> Result<()> {
        let path = format!("/auth/users/{}", seg(id));
        self.send_empty(Method::DELETE, &path, None).await
    }

    pub async fn set_user_role(&self, id: &str, role: Role) -> Result<UserInfo> {
        let path = format!("/auth/users/{}/role", seg(id));
        self.send_json(Method::PATCH, &path, Some(json!({ "role": role })))
            .await
    }

    // ── Alerts ───────────────────────────────────────────────────────────

    pub async fn list_alerts(&self) -> Result<Vec<Alert>> {
        self.get_json("/alerts").await
    }

    // ── Modes ────────────────────────────────────────────────────────────

    pub async fn list_modes(&self) -> Result<Vec<ModeRecord>> {
        self.get_json("/modes").await
    }

    pub async fn create_mode(&self, id: &str, name: &str, kind: &str) -> Result<ModeConfig> {
        let body = json!({ "id": id, "name": name, "kind": kind });
        self.send_json(Method::POST, "/modes", Some(body)).await
    }

    pub async fn delete_mode(&self, id: &str) -> Result<()> {
        let path = format!("/modes/{}", seg(id));
        self.send_empty(Method::DELETE, &path, None).await
    }

    // ── Helpers (glue devices) ───────────────────────────────────────────

    /// `POST /glue`. `type` is one of switch, timer, counter, number,
    /// select, text, button, datetime, group, threshold, schedule.
    pub async fn create_glue(
        &self,
        id: &str,
        name: &str,
        glue_type: &str,
        config: Value,
    ) -> Result<DeviceState> {
        let body = json!({ "id": id, "name": name, "type": glue_type, "config": config });
        self.send_json(Method::POST, "/glue", Some(body)).await
    }

    // ── Plugins ──────────────────────────────────────────────────────────

    pub async fn list_plugins(&self) -> Result<Vec<PluginRecord>> {
        self.get_json("/plugins").await
    }

    /// `DELETE /plugins/{id}`: stop the plugin, remove its devices and
    /// its registration. Returns core's summary.
    pub async fn uninstall_plugin(&self, plugin_id: &str) -> Result<Value> {
        let path = format!("/plugins/{}", seg(plugin_id));
        let resp = self.request(Method::DELETE, &path, None).await?;
        let resp = check_status(resp).await?;
        let text = resp.text().await.unwrap_or_default();
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub async fn get_plugin_capabilities(&self, plugin_id: &str) -> Result<Capabilities> {
        let path = format!("/plugins/{}/capabilities", seg(plugin_id));
        self.get_json(&path).await
    }

    /// `POST /plugins/{id}/upload/{action}`: run an action with a file. The
    /// body is the file; `name` and every other param go in the query (as
    /// strings, the way core passes them on). The reply is `command`'s.
    pub async fn post_plugin_upload(
        &self,
        plugin_id: &str,
        action: &str,
        name: &str,
        file: Vec<u8>,
        params: &serde_json::Map<String, Value>,
    ) -> Result<Value> {
        let mut path = format!(
            "/plugins/{}/upload/{}?name={}",
            seg(plugin_id),
            seg(action),
            seg(name)
        );
        for (k, v) in params {
            let v = match v {
                Value::String(s) => s.clone(),
                Value::Null => continue,
                other => other.to_string(),
            };
            path.push_str(&format!("&{}={}", seg(k), seg(&v)));
        }
        let build = || -> Result<RequestBuilder> {
            let rb = self
                .http
                .request(Method::POST, self.endpoint(&path))
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                // Up to 32 MiB, which 20 seconds may not carry; as long as a backup.
                .timeout(BACKUP_TIMEOUT)
                .body(file.clone());
            self.authed(rb)
        };
        let mut resp = build()?
            .send()
            .await
            .map_err(|e| anyhow!("request failed: upload: {}", e.without_url()))?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            self.refresh_session().await?;
            resp = build()?
                .send()
                .await
                .map_err(|e| anyhow!("request failed: upload: {}", e.without_url()))?;
        }
        let resp = check_status(resp).await?;
        let text = resp.text().await.unwrap_or_default();
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// `POST /plugins/{id}/query`: a read-only action, which core answers
    /// for anyone who can read devices (`command` needs `plugins:write`).
    pub async fn post_plugin_query(
        &self,
        plugin_id: &str,
        action: &str,
        params: Value,
    ) -> Result<Value> {
        let path = format!("/plugins/{}/query", seg(plugin_id));
        let body = build_plugin_command_body(action, params);
        let resp = self.request(Method::POST, &path, Some(&body)).await?;
        let resp = check_status(resp).await?;
        let text = resp.text().await.unwrap_or_default();
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// `POST /plugins/{id}/command`. The body is `{action, ...params}`.
    /// A busy single-concurrency stream answers 409 with
    /// `{status:"busy", active_request_id}`; that comes back as `Ok`.
    pub async fn post_plugin_command(
        &self,
        plugin_id: &str,
        action: &str,
        params: Value,
    ) -> Result<Value> {
        let path = format!("/plugins/{}/command", seg(plugin_id));
        let body = build_plugin_command_body(action, params);
        let resp = self.request(Method::POST, &path, Some(&body)).await?;
        if resp.status() == StatusCode::CONFLICT {
            let text = resp.text().await.unwrap_or_default();
            if let Ok(v) = serde_json::from_str::<Value>(&text)
                && v.get("status").and_then(Value::as_str) == Some("busy")
            {
                return Ok(v);
            }
            return Err(anyhow!("409 Conflict: {text}"));
        }
        parse_json(&path, resp).await
    }

    pub async fn cancel_streaming_action(
        &self,
        plugin_id: &str,
        request_id: &str,
    ) -> Result<Value> {
        let body = json!({ "target_request_id": request_id });
        self.post_plugin_command(plugin_id, "cancel", body).await
    }

    pub async fn respond_streaming_action(
        &self,
        plugin_id: &str,
        request_id: &str,
        response: Value,
    ) -> Result<Value> {
        let body = json!({ "target_request_id": request_id, "response": response });
        self.post_plugin_command(plugin_id, "respond", body).await
    }

    /// Open `GET /plugins/{id}/command/{request_id}/stream` with the
    /// bearer header (never a `?token=` that could leak into an error).
    pub async fn open_action_stream(&self, plugin_id: &str, request_id: &str) -> Result<Response> {
        let path = format!(
            "/plugins/{}/command/{}/stream",
            seg(plugin_id),
            seg(request_id)
        );
        let rb = self
            .stream_http
            .get(self.endpoint(&path))
            .header("Accept", "text/event-stream");
        let resp = self
            .authed(rb)?
            .send()
            .await
            .map_err(|e| anyhow!("stream connect failed: {}", e.without_url()))?;
        check_status(resp).await
    }

    // ── Matter ───────────────────────────────────────────────────────────

    pub async fn list_matter_nodes(&self) -> Result<Vec<MatterNode>> {
        let body: MatterNodesResponse = self.get_json("/plugins/matter/nodes").await?;
        Ok(body.nodes)
    }

    pub async fn matter_commission(&self, payload: Value) -> Result<()> {
        self.send_empty(Method::POST, "/plugins/matter/commission", Some(payload))
            .await
    }

    pub async fn matter_reinterview(&self, node_id: &str) -> Result<()> {
        let body = json!({ "node_id": node_id });
        self.send_empty(Method::POST, "/plugins/matter/reinterview", Some(body))
            .await
    }

    pub async fn matter_remove_node(&self, node_id: &str) -> Result<()> {
        let path = format!("/plugins/matter/nodes/{}", seg(node_id));
        self.send_empty(Method::DELETE, &path, None).await
    }
}

/// Reshape a plugin-action invocation into `{ action, ...params }`.
/// Non-object `params` collapse to an empty object.
pub fn build_plugin_command_body(action: &str, params: Value) -> Value {
    let mut map = match params {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    map.insert("action".to_string(), Value::String(action.to_string()));
    Value::Object(map)
}

pub(crate) async fn check_status(resp: Response) -> Result<Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    if status == StatusCode::UNAUTHORIZED {
        return Err(Unauthorized.into());
    }
    Err(anyhow!("{status}: {}", error_message(resp).await))
}

pub(crate) async fn parse_json<T: for<'a> Deserialize<'a>>(
    path: &str,
    resp: Response,
) -> Result<T> {
    let resp = check_status(resp).await?;
    let text = resp.text().await.context("reading response body")?;
    parse_body(path, &text)
}

/// Parse a JSON body, naming the endpoint and a short snippet on error.
pub fn parse_body<T: for<'a> Deserialize<'a>>(path: &str, text: &str) -> Result<T> {
    serde_json::from_str::<T>(text).map_err(|e| {
        let snippet = crate::text::take_chars(text, 200);
        anyhow!("{path}: unexpected response ({e}): {snippet}")
    })
}

/// Core error bodies are `{"error": "..."}`.
async fn error_message(resp: Response) -> String {
    let text = resp.text().await.unwrap_or_default();
    error_message_from(&text)
}

pub fn error_message_from(text: &str) -> String {
    if text.trim().is_empty() {
        return "request failed".to_string();
    }
    if let Ok(body) = serde_json::from_str::<Value>(text) {
        for key in ["error", "message"] {
            if let Some(msg) = body.get(key).and_then(Value::as_str) {
                return msg.to_string();
            }
        }
    }
    crate::text::truncate(text.trim(), 200)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_segments_are_encoded() {
        assert_eq!(seg("a b/c"), "a%20b%2Fc");
        assert_eq!(seg("hue_001"), "hue_001");
    }

    #[test]
    fn command_body_flattens_params() {
        let body = build_plugin_command_body("scan", json!({"timeout": 5}));
        assert_eq!(body, json!({"action": "scan", "timeout": 5}));
        let body = build_plugin_command_body("scan", Value::Null);
        assert_eq!(body, json!({"action": "scan"}));
    }

    #[test]
    fn error_messages_come_from_the_body() {
        assert_eq!(error_message_from(r#"{"error":"nope"}"#), "nope");
        assert_eq!(error_message_from(""), "request failed");
        assert_eq!(error_message_from("plain"), "plain");
    }

    #[test]
    fn ws_urls_swap_the_scheme() {
        let c = HomeCoreClient::new("https://hc.local/api/v1/".into());
        assert_eq!(
            c.ws_url("/events/stream"),
            "wss://hc.local/api/v1/events/stream"
        );
        let c = HomeCoreClient::new("http://127.0.0.1:8080".into());
        assert_eq!(
            c.ws_url("/logs/stream"),
            "ws://127.0.0.1:8080/api/v1/logs/stream"
        );
    }

    #[test]
    fn tokens_debug_redacts() {
        let t = Tokens {
            access: Some("secret".into()),
            refresh: Some("secret2".into()),
            expires_at: None,
            version: 1,
            retry_refresh_after: None,
        };
        let s = format!("{t:?}");
        assert!(!s.contains("secret"));
    }

    #[test]
    fn needs_refresh_near_expiry() {
        let c = HomeCoreClient::new("http://x".into());
        c.set_tokens("a".into(), Some("r".into()), Some(now_unix() + 30));
        assert!(c.needs_refresh());
        c.set_tokens("a".into(), Some("r".into()), Some(now_unix() + 3600));
        assert!(!c.needs_refresh());
        c.set_tokens("a".into(), None, Some(now_unix() + 30));
        assert!(!c.needs_refresh());
        c.set_tokens("a".into(), Some("r".into()), Some(now_unix() + 30));
        c.lock().retry_refresh_after = Some(now_unix() + 30);
        assert!(!c.needs_refresh(), "a failed refresh backs off");
    }
}
