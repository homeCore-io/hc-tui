//! Media players: a generic model built from a device's attributes and
//! `supported_actions`, plus the transport commands.

use super::App;
use crate::model::DeviceState;
use serde_json::{Value, json};

#[derive(Debug, Clone, Default)]
pub struct MediaPlayerCapabilities {
    pub can_play: bool,
    pub can_pause: bool,
    pub can_stop: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_set_volume: bool,
    pub can_mute: bool,
}

#[derive(Debug, Clone)]
pub struct MediaPlayerModel {
    pub device_id: String,
    pub display_name: String,
    pub canonical_name: Option<String>,
    pub plugin_id: String,
    pub playback_state: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub source: Option<String>,
    pub volume: Option<u8>,
    pub muted: Option<bool>,
    pub position_secs: Option<u64>,
    pub duration_secs: Option<u64>,
    pub capabilities: MediaPlayerCapabilities,
}

pub fn is_media_player(device: &DeviceState) -> bool {
    device.device_type() == "media_player"
        || device.attributes.get("kind").and_then(Value::as_str) == Some("media_player")
}

fn supported_actions(device: &DeviceState) -> Vec<String> {
    device
        .attributes
        .get("supported_actions")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_ascii_lowercase)
                .collect()
        })
        .unwrap_or_default()
}

fn string_attr(device: &DeviceState, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| device.attributes.get(*k).and_then(Value::as_str))
        .map(str::to_string)
}

fn u64_attr(device: &DeviceState, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|k| {
        let v = device.attributes.get(*k)?;
        v.as_u64()
            .or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
    })
}

fn playback_state(device: &DeviceState) -> String {
    device
        .attributes
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_ascii_lowercase()
}

pub fn media_player_model(device: &DeviceState) -> Option<MediaPlayerModel> {
    if !is_media_player(device) {
        return None;
    }
    let actions = supported_actions(device);
    let supports = |name: &str| actions.iter().any(|a| a == name);
    let unknown = actions.is_empty();
    Some(MediaPlayerModel {
        device_id: device.device_id.clone(),
        display_name: device.display_name().to_string(),
        canonical_name: device.canonical_name.clone(),
        plugin_id: device.plugin_id.clone(),
        playback_state: playback_state(device),
        title: string_attr(device, &["title", "track_title", "media_title"]),
        artist: string_attr(device, &["artist", "track_artist", "media_artist"]),
        album: string_attr(device, &["album", "track_album", "media_album"]),
        source: string_attr(device, &["source", "input_source", "media_source"]),
        volume: device
            .attributes
            .get("volume")
            .and_then(Value::as_f64)
            .map(|v| v.clamp(0.0, 100.0) as u8),
        muted: device.attributes.get("muted").and_then(Value::as_bool),
        position_secs: u64_attr(device, &["position_secs", "position"]),
        duration_secs: u64_attr(device, &["duration_secs", "duration"]),
        capabilities: MediaPlayerCapabilities {
            can_play: unknown || supports("play"),
            can_pause: unknown || supports("pause"),
            can_stop: supports("stop"),
            can_next: supports("next"),
            can_previous: supports("previous"),
            can_set_volume: supports("set_volume")
                || supports("volume")
                || device.attributes.contains_key("volume"),
            can_mute: supports("set_mute")
                || supports("mute")
                || device.attributes.contains_key("muted"),
        },
    })
}

/// What Space / `t` sends: stop while playing, else play.
pub fn toggle_action(device: &DeviceState) -> Option<&'static str> {
    if !is_media_player(device) {
        return None;
    }
    Some(match playback_state(device).as_str() {
        "playing" | "buffering" => "stop",
        _ => "play",
    })
}

impl App {
    fn selected_media(&self) -> Option<(String, String, MediaPlayerModel)> {
        let d = self.selected_device()?;
        let m = media_player_model(d)?;
        Some((d.device_id.clone(), d.display_name().to_string(), m))
    }

    fn media_action(&mut self, id: String, body: Value, done: String) {
        self.command_device(id, body, done);
    }

    /// Stop, falling back to pause when the player rejects stop.
    fn media_stop_with_fallback(&mut self, id: String, name: String, can_pause: bool) {
        let c = self.client.clone();
        self.spawn_ok(
            "stop",
            async move {
                match c.send_device_action(&id, "stop").await {
                    Ok(()) => Ok("Stop"),
                    Err(stop_err) if can_pause => match c.send_device_action(&id, "pause").await {
                        Ok(()) => Ok("Pause"),
                        Err(pause_err) => Err(anyhow::anyhow!(
                            "stop failed: {stop_err}; pause fallback failed: {pause_err}"
                        )),
                    },
                    Err(e) => Err(e),
                }
            },
            move |app, what| app.status = format!("{name} → {what}"),
        );
    }

    pub(crate) fn media_play_stop(&mut self) {
        let Some(device) = self.selected_device() else {
            return;
        };
        let Some(action) = toggle_action(device) else {
            return;
        };
        let (id, name) = (device.device_id.clone(), device.display_name().to_string());
        if action == "stop" {
            self.media_stop_with_fallback(id, name, true);
        } else {
            self.media_action(id, json!({"action": "play"}), format!("{name} → Play"));
        }
    }

    pub(crate) fn media_play_pause(&mut self) {
        let Some((id, name, m)) = self.selected_media() else {
            return;
        };
        let playing = matches!(m.playback_state.as_str(), "playing" | "buffering");
        if playing && m.capabilities.can_pause {
            self.media_action(id, json!({"action": "pause"}), format!("{name} → Pause"));
        } else if m.capabilities.can_play {
            self.media_action(id, json!({"action": "play"}), format!("{name} → Play"));
        }
    }

    pub(crate) fn media_stop(&mut self) {
        let Some((id, name, m)) = self.selected_media() else {
            return;
        };
        if m.capabilities.can_stop {
            self.media_stop_with_fallback(id, name, m.capabilities.can_pause);
        } else if m.capabilities.can_pause {
            self.media_action(id, json!({"action": "pause"}), format!("{name} → Pause"));
        }
    }

    pub(crate) fn media_simple(&mut self, action: &'static str, label: &'static str) {
        let Some((id, name, m)) = self.selected_media() else {
            return;
        };
        let allowed = match action {
            "next" => m.capabilities.can_next,
            "previous" => m.capabilities.can_previous,
            _ => true,
        };
        if allowed {
            self.media_action(id, json!({"action": action}), format!("{name} → {label}"));
        }
    }

    pub(crate) fn media_toggle_mute(&mut self) {
        let Some((id, name, m)) = self.selected_media() else {
            return;
        };
        if !m.capabilities.can_mute {
            return;
        }
        let muted = !m.muted.unwrap_or(false);
        let label = if muted { "Muted" } else { "Unmuted" };
        self.media_action(
            id,
            json!({ "action": "set_mute", "muted": muted }),
            format!("{name} → {label}"),
        );
    }

    pub(crate) fn media_adjust_volume(&mut self, delta: i64) {
        let Some((id, name, m)) = self.selected_media() else {
            return;
        };
        if !m.capabilities.can_set_volume {
            return;
        }
        let next = (i64::from(m.volume.unwrap_or(0)) + delta).clamp(0, 100);
        self.media_action(
            id,
            json!({ "action": "set_volume", "volume": next }),
            format!("{name} volume → {next}%"),
        );
    }

    /// The selected player's model, for the detail pane.
    pub fn selected_media_player_model(&self) -> Option<MediaPlayerModel> {
        self.selected_device().and_then(media_player_model)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::device;
    use super::*;

    fn player(state: &str, extra: Value) -> DeviceState {
        let mut d = device("m", "Player");
        d.device_type = Some("media_player".into());
        let mut attrs = extra.as_object().cloned().unwrap_or_default();
        attrs.insert("state".into(), json!(state));
        d.attributes = attrs;
        d
    }

    #[test]
    fn toggle_prefers_stop_when_playing() {
        assert_eq!(toggle_action(&player("playing", json!({}))), Some("stop"));
        assert_eq!(toggle_action(&player("paused", json!({}))), Some("play"));
    }

    #[test]
    fn model_uses_supported_actions() {
        let d = player(
            "playing",
            json!({"supported_actions": ["play", "pause", "next", "set_volume"], "volume": 42}),
        );
        let m = media_player_model(&d).unwrap();
        assert!(m.capabilities.can_play && m.capabilities.can_pause && m.capabilities.can_next);
        assert!(m.capabilities.can_set_volume);
        assert!(!m.capabilities.can_previous && !m.capabilities.can_stop);
        assert_eq!(m.volume, Some(42));
    }
}
