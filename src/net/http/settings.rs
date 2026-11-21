//! Calls for the Settings screens: people (roles, passwords, PINs, API
//! keys, sessions), data (kept backups, restore, calendars) and system
//! (health, versions, log level, connections, restart).
//!
//! A child of `http`, so it shares the session plumbing: every call
//! refreshes and retries like the rest.

use super::{BACKUP_TIMEOUT, HomeCoreClient, Unauthorized, check_status, seg};
use crate::model::SystemStatus;
use crate::model::settings::{
    ApiKey, Calendar, Health, HouseTemperature, KeptBackups, NewApiKey, RestorePlan, RoleScopes,
    SessionInfo, WsConnections,
};
use anyhow::{Context, Result, anyhow};
use reqwest::{Method, Response, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

/// How long a restart may take before we stop waiting for it.
const RESTART_WAIT: Duration = Duration::from_secs(90);

impl HomeCoreClient {
    // ── People ───────────────────────────────────────────────────────────

    pub async fn list_roles(&self) -> Result<Vec<RoleScopes>> {
        self.get_json("/auth/roles").await
    }

    /// `PATCH /auth/users/{id}/password` (admin): a new password for
    /// someone else. Core signs them out everywhere.
    pub async fn set_user_password(&self, id: &str, new_password: &str) -> Result<()> {
        let path = format!("/auth/users/{}/password", seg(id));
        let body = json!({ "new_password": new_password });
        self.send_empty(Method::PATCH, &path, Some(body)).await
    }

    /// `DELETE /auth/users/{id}/pin` (admin): forget a person's PIN.
    pub async fn clear_user_pin(&self, id: &str) -> Result<()> {
        let path = format!("/auth/users/{}/pin", seg(id));
        self.send_empty(Method::DELETE, &path, None).await
    }

    pub async fn list_api_keys(&self) -> Result<Vec<ApiKey>> {
        self.get_json("/auth/api-keys").await
    }

    /// `POST /auth/api-keys`. The answer holds the secret, once.
    pub async fn create_api_key(&self, label: &str, scopes: &[String]) -> Result<NewApiKey> {
        let body = json!({ "label": label, "scopes": scopes });
        self.send_json(Method::POST, "/auth/api-keys", Some(body))
            .await
    }

    /// `POST /auth/api-keys/{id}/rotate`: a new secret, shown once; the
    /// old one stops working.
    pub async fn rotate_api_key(&self, id: &str) -> Result<NewApiKey> {
        let path = format!("/auth/api-keys/{}/rotate", seg(id));
        self.send_json(Method::POST, &path, None).await
    }

    pub async fn revoke_api_key(&self, id: &str) -> Result<()> {
        let path = format!("/auth/api-keys/{}", seg(id));
        self.send_empty(Method::DELETE, &path, None).await
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        self.get_json("/auth/sessions").await
    }

    pub async fn end_session(&self, id: &str) -> Result<()> {
        let path = format!("/auth/sessions/{}", seg(id));
        self.send_empty(Method::DELETE, &path, None).await
    }

    // ── Data and backups ─────────────────────────────────────────────────

    pub async fn list_kept_backups(&self) -> Result<KeptBackups> {
        self.get_json("/system/backups").await
    }

    /// The zip of one kept backup.
    pub async fn download_kept_backup(&self, name: &str) -> Result<Vec<u8>> {
        let path = format!("/system/backups/{}", seg(name));
        let resp = self
            .request_with(&self.http, Method::GET, &path, None, Some(BACKUP_TIMEOUT))
            .await?;
        let resp = check_status(resp).await?;
        Ok(resp.bytes().await.context("reading backup body")?.to_vec())
    }

    /// Check (`check`) or stage a kept backup for the next start.
    pub async fn restore_kept_backup(&self, name: &str, check: bool) -> Result<RestorePlan> {
        let mut path = format!("/system/backups/{}/restore", seg(name));
        if check {
            path.push_str("?check=true");
        }
        let resp = self
            .request_with(&self.http, Method::POST, &path, None, Some(BACKUP_TIMEOUT))
            .await?;
        super::parse_json(&path, resp).await
    }

    /// Check (`check`) or stage an uploaded backup zip.
    pub async fn restore_backup_file(&self, zip: Vec<u8>, check: bool) -> Result<RestorePlan> {
        let path = if check {
            "/system/restore?check=true"
        } else {
            "/system/restore"
        };
        let resp = self.post_bytes(path, zip, "application/zip").await?;
        super::parse_json(path, resp).await
    }

    pub async fn list_calendars(&self) -> Result<Vec<Calendar>> {
        self.get_json("/calendars").await
    }

    /// `POST /calendars/upload` with the text of a `.ics` file. Returns
    /// the calendar's name and how many events it holds.
    pub async fn upload_calendar(&self, name: &str, content: &str) -> Result<(String, usize)> {
        let body = json!({ "content": content, "name": name });
        let v: Value = self
            .send_json(Method::POST, "/calendars/upload", Some(body))
            .await?;
        Ok((
            v.get("calendar_id")
                .and_then(Value::as_str)
                .unwrap_or(name)
                .to_string(),
            v.get("event_count").and_then(Value::as_u64).unwrap_or(0) as usize,
        ))
    }

    /// `POST /calendars/fetch`: core downloads the `.ics` at `url` and keeps
    /// it (and refreshes it). Returns the calendar's name and its events.
    pub async fn fetch_calendar(&self, url: &str, name: Option<&str>) -> Result<(String, usize)> {
        let mut body = json!({ "url": url });
        if let Some(n) = name {
            body["name"] = json!(n);
        }
        let v: Value = self
            .send_json(Method::POST, "/calendars/fetch", Some(body))
            .await?;
        Ok((
            v.get("calendar_id")
                .or_else(|| v.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            v.get("event_count").and_then(Value::as_u64).unwrap_or(0) as usize,
        ))
    }

    /// `GET /calendars/{id}/events`: the next `limit` occurrences from now.
    pub async fn calendar_events(&self, id: &str, limit: usize) -> Result<Vec<Value>> {
        let path = format!("/calendars/{}/events?limit={limit}", seg(id));
        let v: Value = self.get_json(&path).await?;
        Ok(v.get("events")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| v.as_array().cloned().unwrap_or_default()))
    }

    /// `DELETE /calendars/{id}`. Returns the names of rules that still
    /// check it.
    pub async fn delete_calendar(&self, id: &str) -> Result<Vec<String>> {
        let path = format!("/calendars/{}", seg(id));
        let v: Value = self.send_json(Method::DELETE, &path, None).await?;
        Ok(v.get("referencing_rules")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|r| r.get("rule_name").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default())
    }

    // ── System ───────────────────────────────────────────────────────────

    pub async fn health(&self) -> Result<Health> {
        self.get_json("/health").await
    }

    pub async fn system_versions(&self) -> Result<Value> {
        self.get_json("/system/versions").await
    }

    pub async fn get_log_level(&self) -> Result<String> {
        let v: Value = self.get_json("/system/log-level").await?;
        v.get("level")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("/system/log-level: no level"))
    }

    pub async fn set_log_level(&self, directive: &str) -> Result<()> {
        let body = json!({ "level": directive });
        self.send_empty(Method::PUT, "/system/log-level", Some(body))
            .await
    }

    pub async fn ws_connections(&self) -> Result<WsConnections> {
        self.get_json("/ws/connections").await
    }

    pub async fn house_temperature(&self) -> Result<HouseTemperature> {
        self.get_json("/system/temperature-unit").await
    }

    /// `POST /system/restart`, then wait until core answers with a new
    /// `started_at`: the old process answers for a moment after agreeing.
    /// Returns the seconds it took.
    pub async fn restart_and_wait(&self) -> Result<u64> {
        let before: Option<SystemStatus> = self.get_json("/system/status").await.ok();
        let was = before.map(|s| s.started_at);
        self.send_empty(Method::POST, "/system/restart", None)
            .await?;
        let t0 = std::time::Instant::now();
        while t0.elapsed() < RESTART_WAIT {
            tokio::time::sleep(Duration::from_secs(1)).await;
            match self.get_json::<SystemStatus>("/system/status").await {
                Ok(now) if Some(&now.started_at) != was.as_ref() => {
                    return Ok(t0.elapsed().as_secs());
                }
                Err(e) if crate::net::is_unauthorized(&e) => return Err(e),
                _ => {}
            }
        }
        Err(anyhow!(
            "homeCore hasn't come back yet. Whatever runs it (systemd, Docker) should start it again; check there."
        ))
    }

    // ── Plumbing ─────────────────────────────────────────────────────────

    /// POST a raw body (a zip), with the same refresh and one retry on
    /// 401 as JSON requests.
    async fn post_bytes(&self, path: &str, body: Vec<u8>, content_type: &str) -> Result<Response> {
        if self.needs_refresh() {
            let _ = self.refresh_session().await;
        }
        let send = |body: Vec<u8>| {
            let rb = self
                .http
                .post(self.endpoint(path))
                .header(reqwest::header::CONTENT_TYPE, content_type)
                .timeout(BACKUP_TIMEOUT)
                .body(body);
            async move {
                self.authed(rb)?
                    .send()
                    .await
                    .map_err(|e| anyhow!("request failed: {path}: {}", e.without_url()))
            }
        };
        let resp = send(body.clone()).await?;
        if resp.status() != StatusCode::UNAUTHORIZED {
            return Ok(resp);
        }
        self.refresh_session().await?;
        let retry = send(body).await?;
        if retry.status() == StatusCode::UNAUTHORIZED {
            return Err(Unauthorized.into());
        }
        Ok(retry)
    }
}
