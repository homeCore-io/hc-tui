//! Plugin management calls: start and stop, enable, settings, the
//! registry, install and plugin runtimes. Kept apart from `http.rs` so
//! the Plugins screen's calls live in one place.

use super::http::{HomeCoreClient, Unauthorized, seg};
use crate::plugin_admin::{
    IssuedKey, Placement, PlacementList, RegistryIndex, RegistryPlugin, Runtime, RuntimeList,
};
use anyhow::{Result, anyhow};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// An answer kept whole, for calls whose refusals carry data (an install
/// that needs a runtime chosen) or whose status means something (a
/// stopped plugin's settings).
#[derive(Debug, Clone)]
pub struct Answer {
    pub status: u16,
    pub body: Value,
}

impl Answer {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Core's own words for a refusal.
    pub fn error(&self) -> String {
        self.body
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("homeCore said no ({})", self.status))
    }
}

/// Lifecycle verbs core takes at `POST /plugins/{id}/{verb}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    Start,
    Stop,
    Restart,
}

impl Lifecycle {
    pub fn path(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }
}

impl HomeCoreClient {
    /// Any call, its status and body kept. A 401 is still the session
    /// ending.
    pub async fn answer(&self, method: Method, path: &str, body: Option<Value>) -> Result<Answer> {
        let resp = self.request(method, path, body.as_ref()).await?;
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(Unauthorized.into());
        }
        let text = resp.text().await.unwrap_or_default();
        let body = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        Ok(Answer {
            status: status.as_u16(),
            body,
        })
    }

    async fn ok_answer(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let a = self.answer(method, path, body).await?;
        if a.ok() {
            Ok(a.body)
        } else {
            Err(anyhow!("{}", a.error()))
        }
    }

    /// `PATCH /plugins/{id}` `{enabled}`: turned on or off for good.
    pub async fn set_plugin_enabled(&self, plugin_id: &str, enabled: bool) -> Result<()> {
        let path = format!("/plugins/{}", seg(plugin_id));
        self.ok_answer(Method::PATCH, &path, Some(json!({ "enabled": enabled })))
            .await
            .map(|_| ())
    }

    /// `PATCH /plugins/{id}` `{log_level}`: until the plugin restarts.
    pub async fn set_plugin_log_level(&self, plugin_id: &str, level: &str) -> Result<()> {
        let path = format!("/plugins/{}", seg(plugin_id));
        self.ok_answer(Method::PATCH, &path, Some(json!({ "log_level": level })))
            .await
            .map(|_| ())
    }

    /// `POST /plugins/{id}/start|stop|restart`.
    pub async fn plugin_lifecycle(&self, plugin_id: &str, verb: Lifecycle) -> Result<()> {
        let path = format!("/plugins/{}/{}", seg(plugin_id), verb.path());
        self.ok_answer(Method::POST, &path, None).await.map(|_| ())
    }

    /// `GET /plugins/{id}/config`, whole: a stopped plugin answers 404
    /// or 503, which the Settings tab says in words.
    pub async fn get_plugin_config(&self, plugin_id: &str) -> Result<Answer> {
        let path = format!("/plugins/{}/config", seg(plugin_id));
        self.answer(Method::GET, &path, None).await
    }

    /// The plugin's own config descriptor, or `None` when it ships none.
    pub async fn get_plugin_config_descriptor(&self, plugin_id: &str) -> Result<Option<Value>> {
        let path = format!("/plugins/{}/config/descriptor", seg(plugin_id));
        let a = self.answer(Method::GET, &path, None).await?;
        match a.status {
            404 => Ok(None),
            _ if a.ok() => Ok(a.body.get("descriptor").cloned().filter(|d| !d.is_null())),
            _ => Err(anyhow!("{}", a.error())),
        }
    }

    /// The plugin's config JSON Schema, or `None` when it publishes none.
    pub async fn get_plugin_config_schema(&self, plugin_id: &str) -> Result<Option<Value>> {
        let path = format!("/plugins/{}/config/schema", seg(plugin_id));
        let a = self.answer(Method::GET, &path, None).await?;
        match a.status {
            404 => Ok(None),
            _ if a.ok() => Ok(a.body.get("schema").cloned().filter(|d| !d.is_null())),
            _ => Err(anyhow!("{}", a.error())),
        }
    }

    /// `PUT /plugins/{id}/config` with a `patch` (or, for a remote
    /// plugin, `config`) body.
    pub async fn put_plugin_config(&self, plugin_id: &str, body: Value) -> Result<()> {
        let path = format!("/plugins/{}/config", seg(plugin_id));
        self.ok_answer(Method::PUT, &path, Some(body))
            .await
            .map(|_| ())
    }

    /// `GET /registry/plugins`.
    pub async fn list_registry(&self) -> Result<Vec<RegistryPlugin>> {
        let v = self
            .ok_answer(Method::GET, "/registry/plugins", None)
            .await?;
        Ok(serde_json::from_value::<RegistryIndex>(v)
            .map_err(|e| anyhow!("/registry/plugins: unexpected response ({e})"))?
            .plugins)
    }

    /// `POST /plugins/install` `{id, version?, runtime_id?}`, the answer
    /// kept whole: a 409 lists the runtimes to choose from.
    pub async fn install_plugin(
        &self,
        id: &str,
        version: Option<&str>,
        runtime_id: Option<&str>,
    ) -> Result<Answer> {
        let mut body = json!({ "id": id });
        if let Some(v) = version {
            body["version"] = json!(v);
        }
        if let Some(r) = runtime_id {
            body["runtime_id"] = json!(r);
        }
        self.answer(Method::POST, "/plugins/install", Some(body))
            .await
    }

    /// `GET /plugin-runtimes`.
    pub async fn list_runtimes(&self) -> Result<Vec<Runtime>> {
        let v = self
            .ok_answer(Method::GET, "/plugin-runtimes", None)
            .await?;
        Ok(serde_json::from_value::<RuntimeList>(v)
            .map_err(|e| anyhow!("/plugin-runtimes: unexpected response ({e})"))?
            .runtimes)
    }

    /// `GET /plugin-runtimes/placements`. A core without runtimes
    /// answers 404: none.
    pub async fn list_placements(&self) -> Result<Vec<Placement>> {
        let a = self
            .answer(Method::GET, "/plugin-runtimes/placements", None)
            .await?;
        match a.status {
            404 => Ok(Vec::new()),
            _ if a.ok() => Ok(serde_json::from_value::<PlacementList>(a.body)
                .map(|p| p.placements)
                .unwrap_or_default()),
            _ => Err(anyhow!("{}", a.error())),
        }
    }

    /// `POST /plugin-runtimes/{id}/reset`: a new key, the old one stops
    /// working at once.
    /// `GET /plugin-runtimes/interpreters`: each runtime's interpreters and
    /// any update waiting. Empty when homeCore has no registry to ask.
    pub async fn list_interpreters(&self) -> Result<Vec<crate::plugin_admin::Interpreter>> {
        let resp = self
            .request(Method::GET, "/plugin-runtimes/interpreters", None)
            .await?;
        if resp.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
            return Ok(Vec::new());
        }
        let v: Value = super::http::parse_json("/plugin-runtimes/interpreters", resp).await?;
        Ok(
            serde_json::from_value(v.get("interpreters").cloned().unwrap_or_default())
                .unwrap_or_default(),
        )
    }

    /// `POST /plugin-runtimes/{id}/interpreters/update`: move one series to its
    /// newest patch, restarting the plugins on it.
    pub async fn update_interpreter(
        &self,
        runtime_id: &str,
        kind: &str,
        series: &str,
    ) -> Result<Value> {
        let path = format!("/plugin-runtimes/{}/interpreters/update", seg(runtime_id));
        self.ok_answer(
            Method::POST,
            &path,
            Some(serde_json::json!({ "kind": kind, "series": series })),
        )
        .await
    }

    /// `POST /plugin-runtimes`: a new runtime and its key, shown once.
    pub async fn add_runtime(&self, name: &str, homecore_url: &str) -> Result<IssuedKey> {
        let body =
            serde_json::json!({ "name": name, "kind": "python", "homecore_url": homecore_url });
        let v = self
            .ok_answer(Method::POST, "/plugin-runtimes", Some(body))
            .await?;
        serde_json::from_value(v)
            .map_err(|e| anyhow!("/plugin-runtimes: unexpected response ({e})"))
    }

    /// `DELETE /plugin-runtimes/{id}`: uninstalls what is placed on it, then
    /// removes it, which ends its key.
    pub async fn remove_runtime(&self, runtime_id: &str) -> Result<()> {
        let path = format!("/plugin-runtimes/{}", seg(runtime_id));
        self.ok_answer(Method::DELETE, &path, None)
            .await
            .map(|_| ())
    }

    pub async fn reset_runtime(&self, runtime_id: &str) -> Result<IssuedKey> {
        let path = format!("/plugin-runtimes/{}/reset", seg(runtime_id));
        let v = self.ok_answer(Method::POST, &path, None).await?;
        serde_json::from_value(v).map_err(|e| anyhow!("{path}: unexpected response ({e})"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_name_core_s_words() {
        let a = Answer {
            status: 409,
            body: json!({"error": "choose one", "runtimes": ["a"]}),
        };
        assert!(!a.ok());
        assert_eq!(a.error(), "choose one");
        let b = Answer {
            status: 500,
            body: Value::Null,
        };
        assert_eq!(b.error(), "homeCore said no (500)");
        assert_eq!(Lifecycle::Restart.path(), "restart");
    }
}
