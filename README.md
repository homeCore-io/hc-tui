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
  login screen. `L` signs out (`/api/v1/auth/logout`).
- The app's screens and names. The sidebar has 1 Home, 2 Rooms,
  3 Devices, 4 Scenes, 5 Modes, 6 Helpers, 7 Automations, 8 Alerts (with
  the number of open alerts) and 9 Plugins, then under Settings: People,
  Data & backups, Logs, Events, Audit and System. A screen shows only to
  roles that can use it, from the scopes `/auth/me` sends (core's role
  table when it sends none): Plugins needs `plugins:read`, People
  `users:read`, Audit `audit:read`, Logs the admin role, and Data &
  backups the admin role or `automations:write`.
- The header names the homeCore version, the modes that are on and the
  outside temperature when a weather device reports one; on the right,
  your role in words and whether the live stream is up.
- Home, the first screen: rooms, devices, scenes, modes, helpers and
  automations you pick, under headings you name, each with its live
  state. Space does the item's main action: a room's lights on or off,
  a light or switch toggled, a scene activated, a manual mode switched,
  a helper's own action (a timer started or paused, a counter counted
  up), an automation turned on or off. Home is kept per server and per
  person in hc-tui's config folder (see Files); an item whose target is
  gone shows as missing until you take it off.
- Rooms: each room with how many lights are on, a dot per light, the
  mean temperature and notes (open, low battery, offline), and the
  selected room's devices below. Space turns the whole room on or off
  (`POST /areas/{id}/on|off`); Enter opens its devices.
- Devices: on/off, brightness (`brightness_pct`), locks, and a Media
  players tab with transport, volume, mute, progress and metadata.
  Editing the name (a person's rename; empty restores the plugin's
  name), room and canonical name sends only the fields that changed.
- Scenes: homeCore scenes and scene devices (`device_type: "scene"`).
- Modes: Space turns a manual mode on or off; solar modes follow the
  sun.
- Helpers: homeCore's own switches, timers, counters and the rest, with
  Space for each type's main action and `n` to make one.
- Alerts: the open and acknowledged alerts from `GET /alerts`, kept live
  from the event stream.
- Automations: detail view with RON and Recent runs, groups, marking
  several to switch at once.
- Plugins, in tabs as the app has them:
  - Installed: each plugin's version, state, devices and notes (a newer
    version in the registry, notices, restarts). Admins turn a plugin on
    or off, start, stop, restart, update and uninstall it. Its detail has
    Overview, Settings, Actions, Notices and Logs. Settings is a form made
    from the plugin's config descriptor (or its schema), with its log
    level; only what changed is written, in place, so comments and keys
    the form does not know stay in the file. Secrets are never shown.
    Actions that take parameters ask for them first; dangerous ones ask
    before they run. Logs streams the plugin's own log while the tab is
    open.
  - Catalog: plugins in the registry that are not installed, and
    install. When more than one runtime can run one, hc-tui asks which.
  - Runtimes (admins): each plugin runtime, how it is doing, its host
    version and the plugins placed on it; Reset login makes it a new key,
    shown once.
  - Matter, when the Matter plugin is installed: commission, reinterview
    and remove nodes.
- Temperatures are shown in the unit set on homeCore
  (`/system/status` `temperature_unit`); values on the bus are °C.
- Live updates over `/api/v1/events/stream`: devices, rooms, scenes,
  automations, modes, alerts and plugin status update in place. The
  stream reconnects with backoff (1s to 30s) and resyncs after a
  reconnect or a `lagged` notice.
- Every delete, uninstall and import asks for `y` first.

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
- Home: `$XDG_CONFIG_HOME/hc-tui/home/<server>-<user>.json`
  (`~/.config/hc-tui/home`). Core keeps no UI settings and the app's
  favorites live on the app's own server, so this Home is hc-tui's
  alone.
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
- The log WebSocket is open only while Logs is on screen.

## Key Bindings

The footer shows the main keys of the screen you are on; `?` lists all
of them. Both come from the same table in each screen's module
(`src/screens/`).

Everywhere:

| Key | Does |
| --- | --- |
| `1` to `9`, `Tab`, `Shift+Tab` | jump between screens |
| `j`/`k` or arrows | move |
| `Enter` | open or edit |
| `Space` | the main action: on or off, enable or disable, activate |
| `n` | new |
| `x` | delete, remove or uninstall; always asks first (Home's `x` only takes an item off Home, so it does not ask) |
| `/` | search the current list as you type; `Enter` keeps it, `Esc` clears it |
| `*` | pin or unpin the selected item on Home |
| `r` | refresh |
| `?` | every key for this screen |
| `Esc` | back or close |
| `T` | local or UTC time |
| `L` | sign out |
| `q`, `Ctrl+C` | quit |

Login screen: `Tab` switch field, `Enter` sign in, `Esc` quit.

Screens:

- Home: `Space` main action, `Enter` open the item on its own screen,
  `a` add (a picker: type to search, `Tab` kind, `←/→` heading, `Enter`
  add, `Esc` done), `x` take off Home, `J`/`K` move, `h` rename the
  heading (`Tab` in the box adds a new heading instead).
- Rooms: `Space` room on/off, `Enter` its devices (`Esc` comes back),
  `n` new room, `e` rename, `x` delete.
- Devices: `Space`/`t` on/off, `+`/`-` brightness, `l`/`u` lock and
  unlock, `f` filter, `s` sort, `v` grouped or flat, `Enter` edit, `x`
  remove, `←/→` Media players.
- Media players: `Space` play or stop, `p` play or pause, `s` stop, `n`
  next, `b` previous, `+`/`-` volume, `m` mute, `Enter` edit, `x` remove.
- Scenes: `Space` (or `a`) activate.
- Modes: `Space` mode on/off, `n` new, `x` delete.
- Helpers: `Space` main action, `n` new helper.
- Automations: `Space` on/off (the marked ones when any are marked),
  `Enter` detail, `h` Recent runs, `m` mark, `c` copy, `x` delete, `g`
  groups (`+`/`-` on and off, `x` delete), `f` filter, `s` only those
  that need fixing. In the detail: `j`/`k` scroll, `r` refresh, `Esc`
  back.
- Plugins: `←/→` tabs (Installed, Catalog, Runtimes, Matter). Installed:
  `Enter` detail, and for admins `Space` on/off, `s` start or stop, `R`
  restart, `U` update (when there is one), `x` uninstall. In the detail
  `←/→` moves between its tabs; Settings: `Enter` edit, `Space` change a
  choice, `u` undo a change, `Ctrl+S` save, `Esc` discard; Actions:
  `Enter` run; Logs: `Space` pause, `e`/`w`/`i`/`d` level, `c` clear.
  Catalog: `i` install. Runtimes: `Enter` detail, `R` reset login.
  Matter: `c` commission, `i` reinterview, `x` remove a node.
- People: `n` new person, `Enter` change role, `x` delete (admin); `p`
  change your password.
- Data & backups: `Enter` run.
- Logs: `Space`/`p` pause, `e`/`w`/`i`/`d` level (error, warn, info,
  debug), `/` filter, `c` clear.
- Events: `f` event type.
- Audit: `Enter` detail, `]` next page, `[` previous page.

## homeCore API Integration

This client uses:
- `POST /auth/login`, `POST /auth/refresh`, `POST /auth/logout`,
  `GET /auth/me`, `POST /auth/change-password`
- `GET /devices`, `PATCH /devices/{id}`, `PATCH /devices/{id}/state`,
  `DELETE /devices/{id}`
- `GET /scenes`, `POST /scenes/{id}/activate`, `/scenes/export|import`
- `GET|POST /areas`, `PATCH|DELETE /areas/{id}`, `POST /areas/{id}/on`,
  `POST /areas/{id}/off`
- `GET /alerts`
- `GET /automations` and `/automations/{id}` (`PATCH`, `DELETE`,
  `/clone`, `/history`, `/ron`), `PATCH /automations`,
  `/automations/groups`, `/automations/export|import`
- `GET /events`, `GET /events/stream` (WebSocket), `GET /logs/stream`
  (WebSocket, admin)
- `GET /system/status`, `POST /system/backup`, `GET /audit`
- `GET|POST /modes`, `DELETE /modes/{id}`, `POST /glue`; manual modes
  and helpers are switched with `PATCH /devices/{id}/state`
- `GET|POST /auth/users`, `DELETE /auth/users/{id}`,
  `PATCH /auth/users/{id}/role` (admin)
- `GET /plugins`, `PATCH|DELETE /plugins/{id}`,
  `POST /plugins/{id}/start|stop|restart`, `GET /plugins/{id}/capabilities`,
  `GET|PUT /plugins/{id}/config`, `GET /plugins/{id}/config/descriptor`,
  `GET /plugins/{id}/config/schema`, `POST /plugins/{id}/command`,
  `GET /plugins/{id}/command/{rid}/stream` (SSE), `POST /plugins/install`,
  `GET /registry/plugins`, `GET /plugin-runtimes`,
  `GET /plugin-runtimes/placements`, `POST /plugin-runtimes/{id}/reset`;
  a plugin's log is `GET /logs/stream?plugin_id=` (WebSocket, admin)
- `GET /plugins/matter/nodes`, `POST /plugins/matter/commission`,
  `POST /plugins/matter/reinterview`, `DELETE /plugins/matter/nodes/{id}`

All paths are under `/api/v1`.
