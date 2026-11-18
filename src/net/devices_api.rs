//! REST calls for device controls, history, removed devices, room
//! membership, scenes and modes. All paths are under `/api/v1`.

use super::http::{HomeCoreClient, check_status, parse_json, seg};
use crate::model::{Area, ModeConfig, Scene};
use crate::model_devices::{DeviceSchema, HistoryPoint, RemovedDevice};
use anyhow::{Result, anyhow};
use reqwest::{Method, StatusCode};
use serde_json::{Map, Value, json};

/// The body of `POST /scenes` and `PUT /scenes/{id}`.
pub fn scene_body(name: &str, states: &Map<String, Value>) -> Value {
    json!({ "name": name.trim(), "states": states })
}

/// Device ids of a room after adding `add` and taking out `remove`,
/// sorted and without repeats.
pub fn room_members(current: &[String], add: &[String], remove: &[String]) -> Vec<String> {
    let mut out: Vec<String> = current
        .iter()
        .chain(add.iter())
        .filter(|id| !remove.contains(id))
        .cloned()
        .collect();
    out.sort();
    out.dedup();
    out
}

impl HomeCoreClient {
    // ── Devices ──────────────────────────────────────────────────────────

    /// `GET /devices/{id}/schema`. `None` when the device has none.
    pub async fn device_schema(&self, id: &str) -> Result<Option<DeviceSchema>> {
        let path = format!("/devices/{}/schema", seg(id));
        let resp = self.request(Method::GET, &path, None).await?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        parse_json(&path, resp).await.map(Some)
    }

    /// `GET /devices/{id}/history` for one attribute since `from`
    /// (RFC 3339), thinned by core to about `points` values.
    pub async fn device_history(
        &self,
        id: &str,
        attribute: &str,
        from: &str,
        points: usize,
    ) -> Result<Vec<HistoryPoint>> {
        let path = format!(
            "/devices/{}/history?attribute={}&from={}&limit=1000&max_points={points}",
            seg(id),
            urlencoding::encode(attribute),
            urlencoding::encode(from),
        );
        self.get_json(&path).await
    }

    /// `GET /devices/removed`, newest first.
    pub async fn list_removed_devices(&self) -> Result<Vec<RemovedDevice>> {
        self.get_json("/devices/removed").await
    }

    /// `DELETE /devices/removed/{id}`: let the device come back the next
    /// time its plugin reports it.
    pub async fn restore_device(&self, id: &str) -> Result<()> {
        let path = format!("/devices/removed/{}", seg(id));
        self.send_empty(Method::DELETE, &path, None).await
    }

    /// `DELETE /devices/{id}`. Returns the names of the automations that
    /// used it; core turns those off.
    pub async fn remove_device(&self, id: &str) -> Result<Vec<String>> {
        let path = format!("/devices/{}", seg(id));
        let body: Value = self.send_json(Method::DELETE, &path, None).await?;
        Ok(body
            .get("affected_rules")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default())
    }

    /// How many automations use the device: `X-Total-Count` of
    /// `GET /automations?device_id=..&limit=0`, which sends no rows.
    pub async fn automations_using(&self, id: &str) -> Result<Option<u64>> {
        let path = format!("/automations?device_id={}&limit=0", seg(id));
        let resp = self.request(Method::GET, &path, None).await?;
        let resp = check_status(resp).await?;
        Ok(resp
            .headers()
            .get("x-total-count")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok()))
    }

    // ── Rooms ────────────────────────────────────────────────────────────

    /// Add and take devices out of a room with `PUT /areas/{id}/devices`,
    /// which replaces the room's whole list. The list is read fresh
    /// first so a device another person just moved in stays.
    pub async fn change_room_devices(
        &self,
        area_id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<Area> {
        let areas: Vec<Area> = self.get_json("/areas").await?;
        let area = areas
            .into_iter()
            .find(|a| a.id == area_id)
            .ok_or_else(|| anyhow!("that room is gone"))?;
        let ids = room_members(&area.device_ids, add, remove);
        let path = format!("/areas/{}/devices", seg(area_id));
        self.send_json(Method::PUT, &path, Some(json!(ids))).await
    }

    // ── Scenes ───────────────────────────────────────────────────────────

    pub async fn create_scene(&self, name: &str, states: &Map<String, Value>) -> Result<Scene> {
        self.send_json(Method::POST, "/scenes", Some(scene_body(name, states)))
            .await
    }

    /// `PUT /scenes/{id}`: the whole scene, name and states.
    pub async fn update_scene(
        &self,
        id: &str,
        name: &str,
        states: &Map<String, Value>,
    ) -> Result<Scene> {
        let path = format!("/scenes/{}", seg(id));
        self.send_json(Method::PUT, &path, Some(scene_body(name, states)))
            .await
    }

    pub async fn delete_scene(&self, id: &str) -> Result<()> {
        let path = format!("/scenes/{}", seg(id));
        self.send_empty(Method::DELETE, &path, None).await
    }

    // ── Modes ────────────────────────────────────────────────────────────

    /// `PATCH /modes/{id}`: name, and for a solar mode its events and
    /// offsets. Only the fields given change.
    pub async fn update_mode(&self, id: &str, fields: Map<String, Value>) -> Result<ModeConfig> {
        let path = format!("/modes/{}", seg(id));
        self.send_json(Method::PATCH, &path, Some(Value::Object(fields)))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scene_body_is_name_and_states() {
        let mut states = Map::new();
        states.insert("lamp".into(), json!({"on": true, "brightness_pct": 40}));
        let b = scene_body("  Evening ", &states);
        assert_eq!(
            b,
            json!({"name": "Evening", "states": {"lamp": {"on": true, "brightness_pct": 40}}})
        );
    }

    #[test]
    fn room_members_add_and_take_out() {
        let now = vec!["b".to_string(), "a".to_string()];
        let ids = room_members(&now, &["c".into(), "a".into()], &["b".into()]);
        assert_eq!(ids, vec!["a", "c"]);
    }
}
