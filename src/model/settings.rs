//! Shapes for the Settings screens: roles, API keys, sessions, kept
//! backups, restore plans, calendars, health, versions, the log level and
//! open connections.
//!
//! As elsewhere every optional field defaults, so an older or newer core
//! still parses.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroize;

// ── People ──────────────────────────────────────────────────────────────────

/// One row of `GET /auth/roles`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RoleScopes {
    pub role: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// One row of `GET /auth/api-keys`. No secret: only its display prefix.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApiKey {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub owner_uid: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub last_used_at: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub revoked_at: Option<String>,
    /// A display's key, managed under Displays.
    #[serde(default)]
    pub display: bool,
    /// The plugin core made this key for.
    #[serde(default)]
    pub plugin: Option<String>,
}

impl ApiKey {
    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some()
    }

    /// Whether it has stopped working by `now` (RFC 3339 comparison).
    pub fn is_expired(&self, now: &chrono::DateTime<chrono::Utc>) -> bool {
        self.expires_at
            .as_deref()
            .and_then(|e| chrono::DateTime::parse_from_rfc3339(e).ok())
            .is_some_and(|e| e < *now)
    }
}

/// What `POST /auth/api-keys` and `POST /auth/api-keys/{id}/rotate`
/// return. The `token` is the only time core sends the secret. It is
/// never serialized, never printed by `Debug`, and zeroed on drop.
#[derive(Default, Deserialize)]
pub struct NewApiKey {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub token: String,
}

impl std::fmt::Debug for NewApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewApiKey")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl Drop for NewApiKey {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

/// One of my sessions, from `GET /auth/sessions`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    #[serde(default)]
    pub current: bool,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub last_active: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

/// "Firefox on Linux", "hc-tui", "curl" from a user agent; the raw words
/// when it can't tell.
pub fn client_of(ua: Option<&str>) -> String {
    let Some(ua) = ua.map(str::trim).filter(|u| !u.is_empty()) else {
        return "something unnamed".to_string();
    };
    if let Some(first) = ua.split_whitespace().next()
        && !first.starts_with("Mozilla/")
    {
        // `hc-tui/0.1.3`, `curl/8.21.0`, `python-requests/2.3`.
        return first.split('/').next().unwrap_or(first).to_string();
    }
    let browser = if ua.contains("Edg/") {
        "Edge"
    } else if ua.contains("Firefox/") {
        "Firefox"
    } else if ua.contains("HeadlessChrome/") {
        "Headless Chrome"
    } else if ua.contains("Chrome/") {
        "Chrome"
    } else if ua.contains("Safari/") {
        "Safari"
    } else {
        "a browser"
    };
    let os = if ua.contains("iPhone") || ua.contains("iPad") {
        "iOS"
    } else if ua.contains("Android") {
        "Android"
    } else if ua.contains("Mac OS X") {
        "macOS"
    } else if ua.contains("Windows") {
        "Windows"
    } else if ua.contains("Linux") {
        "Linux"
    } else {
        ""
    };
    if os.is_empty() {
        browser.to_string()
    } else {
        format!("{browser} on {os}")
    }
}

// ── Data and backups ────────────────────────────────────────────────────────

/// `GET /system/backups`: the schedule and the backups kept on the house.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct KeptBackups {
    #[serde(default)]
    pub every_hours: u32,
    #[serde(default)]
    pub keep: u32,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default)]
    pub backups: Vec<KeptBackup>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct KeptBackup {
    pub name: String,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub created_at: String,
}

impl KeptBackup {
    /// The day it was taken, `YYYY-MM-DD`: what restoring asks you to
    /// type. Read from the name (`homecore-backup-20261117T210247Z.zip`),
    /// else from `created_at`.
    pub fn date(&self) -> String {
        backup_date(&self.name).unwrap_or_else(|| crate::text::take_chars(&self.created_at, 10))
    }
}

/// `YYYY-MM-DD` from a backup's file name, when it carries one.
pub fn backup_date(name: &str) -> Option<String> {
    let digits: String = name
        .trim_start_matches("homecore-backup-")
        .chars()
        .take(8)
        .collect();
    if digits.len() != 8 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!(
        "{}-{}-{}",
        &digits[0..4],
        &digits[4..6],
        &digits[6..8]
    ))
}

/// What a backup would put back, from a restore check or stage.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RestorePlan {
    #[serde(default)]
    pub restored: Vec<String>,
    #[serde(default)]
    pub skipped: Vec<String>,
    #[serde(default)]
    pub taken_at: Option<String>,
}

impl RestorePlan {
    /// What it brings back, in words, as the app says it.
    pub fn summary(&self) -> Vec<String> {
        let n = |pre: &str| self.restored.iter().filter(|x| x.starts_with(pre)).count();
        let mut out = vec!["The house's state: devices, rooms, scenes, people".to_string()];
        if self.restored.iter().any(|x| x == "history.db") {
            out.push("Its history".to_string());
        }
        if n("rules/") > 0 {
            out.push(format!("{} rules", n("rules/")));
        }
        if n("config/") > 0 {
            out.push("homeCore's settings and modes".to_string());
        }
        if n("plugins/") > 0 {
            out.push(format!("Settings for {} plugins", n("plugins/")));
        }
        if n("calendars/") > 0 {
            out.push("Its calendars".to_string());
        }
        out
    }
}

/// One of `GET /calendars`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Calendar {
    pub id: String,
    #[serde(default)]
    pub event_count: usize,
    #[serde(default)]
    pub upcoming_count: usize,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub refresh_hours: Option<u32>,
}

/// A calendar's name as core keeps it: letters, digits, `-` and `_`.
pub fn calendar_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ── System ──────────────────────────────────────────────────────────────────

/// `GET /health`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Health {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub version: String,
}

impl Health {
    pub fn ok(&self) -> bool {
        self.status.eq_ignore_ascii_case("ok")
    }
}

/// `GET /system/versions` as `(part, version)` pairs, core first. Core
/// sends `{"core": "0.1.116"}`; an appliance sends its own file, which
/// may nest parts one level (`{"plugins": {"hc-hue": "0.2.7"}}`) or put
/// a version under `version`.
pub fn parse_versions(v: &Value) -> Vec<(String, String)> {
    fn text(v: &Value) -> Option<String> {
        match v {
            Value::String(s) => Some(s.trim_start_matches('v').to_string()),
            Value::Number(n) => Some(n.to_string()),
            Value::Object(m) => m.get("version").and_then(text),
            _ => None,
        }
    }
    let mut out = Vec::new();
    let Some(map) = v.as_object() else {
        return out;
    };
    for (k, v) in map {
        if let Some(t) = text(v) {
            out.push((k.clone(), t));
        } else if let Some(inner) = v.as_object() {
            for (k2, v2) in inner {
                if let Some(t) = text(v2) {
                    out.push((k2.clone(), t));
                }
            }
        }
    }
    out.sort_by_key(|(k, _)| (k != "core", k.clone()));
    out
}

/// The levels Left and Right step through, quietest first.
pub const LOG_LEVELS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

/// What the app sends with a level so the broker stays quiet.
pub const BROKER_QUIET: [&str; 2] = ["rumqttc::state=info", "rumqttd=info"];

/// What `GET /system/log-level`'s directive amounts to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLevelReading {
    /// The level everything not named gets: the last bare word.
    pub level: Option<String>,
    /// Parts set apart, other than the broker's.
    pub others: Vec<String>,
}

pub fn read_log_level(directive: &str) -> LogLevelReading {
    let mut level = None;
    let mut others = Vec::new();
    for p in directive
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        if LOG_LEVELS.contains(&p.to_ascii_lowercase().as_str()) {
            level = Some(p.to_ascii_lowercase());
        } else if !BROKER_QUIET.contains(&p) {
            others.push(p.to_string());
        }
    }
    LogLevelReading { level, others }
}

/// The directive for `level`, keeping any parts set apart and the
/// broker quiet.
pub fn log_directive(level: &str, others: &[String]) -> String {
    let mut parts = vec![level.to_string()];
    parts.extend(others.iter().cloned());
    parts.extend(BROKER_QUIET.iter().map(|s| s.to_string()));
    parts.join(",")
}

/// The level `delta` steps from `current` (clamped).
pub fn step_log_level(current: Option<&str>, delta: isize) -> &'static str {
    let i = current
        .and_then(|c| LOG_LEVELS.iter().position(|l| *l == c))
        .unwrap_or(2) as isize;
    let n = LOG_LEVELS.len() as isize;
    LOG_LEVELS[(i + delta).clamp(0, n - 1) as usize]
}

/// One of `GET /ws/connections`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WsConnection {
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub connected_at: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WsConnections {
    #[serde(default)]
    pub connections: Vec<WsConnection>,
}

/// Connections grouped by who, what they watch and with what: a
/// person's browser tabs count once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionGroup {
    pub user: String,
    /// "the house" or "the log".
    pub watching: &'static str,
    pub client: String,
    pub count: usize,
    pub since: String,
}

pub fn group_connections(list: &[WsConnection]) -> Vec<ConnectionGroup> {
    let mut out: Vec<ConnectionGroup> = Vec::new();
    for c in list {
        let user = c.user.trim_start_matches("api_key:").to_string();
        let watching = if c.endpoint == "logs_stream" {
            "the log"
        } else {
            "the house"
        };
        let client = client_of(c.user_agent.as_deref());
        match out
            .iter_mut()
            .find(|g| g.user == user && g.watching == watching && g.client == client)
        {
            Some(g) => {
                g.count += 1;
                if c.connected_at < g.since {
                    g.since = c.connected_at.clone();
                }
            }
            None => out.push(ConnectionGroup {
                user,
                watching,
                client,
                count: 1,
                since: c.connected_at.clone(),
            }),
        }
    }
    out
}

/// `GET /system/temperature-unit`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HouseTemperature {
    #[serde(default)]
    pub temperature_unit: String,
    /// False while nobody has chosen one for the household.
    #[serde(default)]
    pub chosen: bool,
}

/// `12.3 MB` style sizes.
pub fn size(bytes: u64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b < K {
        format!("{bytes} B")
    } else if b < K * K {
        format!("{:.0} KB", b / K)
    } else if b < K * K * K {
        format!("{:.1} MB", b / K / K)
    } else {
        format!("{:.1} GB", b / K / K / K)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::http::parse_body;
    use serde_json::json;

    macro_rules! fixture {
        ($name:literal) => {
            include_str!(concat!("../../tests/fixtures/", $name))
        };
    }

    #[test]
    fn kept_backups_parse_newest_first_with_their_dates() {
        let k: KeptBackups =
            parse_body("/system/backups", fixture!("system_backups.json")).unwrap();
        assert_eq!((k.every_hours, k.keep), (24, 7));
        assert_eq!(k.backups.len(), 3);
        assert_eq!(k.backups[0].name, "homecore-backup-20261117T210247Z.zip");
        assert_eq!(k.backups[0].date(), "2026-11-17");
        assert_eq!(k.backups[2].date(), "2026-11-15");
        assert_eq!(size(k.backups[0].bytes), "18.4 MB");
        // A house that keeps none says so with an empty list.
        let none: KeptBackups = parse_body(
            "/system/backups",
            r#"{"every_hours":0,"keep":0,"dir":null,"backups":[]}"#,
        )
        .unwrap();
        assert!(none.backups.is_empty() && none.dir.is_none());
    }

    #[test]
    fn a_backup_without_a_dated_name_falls_back_to_its_time() {
        let b = KeptBackup {
            name: "homecore-backup-x.zip".into(),
            bytes: 1,
            created_at: "2026-10-01T03:00:00Z".into(),
        };
        assert_eq!(b.date(), "2026-10-01");
        assert_eq!(
            backup_date("homecore-backup-20250102T000000Z.zip").as_deref(),
            Some("2025-01-02")
        );
        assert_eq!(backup_date("other.zip"), None);
    }

    #[test]
    fn a_restore_plan_says_what_it_brings() {
        let p: RestorePlan = parse_body(
            "/restore",
            &json!({
                "restored": ["state.redb", "history.db", "rules/a.ron", "rules/b.ron",
                             "config/modes.toml", "plugins/hc-hue/config.toml"],
                "skipped": [], "taken_at": "2026-11-17T21:02:47Z"
            })
            .to_string(),
        )
        .unwrap();
        let s = p.summary();
        assert!(s.contains(&"Its history".to_string()));
        assert!(s.contains(&"2 rules".to_string()));
        assert!(s.contains(&"Settings for 1 plugins".to_string()));
    }

    #[test]
    fn health_and_versions_parse() {
        let h: Health = parse_body("/health", r#"{"status":"ok","version":"0.1.116"}"#).unwrap();
        assert!(h.ok());
        assert_eq!(h.version, "0.1.116");
        let v = parse_versions(&json!({"core": "0.1.116"}));
        assert_eq!(v, vec![("core".to_string(), "0.1.116".to_string())]);
        let v = parse_versions(
            &serde_json::from_str(fixture!("system_versions_appliance.json")).unwrap(),
        );
        assert_eq!(v[0], ("core".to_string(), "0.1.116".to_string()));
        assert!(
            v.contains(&("hc-hue".to_string(), "0.2.7".to_string())),
            "{v:?}"
        );
        assert!(
            v.contains(&("app".to_string(), "0.1.82".to_string())),
            "{v:?}"
        );
        assert!(
            v.contains(&("image".to_string(), "2026.11.1".to_string())),
            "{v:?}"
        );
        assert!(!v.iter().any(|(k, _)| k == "built_at"), "{v:?}");
        assert!(parse_versions(&json!([1, 2])).is_empty());
    }

    #[test]
    fn the_log_level_reads_and_steps_keeping_the_broker_quiet() {
        let r = read_log_level("info,rumqttc::state=info,rumqttd=info");
        assert_eq!(r.level.as_deref(), Some("info"));
        assert!(r.others.is_empty());
        assert_eq!(step_log_level(r.level.as_deref(), 1), "debug");
        assert_eq!(step_log_level(Some("error"), -1), "error");
        assert_eq!(step_log_level(Some("trace"), 1), "trace");
        assert_eq!(
            log_directive("warn", &r.others),
            "warn,rumqttc::state=info,rumqttd=info"
        );
        let r = read_log_level("debug,hc_core=trace");
        assert_eq!(r.others, vec!["hc_core=trace".to_string()]);
        assert_eq!(
            log_directive("info", &r.others),
            "info,hc_core=trace,rumqttc::state=info,rumqttd=info"
        );
    }

    #[test]
    fn connections_group_by_person_and_client() {
        let c: WsConnections =
            parse_body("/ws/connections", fixture!("ws_connections.json")).unwrap();
        let g = group_connections(&c.connections);
        assert_eq!(g.len(), 3, "{g:?}");
        assert_eq!(g[0].user, "admin");
        assert_eq!(g[0].client, "Firefox on Linux");
        assert_eq!(g[0].count, 2);
        assert_eq!(g[0].since, "2026-11-17T18:00:00Z");
        assert!(
            g.iter()
                .any(|x| x.client == "hc-tui" && x.watching == "the log")
        );
        assert!(g.iter().any(|x| x.user == "Kitchen panel"));
    }

    #[test]
    fn api_keys_and_sessions_parse() {
        let keys: Vec<ApiKey> = parse_body("/auth/api-keys", fixture!("api_keys.json")).unwrap();
        assert_eq!(keys.len(), 3);
        assert!(keys[0].display && !keys[0].is_revoked());
        assert!(keys[1].is_revoked());
        assert_eq!(keys[2].plugin.as_deref(), Some("hc-mcp"));
        let s: Vec<SessionInfo> = parse_body("/auth/sessions", fixture!("sessions.json")).unwrap();
        assert_eq!(s.iter().filter(|x| x.current).count(), 1);
        assert_eq!(client_of(s[0].user_agent.as_deref()), "hc-tui");
        assert_eq!(client_of(Some("curl/8.21.0")), "curl");
        assert_eq!(client_of(None), "something unnamed");
        let roles: Vec<RoleScopes> = parse_body(
            "/auth/roles",
            r#"[{"role":"admin","scopes":["devices:read","users:write"]},{"role":"observer"}]"#,
        )
        .unwrap();
        assert_eq!(roles[0].scopes.len(), 2);
        assert!(roles[1].scopes.is_empty());
    }

    #[test]
    fn a_new_key_never_shows_its_secret_in_debug() {
        let k: NewApiKey = serde_json::from_value(json!({
            "id": "1", "label": "script", "token": "hc_sk_supersecretvalue",
            "owner_uid": "u", "scopes": [], "created_at": "2026-11-17T00:00:00Z"
        }))
        .unwrap();
        assert_eq!(k.token, "hc_sk_supersecretvalue");
        assert!(!format!("{k:?}").contains("supersecret"));
    }

    #[test]
    fn calendar_names_are_tidied() {
        assert_eq!(calendar_name("Bins & recycling"), "Bins___recycling");
        let c: Vec<Calendar> = parse_body(
            "/calendars",
            r#"[{"event_count":58,"fetched_at":null,"id":"bins","loaded_at":"2026-11-17T18:20:24Z","refresh_hours":null,"source_url":null,"upcoming_count":57}]"#,
        )
        .unwrap();
        assert_eq!((c[0].id.as_str(), c[0].upcoming_count), ("bins", 57));
    }
}
