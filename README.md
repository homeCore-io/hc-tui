# hc-tui

[![CI](https://github.com/homeCore-io/hc-tui/actions/workflows/ci.yml/badge.svg)](https://github.com/homeCore-io/hc-tui/actions/workflows/ci.yml) [![Release](https://github.com/homeCore-io/hc-tui/actions/workflows/release.yml/badge.svg)](https://github.com/homeCore-io/hc-tui/actions/workflows/release.yml) [![Dashboard](https://img.shields.io/badge/builds-dashboard-blue?style=flat-square)](https://homecore-io.github.io/ci-glance/)

`hc-tui` is a terminal UI client for the homeCore home automation server.

It is built with:
- Rust
- [`ratatui`](https://github.com/ratatui/ratatui/)
- `crossterm`
- `reqwest`

## What It Supports

- Login with homeCore auth (`/api/v1/auth/login`). The session (access
  token, refresh token and expiry) is saved and refreshed before it
  expires; when it can no longer be refreshed the TUI returns to the
  login screen. `L` logs out (`/api/v1/auth/logout`).
- Role-aware UI. The Plugins tab shows for roles with `plugins:read`
  (admin, observer, device operator, rule editor); actions that need the
  admin role stay admin-only.
- Device control: on/off, brightness (`brightness_pct`), locks, and a
  Media Players sub-tab with generic transport, volume, mute, progress
  and metadata.
- Device editing: name (a person's rename; empty restores the plugin's
  name), room and canonical name. Only changed fields are sent.
- Scenes: homeCore scenes and scene devices (`device_type: "scene"`).
- Areas, rules (detail view with RON and fire history, groups, bulk
  enable/disable), plugins (overview, actions, streaming actions,
  recent events), and Manage: modes, Matter, system status, users, logs,
  events, audit and backup.
- Temperatures are shown in the unit set on homeCore
  (`/system/status` `temperature_unit`); values on the bus are °C.
- Live updates over `/api/v1/events/stream`: devices, rooms, scenes,
  rules, modes and plugin status update in place. The stream reconnects
  with backoff (1s to 30s) and resyncs after a reconnect or a `lagged`
  notice.
- Every destructive action asks for `y` first.

## Run

```bash
cargo run -- --base-url http://127.0.0.1:8080
```

`--base-url` should point to the homeCore server root (without `/api/v1`).

Files:
- Config: `--config`, else `$XDG_CONFIG_HOME/hc-tui/config.toml`
  (`~/.config/hc-tui/config.toml`), else `./config/config.toml` when only
  that exists. See `config/config.toml.example`.
- Cache: `--cache-dir`, else `[cache] dir` from the config, else
  `$XDG_CACHE_HOME/hc-tui` (`~/.cache/hc-tui`).
- Backups and exports go to `~/.homecore/exports`; imports are read from
  `~/.homecore/imports`.

Directories the TUI creates are 0700 and files 0600.

## Caching Behavior

- After sign-in the TUI shows cached data at once and syncs in the
  background. A spinner in the status bar shows work in flight.
- Every call of a sync runs at once and succeeds or fails on its own.
  Failures are named in the status bar, and only data from calls that
  succeeded is written to the cache, so a failed call never replaces
  good cached data.
- `r` syncs again. Live changes come from the WebSocket.
- The log WebSocket is open only while Manage > Logs is on screen.

## Key Bindings

- Everywhere: `Ctrl+C` quits.
- Login screen: `Tab` switch field, `Enter` sign in, `Esc` quit.
- Main UI:
  - `Tab` / `Shift+Tab` or `1`-`9` switch tabs
  - `j` / `k` or `Down` / `Up` move
  - `r` refresh, `q` quit, `T` local/UTC time, `L` log out
- Devices: `◄/►` All / Media Players, `Space`/`t` toggle, `+`/`-`
  brightness, `l`/`u` lock/unlock, `v` grouped/flat, `f` filter, `s` sort,
  `/` search, `Enter` edit, `d` delete
- Media Players: `Space`/`t` play-stop, `p` play/pause, `x` stop, `n` next,
  `b` previous, `+`/`-` volume, `m` mute, `Enter` edit
- Scenes: `a` or `Enter` activate
- Areas: `◄/►` pane, `n` new, `Enter` open the area (left) or rename it
  (right), `Space` mark a device, `-`/`d` remove marked devices (right),
  `d` delete the area (left)
- Rules: `Enter` detail, `e`/`d` enable/disable (marked rules when any),
  `Space` mark, `c` clone, `x` delete, `h` history, `f` filter, `s` stale
  only, `g` groups
- Plugins: `Enter` detail (`1`-`3` or `◄/►` panels, `Enter` run action),
  `d` uninstall (admin)
- Manage: `◄/►` panel, `g` create a helper
  - Modes: `n` new, `d` delete
  - Matter: `c` commission, `i` reinterview, `d` remove
  - Users: `n` new, `Enter` change role, `p` change your password, `d`
    delete
  - Logs: `p`/`Space` pause, `e`/`w`/`i`/`d` level, `/` filter, `c` clear
  - Events: `f` cycle event type filter
  - Audit: `n`/`p` page, `Enter` detail
  - Backup: `Enter` run

## homeCore API Integration

This client uses:
- `POST /auth/login`, `POST /auth/refresh`, `POST /auth/logout`,
  `GET /auth/me`, `POST /auth/change-password`
- `GET /devices`, `PATCH /devices/{id}`, `PATCH /devices/{id}/state`,
  `DELETE /devices/{id}`
- `GET /scenes`, `POST /scenes/{id}/activate`, `/scenes/export|import`
- `GET|POST /areas`, `PATCH|DELETE /areas/{id}`, `PUT /areas/{id}/devices`
- `GET /automations` and `/automations/{id}` (`PATCH`, `DELETE`,
  `/clone`, `/history`, `/ron`), `PATCH /automations`,
  `/automations/groups`, `/automations/export|import`
- `GET /events`, `GET /events/stream` (WebSocket), `GET /logs/stream`
  (WebSocket, admin)
- `GET /system/status`, `POST /system/backup`, `GET /audit`
- `GET|POST /modes`, `DELETE /modes/{id}`, `POST /glue`
- `GET|POST /auth/users`, `DELETE /auth/users/{id}`,
  `PATCH /auth/users/{id}/role` (admin)
- `GET /plugins`, `DELETE /plugins/{id}`, `GET /plugins/{id}/capabilities`,
  `POST /plugins/{id}/command`, `GET /plugins/{id}/command/{rid}/stream`
  (SSE)
- `GET /plugins/matter/nodes`, `POST /plugins/matter/commission`,
  `POST /plugins/matter/reinterview`, `DELETE /plugins/matter/nodes/{id}`

All paths are under `/api/v1`.
