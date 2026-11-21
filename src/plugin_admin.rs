//! Plugins as an admin manages them: the registry (the Catalog), plugin
//! runtimes, install results, and the words the Installed list uses.
//!
//! Shapes follow core's `GET /registry/plugins`, `GET /plugin-runtimes`,
//! `GET /plugin-runtimes/placements` and `POST /plugins/install`, and the
//! wording follows the app's Plugins pages. Everything here is pure, so
//! the list's columns can be tested without a server.

use crate::model::{PluginNotice, PluginRecord};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;

// ── Registry ────────────────────────────────────────────────────────────────

/// One plugin in the registry index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistryPlugin {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub versions: Vec<RegistryVersion>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistryVersion {
    pub version: String,
    #[serde(default)]
    pub min_core: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Artifact {
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub size: u64,
    /// `native` (or absent) for a binary core runs itself; else the
    /// runtime kind that hosts it (`python`).
    #[serde(default)]
    pub runtime: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RegistryIndex {
    #[serde(default)]
    pub plugins: Vec<RegistryPlugin>,
}

impl RegistryPlugin {
    /// The name to show: the registry's, else one made from the id.
    pub fn display_name(&self) -> String {
        if self.name.trim().is_empty() {
            name_from_id(&self.id)
        } else {
            self.name.clone()
        }
    }

    /// What kind of plugin it is, from its artifacts: `native`, or the
    /// runtime kinds that host it, joined.
    pub fn kind(&self, version: Option<&str>) -> String {
        let v = match version {
            Some(want) => self.versions.iter().find(|v| v.version == want),
            None => self.versions.last(),
        };
        let mut kinds: Vec<String> = v
            .map(|v| {
                v.artifacts
                    .iter()
                    .map(|a| match a.runtime.as_deref() {
                        None | Some("") => "native".to_string(),
                        Some(k) => k.to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        kinds.sort();
        kinds.dedup();
        if kinds.is_empty() {
            "-".into()
        } else {
            kinds.join(", ")
        }
    }

    /// Size of the Linux artifact of `version`, as "4.0 MB".
    pub fn size_of(&self, version: &str) -> Option<String> {
        let a = self
            .versions
            .iter()
            .find(|v| v.version == version)?
            .artifacts
            .iter()
            .find(|a| a.os == "linux" && a.size > 0)?;
        Some(format!("{:.1} MB", a.size as f64 / 1_048_576.0))
    }
}

/// Compare dotted versions by number: 0.1.10 is after 0.1.9.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let split = |s: &str| -> Vec<String> {
        s.trim_start_matches('v')
            .split(['.', '-'])
            .map(str::to_string)
            .collect()
    };
    let (pa, pb) = (split(a), split(b));
    for i in 0..pa.len().max(pb.len()) {
        let x = pa.get(i).map(String::as_str).unwrap_or("0");
        let y = pb.get(i).map(String::as_str).unwrap_or("0");
        let c = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(nx), Ok(ny)) => nx.cmp(&ny),
            _ => x.cmp(y),
        };
        if c != Ordering::Equal {
            return c;
        }
    }
    Ordering::Equal
}

/// The `min_core` a version needs that `core` is not, when `core` is
/// older. `None` when it runs, or when either is not known.
pub fn needs_core<'a>(v: &'a RegistryVersion, core: Option<&str>) -> Option<&'a str> {
    let min = v.min_core.as_deref().filter(|m| !m.is_empty())?;
    let core = core.filter(|c| !c.is_empty())?;
    (compare_versions(core, min) == Ordering::Less).then_some(min)
}

/// The newest version that runs on `core` (any, when `core` is unknown).
pub fn latest_of<'a>(r: &'a RegistryPlugin, core: Option<&str>) -> Option<&'a str> {
    r.versions
        .iter()
        .filter(|v| needs_core(v, core).is_none())
        .map(|v| v.version.as_str())
        .max_by(|a, b| compare_versions(a, b))
}

/// The version a plugin runs: what it says, else what was installed.
pub fn version_of(p: &PluginRecord) -> Option<&str> {
    p.version
        .as_deref()
        .or(p.installed_version.as_deref())
        .filter(|v| !v.is_empty())
}

/// A newer version in the registry that runs on `core`, if there is one.
pub fn update_of(
    p: &PluginRecord,
    registry: &[RegistryPlugin],
    core: Option<&str>,
) -> Option<String> {
    let r = registry.iter().find(|r| r.id == p.plugin_id)?;
    let have = version_of(p)?;
    let latest = latest_of(r, core)?;
    (compare_versions(latest, have) == Ordering::Greater).then(|| latest.to_string())
}

/// The Catalog: registry plugins that are not installed, by name.
pub fn catalog<'a>(
    registry: &'a [RegistryPlugin],
    installed: &[PluginRecord],
) -> Vec<&'a RegistryPlugin> {
    let mut out: Vec<&RegistryPlugin> = registry
        .iter()
        .filter(|r| !installed.iter().any(|p| p.plugin_id == r.id))
        .collect();
    out.sort_by_key(|r| r.display_name().to_lowercase());
    out
}

/// The name for an installed plugin: the registry's, else one made from
/// its id.
pub fn plugin_name(p: &PluginRecord, registry: &[RegistryPlugin]) -> String {
    registry
        .iter()
        .find(|r| r.id == p.plugin_id && !r.name.trim().is_empty())
        .map(|r| r.name.clone())
        .unwrap_or_else(|| name_from_id(&p.plugin_id))
}

/// "plugin.hue" to "Hue", as the rest of the TUI names plugins.
pub fn name_from_id(plugin_id: &str) -> String {
    let mut value = plugin_id;
    for prefix in ["plugin.", "plugin_", "hc-"] {
        if let Some(stripped) = value.strip_prefix(prefix) {
            value = stripped;
        }
    }
    crate::text::normalize_label(value.rsplit('.').next().unwrap_or(value))
}

// ── State in words ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Good,
    Quiet,
    Bad,
}

/// The state in the app's words: Running, Turned off, Not answering.
pub fn state_of(p: &PluginRecord) -> (&'static str, Tone) {
    if !p.enabled {
        // Turned off, but homeCore has not stopped it yet: say so.
        return match p.status.as_str() {
            "active" | "not_responding" => ("Turned off (running)", Tone::Quiet),
            "starting" => ("Turned off (starting)", Tone::Quiet),
            _ => ("Turned off", Tone::Quiet),
        };
    }
    match p.status.as_str() {
        "active" => ("Running", Tone::Good),
        "starting" => ("Starting", Tone::Quiet),
        "stopped" => ("Stopped", Tone::Quiet),
        "offline" => ("Not answering", Tone::Bad),
        "not_responding" => ("Not responding", Tone::Bad),
        "degraded" => ("Degraded", Tone::Bad),
        _ => ("Unknown", Tone::Quiet),
    }
}

/// Whether the process is up, for `s` (start or stop).
pub fn is_running(p: &PluginRecord) -> bool {
    matches!(
        p.status.as_str(),
        "active" | "starting" | "not_responding" | "degraded"
    )
}

/// Restarts at or above this many read as trouble in the list.
pub const RESTART_TROUBLE: u64 = 2;

/// The Notes column: an update, notices, restart trouble.
pub fn notes_of(p: &PluginRecord, update: Option<&str>) -> Vec<(String, Tone)> {
    let mut out = Vec::new();
    if let Some(u) = update {
        out.push((format!("update {u}"), Tone::Good));
    }
    let n = p.notices.len();
    if n > 0 {
        let worst = worst_notice(&p.notices).map(|x| x.level.as_str());
        let tone = if worst == Some("error") {
            Tone::Bad
        } else {
            Tone::Quiet
        };
        let word = if n == 1 { "notice" } else { "notices" };
        out.push((format!("{n} {word}"), tone));
    }
    if p.restart_count >= RESTART_TROUBLE {
        out.push((format!("restarted {} times", p.restart_count), Tone::Bad));
    }
    out
}

fn notice_rank(level: &str) -> u8 {
    match level {
        "error" | "critical" => 0,
        "warning" | "warn" => 1,
        _ => 2,
    }
}

/// Notices, the worst first.
pub fn sorted_notices(list: &[PluginNotice]) -> Vec<&PluginNotice> {
    let mut v: Vec<&PluginNotice> = list.iter().collect();
    v.sort_by_key(|n| notice_rank(&n.level));
    v
}

pub fn worst_notice(list: &[PluginNotice]) -> Option<&PluginNotice> {
    sorted_notices(list).into_iter().next()
}

// ── Install ─────────────────────────────────────────────────────────────────

/// What `POST /plugins/install` answered.
#[derive(Debug, Clone, PartialEq)]
pub enum InstallOutcome {
    /// Installed here and (re)started.
    Installed {
        version: String,
        activated: bool,
    },
    /// Sent to a runtime, which installs it on its next check-in.
    Placed {
        runtime_id: String,
        version: String,
    },
    /// More than one runtime can run it: ask which.
    ChooseRuntime(Vec<String>),
    Failed(String),
}

/// Read an install answer: its HTTP status and body.
pub fn install_outcome(status: u16, body: &Value) -> InstallOutcome {
    let s = |k: &str| {
        body.get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    if (200..300).contains(&status) {
        if body.get("placed_on").is_some() {
            return InstallOutcome::Placed {
                runtime_id: s("placed_on"),
                version: s("version"),
            };
        }
        return InstallOutcome::Installed {
            version: s("version"),
            activated: body
                .get("activated")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        };
    }
    if status == 409
        && let Some(ids) = body.get("runtimes").and_then(Value::as_array)
    {
        let ids: Vec<String> = ids
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        if !ids.is_empty() {
            return InstallOutcome::ChooseRuntime(ids);
        }
    }
    let msg = body
        .get("error")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("homeCore said no ({status})"));
    InstallOutcome::Failed(msg)
}

// ── Runtimes ────────────────────────────────────────────────────────────────

/// A plugin runtime as core lists it. It never carries its password.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Runtime {
    pub runtime_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub abi: String,
    #[serde(default)]
    pub plugin_id: String,
    #[serde(default)]
    pub homecore_url: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub host_version: Option<String>,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub network_mode: Option<String>,
    #[serde(default)]
    pub source_ip: Option<String>,
    #[serde(default)]
    pub page_port: Option<u16>,
    #[serde(default)]
    pub page_url: Option<String>,
    #[serde(default)]
    pub last_seen_at: Option<String>,
    #[serde(default)]
    pub max_plugins: Option<u32>,
    #[serde(default)]
    pub outdated: bool,
    #[serde(default)]
    pub min_host_version: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RuntimeList {
    #[serde(default)]
    pub runtimes: Vec<Runtime>,
}

/// Which plugin runs on which runtime.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Placement {
    pub runtime_id: String,
    pub plugin_id: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub placed_at: String,
    /// The interpreter it runs on, as core words it (`"python 3.14.10"`), for a
    /// plugin that needs one. Sent since core v0.1.119.
    #[serde(default)]
    pub interpreter: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PlacementList {
    #[serde(default)]
    pub placements: Vec<Placement>,
}

/// What a reset hands back once: the new connection key.
#[derive(Clone, Deserialize)]
pub struct IssuedKey {
    #[serde(default)]
    pub runtime: Runtime,
    pub key: String,
}

impl std::fmt::Debug for IssuedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IssuedKey({}, key: ***)", self.runtime.runtime_id)
    }
}

/// How a runtime is doing, in words, from when it was last seen.
pub fn runtime_status(r: &Runtime, now: chrono::DateTime<chrono::Utc>) -> (String, Tone) {
    if r.outdated {
        return ("Needs a newer image".into(), Tone::Bad);
    }
    let Some(seen) = r
        .last_seen_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
    else {
        return ("Waiting for it to connect".into(), Tone::Quiet);
    };
    let ago = now.signed_duration_since(seen.with_timezone(&chrono::Utc));
    // A runtime checks in every 30 seconds; two missed is worth saying.
    if ago.num_seconds() <= 90 {
        ("Connected".into(), Tone::Good)
    } else {
        (format!("Not heard from for {}", ago_words(ago)), Tone::Bad)
    }
}

fn ago_words(d: chrono::Duration) -> String {
    let s = d.num_seconds().max(0);
    if s < 3600 {
        format!("{} min", s / 60)
    } else if s < 86_400 {
        format!("{} h", s / 3600)
    } else {
        format!("{} days", s / 86_400)
    }
}

/// The plugins placed on `runtime_id`.
/// An interpreter series a runtime's plugins run on (`GET
/// /plugin-runtimes/interpreters`), and the newer patch waiting, if any.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Interpreter {
    pub runtime_id: String,
    #[serde(default)]
    pub kind: String,
    /// `cp312`: what an update keeps to.
    #[serde(default)]
    pub series: String,
    /// The patches in use now.
    #[serde(default)]
    pub versions: Vec<String>,
    /// The newest patch of the series, when it is newer than one in use.
    #[serde(default)]
    pub update: Option<String>,
    /// The series gets no more fixes upstream.
    #[serde(default)]
    pub eol: bool,
    /// What an update restarts.
    #[serde(default)]
    pub plugins: Vec<String>,
}

impl Interpreter {
    /// "Python 3.12.15", or the series when no patch is known.
    pub fn name(&self) -> String {
        let kind = {
            let mut c = self.kind.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        };
        match self.versions.as_slice() {
            [] => format!("{kind} {}", self.series),
            v => format!("{kind} {}", v.join(", ")),
        }
    }
}

/// A runtime's interpreters, the ones with an update first.
pub fn interpreters_of<'a>(all: &'a [Interpreter], runtime_id: &str) -> Vec<&'a Interpreter> {
    let mut v: Vec<&Interpreter> = all.iter().filter(|i| i.runtime_id == runtime_id).collect();
    v.sort_by_key(|i| i.update.is_none());
    v
}

/// " on Python 3.14.10" for a placement with an interpreter, else "".
pub fn interpreter_note(p: &Placement) -> String {
    match p.interpreter.as_deref().map(str::trim) {
        Some(i) if !i.is_empty() => {
            let mut c = i.chars();
            let first = c.next().map(|f| f.to_uppercase().collect::<String>());
            format!(" on {}{}", first.unwrap_or_default(), c.as_str())
        }
        _ => String::new(),
    }
}

pub fn placed_on<'a>(placements: &'a [Placement], runtime_id: &str) -> Vec<&'a Placement> {
    placements
        .iter()
        .filter(|p| p.runtime_id == runtime_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_runtimes_pythons_come_with_any_update_first() {
        let all: Vec<Interpreter> = serde_json::from_value(json!([
            {"runtime_id": "rt-1", "kind": "python", "series": "cp313", "versions": ["3.13.18"],
             "update": null, "eol": false, "plugins": ["plugin.a"]},
            {"runtime_id": "rt-1", "kind": "python", "series": "cp312", "versions": ["3.12.15"],
             "update": "3.12.16", "eol": false, "plugins": ["plugin.mcp"]},
            {"runtime_id": "rt-2", "kind": "python", "series": "cp314", "versions": ["3.14.10"]}
        ]))
        .unwrap();
        let mine = interpreters_of(&all, "rt-1");
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0].update.as_deref(), Some("3.12.16"));
        assert_eq!(mine[0].name(), "Python 3.12.15");
    }

    #[test]
    fn a_placement_says_which_python_it_runs_on() {
        let p: Placement = serde_json::from_value(json!({
            "runtime_id": "rt-1", "plugin_id": "plugin.x", "version": "0.1.0",
            "interpreter": "python 3.14.10"
        }))
        .unwrap();
        assert_eq!(interpreter_note(&p), " on Python 3.14.10");
        // Older cores send no interpreter at all.
        let old: Placement =
            serde_json::from_value(json!({"runtime_id": "rt-1", "plugin_id": "plugin.x"})).unwrap();
        assert_eq!(interpreter_note(&old), "");
    }

    fn rec(v: Value) -> PluginRecord {
        serde_json::from_value(v).unwrap()
    }

    fn registry() -> Vec<RegistryPlugin> {
        serde_json::from_value::<RegistryIndex>(json!({"plugins": [
            {"id": "plugin.sonos", "name": "Sonos", "description": "Speakers",
             "versions": [
                {"version": "0.1.9", "artifacts": [{"os": "linux", "arch": "x86_64", "size": 4194304, "runtime": "native"}]},
                {"version": "0.1.10", "artifacts": [{"os": "linux", "arch": "x86_64", "size": 4194304}]},
                {"version": "0.2.0", "min_core": "9.0.0", "artifacts": []}
             ]},
            {"id": "plugin.mcp", "name": "Assistant", "description": "Chat",
             "versions": [{"version": "0.1.4", "artifacts": [
                {"os": "linux", "arch": "x86_64", "runtime": "python"},
                {"os": "linux", "arch": "aarch64", "runtime": "python"}]}]},
            {"id": "plugin.hue", "name": "", "description": "Lights", "versions": []}
        ]}))
        .unwrap()
        .plugins
    }

    #[test]
    fn versions_compare_by_number() {
        assert_eq!(compare_versions("0.1.10", "0.1.9"), Ordering::Greater);
        assert_eq!(compare_versions("v1.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.1.2", "0.1.12"), Ordering::Less);
    }

    #[test]
    fn an_update_is_the_newest_version_that_runs_here() {
        let r = registry();
        let p = rec(json!({"plugin_id": "plugin.sonos", "version": "0.1.9"}));
        // 0.2.0 needs a newer core, so 0.1.10 is the update.
        assert_eq!(
            update_of(&p, &r, Some("0.1.116")).as_deref(),
            Some("0.1.10")
        );
        // Unknown core: every version counts.
        assert_eq!(update_of(&p, &r, None).as_deref(), Some("0.2.0"));
        let current = rec(json!({"plugin_id": "plugin.sonos", "version": "0.1.10"}));
        assert_eq!(update_of(&current, &r, Some("0.1.116")), None);
        // The installed version counts when the plugin does not say one.
        let quiet = rec(json!({"plugin_id": "plugin.sonos", "installed_version": "0.1.9"}));
        assert_eq!(
            update_of(&quiet, &r, Some("1.0.0")).as_deref(),
            Some("0.1.10")
        );
        let unknown = rec(json!({"plugin_id": "plugin.x", "version": "0.0.1"}));
        assert_eq!(update_of(&unknown, &r, None), None);
    }

    #[test]
    fn the_catalog_hides_installed_plugins() {
        let r = registry();
        let installed = vec![rec(json!({"plugin_id": "plugin.sonos"}))];
        let c: Vec<&str> = catalog(&r, &installed)
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(
            c,
            vec!["plugin.mcp", "plugin.hue"],
            "by name: Assistant, Hue"
        );
    }

    #[test]
    fn kinds_and_sizes_come_from_the_artifacts() {
        let r = registry();
        assert_eq!(r[0].kind(Some("0.1.10")), "native");
        assert_eq!(r[1].kind(None), "python");
        assert_eq!(r[2].kind(None), "-");
        assert_eq!(r[0].size_of("0.1.10").as_deref(), Some("4.0 MB"));
        assert_eq!(r[2].display_name(), "Hue");
    }

    #[test]
    fn notes_name_updates_notices_and_restarts() {
        let p = rec(json!({
            "plugin_id": "plugin.ecowitt", "enabled": true, "status": "active",
            "restart_count": 3,
            "notices": [
                {"code": "a", "level": "info", "message": "fine"},
                {"code": "b", "level": "error", "message": "no uploads", "remedy": "set bind_addr"}
            ]
        }));
        let notes = notes_of(&p, Some("0.1.27"));
        let words: Vec<&str> = notes.iter().map(|(w, _)| w.as_str()).collect();
        assert_eq!(
            words,
            vec!["update 0.1.27", "2 notices", "restarted 3 times"]
        );
        assert_eq!(notes[1].1, Tone::Bad, "an error notice is bad");
        assert_eq!(worst_notice(&p.notices).unwrap().code, "b");
        assert_eq!(state_of(&p), ("Running", Tone::Good));
        let off = rec(json!({"plugin_id": "x", "enabled": false, "status": "active"}));
        assert_eq!(state_of(&off).0, "Turned off (running)");
        assert!(is_running(&off), "turned off but still running");
    }

    #[test]
    fn install_answers_are_read() {
        assert_eq!(
            install_outcome(
                200,
                &json!({"installed": "plugin.x", "version": "0.1.2", "activated": true})
            ),
            InstallOutcome::Installed {
                version: "0.1.2".into(),
                activated: true
            }
        );
        assert_eq!(
            install_outcome(
                202,
                &json!({"placed_on": "rt-1", "plugin_id": "plugin.x", "version": "0.1.2"})
            ),
            InstallOutcome::Placed {
                runtime_id: "rt-1".into(),
                version: "0.1.2".into()
            }
        );
        assert_eq!(
            install_outcome(
                409,
                &json!({"error": "choose one", "runtimes": ["rt-1", "rt-2"]})
            ),
            InstallOutcome::ChooseRuntime(vec!["rt-1".into(), "rt-2".into()])
        );
        assert_eq!(
            install_outcome(422, &json!({"error": "needs homeCore 9"})),
            InstallOutcome::Failed("needs homeCore 9".into())
        );
    }

    #[test]
    fn runtime_status_reads_last_seen() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-17T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let mut r = Runtime {
            runtime_id: "rt".into(),
            last_seen_at: Some("2026-11-17T11:59:30Z".into()),
            ..Default::default()
        };
        assert_eq!(runtime_status(&r, now).1, Tone::Good);
        r.last_seen_at = Some("2026-11-17T09:00:00Z".into());
        assert_eq!(runtime_status(&r, now).0, "Not heard from for 3 h");
        r.outdated = true;
        assert_eq!(runtime_status(&r, now).0, "Needs a newer image");
        r.last_seen_at = None;
        r.outdated = false;
        assert_eq!(runtime_status(&r, now).0, "Waiting for it to connect");
    }

    #[test]
    fn an_issued_key_never_prints() {
        let k: IssuedKey = serde_json::from_value(json!({
            "runtime": {"runtime_id": "rt-1"}, "key": "hcrt_secret"
        }))
        .unwrap();
        assert!(!format!("{k:?}").contains("hcrt_secret"));
    }
}
