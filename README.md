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
  (`POST /areas/{id}/on|off`); Enter opens its devices, where `a`
  adds devices to the room and `x` takes one out
  (`PUT /areas/{id}/devices` with the room's fresh list).
- Devices: tabs All, Lights, Climate, Media, Covers, Locks, Sensors,
  Offline (with the count) and Removed, sorted into the categories the
  way the app does it. The detail pane has controls built from the
  device's schema (`GET /devices/{id}/schema`): every writable everyday
  setting (on and off, brightness, color temperature, color presets,
  position, lock, mode, setpoints, fan) and the actions that need no
  input, with the shared vocabulary for a device without a schema.
  Changes are sent 600 ms after the last key. Temperatures are shown
  and stepped in the house's unit (1°F or 0.5°C a step) and sent in
  °C. A sparkline shows the last 24 hours of the device's main reading
  (`GET /devices/{id}/history`). Unlocking, and an action that asks
  first, needs `y`. Editing the name (a person's rename; empty restores
  the plugin's name), room, Security choice (yes, no, or as its type
  says) and the name in automations sends only the fields that
  changed. Removing a device first says how many automations use it.
  Removed lists the devices homeCore was told to forget; Space lets one
  come back. The Media tab has transport, volume, mute, progress and
  metadata.
- Scenes: homeCore scenes and scene devices (`device_type: "scene"`),
  with where each comes from and how many devices or which room it
  has. homeCore scenes can be made, edited and deleted: pick devices
  and each is taken as it is now (its first few everyday settings, or
  just off). A plugin's scenes only run.
- Modes: Space turns a manual mode on or off; solar modes follow the
  sun and show their next change (`until sunset 16:46`). A mode's name,
  and a solar mode's events and offsets, can be edited.
- Helpers: every kind of helper core has (switch, timer, counter,
  number, select, text, button, date and time, group, threshold,
  schedule) with its state and its keys. Running timers count down
  live. `n` makes one (a kind, then that kind's settings), `Enter`
  changes its name and settings (`PATCH /glue/{id}` sends only what
  changed), `x` deletes it and names the automations that use it.
- Alerts: Open, Acknowledged, Rules and Ignored. The selected alert
  shows where it came from (its alert rule and where that rule tells,
  or the plugin) and what can be done: acknowledge, press its button,
  ignore its device. Plugin kinds lists the kinds of alert each plugin can
  raise: `Space` turns one off or on, `d` goes back to the plugin's choice.
  ignore its device. Alert rules can be turned on or off and deleted;
  deleting one that alone starts automations names them and asks whether
  to keep them, turned off, or delete them too. Making rules is left to
  the app. Kept live from the event stream,
  with the open count on the sidebar.
- Devices: `h` opens a device's history: any number or on/off reading,
  `←/→` for 1 hour to 30 days, with the lowest, highest and latest, or
  each change and when.
- Automations: `v` shows the hub variables rules have set, as they are
  now, and updates as automations run.
- Automations: what starts each one in words (`sunset -15m`,
  `Garage Door open 10m`, `time 17:00`), its last run and group, a mark
  for one another rule paused (‖) and for one that needs fixing (!,
  naming the missing device). The selected one shows When / If / Then
  and its last 20 runs with how each went. Run now, a dry run at a time
  (nothing is sent), resume a paused one, send a missing device's place
  to another device, the RON read only, groups, marking several to
  switch at once. Automations are still written in the app or as RON
  files.
- Plugins, in tabs as the app has them:
  - Installed: each plugin's version, state, devices and notes (a newer
    version in the registry, notices, restarts). Admins turn a plugin on
    or off, start, stop, restart, update and uninstall it. Its detail has
    Overview, Settings, Actions, Notices and Logs. Settings is a form made
    from the plugin's config descriptor (or its schema), with its log
    level; only what changed is written, in place, so comments and keys
    the form does not know stay in the file. Secrets are never shown.
    Actions that take parameters ask for them first; dangerous ones ask
    before they run. `f` runs one with a file from this computer (such as
    WLED's firmware install), asking for its path with the other
    parameters. Read-only actions run for anyone who can read devices.
    Logs streams the plugin's own log while the tab is
    open.
  - Catalog: plugins in the registry that are not installed, and
    install. When more than one runtime can run one, hc-tui asks which.
  - Runtimes (admins): each plugin runtime, how it is doing, its host
    version and the plugins placed on it, with the Python each runs on;
    Reset login makes it a new key,
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
- Devices: `←/→` tab, `Tab` into the controls (`Shift+Tab` back,
  `←/→` change, `Space` switch or press, `Esc` out), `Space`/`t` on/off,
  `+`/`-` brightness, `l`/`u` lock and unlock, `f` filter, `s` sort,
  `v` grouped or flat, `Enter` or `e` edit, `x` remove. A room's devices
  (from Rooms): `a` add devices (type to search, `Space` mark, `Enter`
  add), `x` take out of the room. Removed: `Space` bring back.
- Media: `Space` play or stop, `p` play or pause, `s` stop, `>` next,
  `<` previous, `+`/`-` volume, `m` mute, `Enter` edit.
- Scenes: `a` (or `Space`) activate, `n` new, `Enter` edit, `x` delete.
  In the editor: `Tab` name or devices, `a` add devices, `x` remove
  one, `c` take how it is now, `Enter` save.
- Modes: `Space` mode on/off, `n` new, `Enter` edit (`←/→` kind, sun
  event and offset), `x` delete.
- Helpers: `Space` the kind's main action (toggle, start or pause,
  up one, press, next choice), `n` new helper, `Enter` edit, `x`
  delete. A timer adds `0` cancel; a counter `+` up, `-` down, `0`
  reset; a number `←`/`→` a step and `s` set to; a select `←`/`→` the
  previous or next choice; text and date and time `s` set. In the
  editor: `Tab` field, `Space` or `←`/`→` change a choice, `Enter`
  save, `Esc` cancel.
- Automations: `Space` on/off (the marked ones when any are marked),
  `R` run now (asks first), `t` test at a time (`22:30`,
  `2026-11-18T07:00`, or nothing for now; a dry run), `u` resume one
  another rule paused, `F` send a missing device's place to another
  device (type to search, `Tab` the next missing one), `Enter` the RON,
  `m` mark, `c` copy, `x` delete, `g` groups (`+`/`-` on and off, `x`
  delete), `f` filter, `s` only those that need fixing. In the RON:
  `j`/`k` scroll, `r` refresh, `R`, `t`, `u` and `F` as above, `Esc`
  back.
- Alerts: `←`/`→` Open, Acknowledged, Rules, Ignored. On an alert: `a`
  acknowledge, `p` run its action, `i` ignore its device. Rules:
  `Space` on/off, `x` delete. Ignored: `x` show them again.
- Plugins: `←/→` tabs (Installed, Catalog, Runtimes, Matter). Installed:
  `Enter` detail, and for admins `Space` on/off, `s` start or stop, `R`
  restart, `U` update (when there is one), `x` uninstall. In the detail
  `←/→` moves between its tabs; Settings: `Enter` edit, `Space` change a
  choice, `u` undo a change, `Ctrl+S` save, `Esc` discard; Actions:
  `Enter` run, `f` run with a file; Logs: `Space` pause, `e`/`w`/`i`/`d` level, `c` clear.
  Catalog: `i` install. Runtimes: `Enter` detail, `R` reset login.
  Matter: `c` commission, `i` reinterview, `x` remove a node.
- People: `←/→` tab (People, Roles, API keys, Sessions).
  - People: `n` new person, `Enter` change role, `x` delete, `c` remove
    PIN (admin); `p` changes your password, or an admin sets someone
    else's.
  - Roles: what each role may do.
  - API keys: `n` new key (a label and what it may do), `s` new secret,
    `x` revoke. A key's secret is shown once, in a dialog; hc-tui never
    writes it anywhere.
  - Sessions: `x` end one, `X` end all others. This one stays; `L` signs
    out.
- Data & backups: `n` back up now; on a kept backup `Enter` downloads
  it and `R` restores it (type the backup's date to go ahead); `Enter`
  runs an action (restore from a file, export, import); `x` deletes a
  calendar. Downloads and exports go to `~/.homecore/exports`, 0600. A
  restore is staged for core's next start, and you're asked about
  restarting.
- Logs: `Space`/`p` pause, `e`/`w`/`i`/`d` level (error, warn, info,
  debug), `c` clear. `/` searches the lines kept so far.
- Events: `f` event type.
- Audit: `Enter` detail, `]` next page, `[` previous page.
- System: `c` checks automations, scenes and devices against what devices
  take (a report; it changes nothing); `←/→` core's log level, until it restarts (`plugins:write`);
  `R` restart core (admin, asks first).
- `/` searches People, API keys, Data & backups, Logs, Events and Audit.

## homeCore API Integration

This client uses:
- `POST /auth/login`, `POST /auth/refresh`, `POST /auth/logout`,
  `GET /auth/me`, `POST /auth/change-password`
- `GET /devices`, `PATCH /devices/{id}`, `PATCH /devices/{id}/state`,
  `DELETE /devices/{id}`, `GET /devices/{id}/schema`,
  `GET /devices/{id}/history`, `GET /devices/removed`,
  `DELETE /devices/removed/{id}`, `GET /automations?device_id=&limit=0`
  (`X-Total-Count`)
- `GET|POST /scenes`, `PUT|DELETE /scenes/{id}`,
  `POST /scenes/{id}/activate`, `/scenes/export|import`
- `GET|POST /areas`, `PATCH|DELETE /areas/{id}`, `POST /areas/{id}/on`,
  `POST /areas/{id}/off`, `PUT /areas/{id}/devices`
- `GET /alerts`, `POST /alerts/{id}/acknowledge`,
  `POST /alerts/{id}/press`, `GET /alert-rules`,
  `PUT|DELETE /alert-rules/{id}` (`?automations=disable`),
  `GET|POST /alert-ignores`, `DELETE /alert-ignores/{id}`
- `GET /automations` and `/automations/{id}` (`PATCH`, `DELETE`,
  `/clone`, `/history`, `/ron`, `POST /run`, `POST /test?at=`,
  `DELETE /pause`), `PATCH /automations`, `/automations/groups`,
  `/automations/export|import`, `GET /automations/paused`,
  `GET /automations/stale-refs`, `POST /automations/repoint`,
  `GET /devices/removed` (names of devices a rule still names)
- `GET /events`, `GET /events/stream` (WebSocket), `GET /logs/stream`
  (WebSocket, admin)
- `GET /system/status`, `POST /system/backup`, `GET /audit`
- `GET /health`, `GET /system/versions`, `GET|PUT /system/log-level`,
  `GET /system/temperature-unit`, `GET /ws/connections` (admin),
  `POST /system/restart` (admin)
- `GET /system/backups`, `GET /system/backups/{name}`,
  `POST /system/backups/{name}/restore`, `POST /system/restore` (admin)
- `GET /calendars`, `POST /calendars/upload`, `DELETE /calendars/{id}`
- `GET /auth/roles`, `GET|POST /auth/api-keys`,
  `DELETE /auth/api-keys/{id}`, `POST /auth/api-keys/{id}/rotate`,
  `GET /auth/sessions`, `DELETE /auth/sessions/{id}`
- `GET|POST /modes`, `PATCH|DELETE /modes/{id}`, `POST /glue`,
  `PATCH|DELETE /glue/{id}`; manual modes and helpers are switched with
  `PATCH /devices/{id}/state` (`{"action": ...}` for a helper)
- `GET|POST /auth/users`, `DELETE /auth/users/{id}`,
  `PATCH /auth/users/{id}/role`, `PATCH /auth/users/{id}/password`,
  `DELETE /auth/users/{id}/pin` (admin)
- `GET /plugins`, `PATCH|DELETE /plugins/{id}`,
  `POST /plugins/{id}/start|stop|restart`, `GET /plugins/{id}/capabilities`,
  `GET|PUT /plugins/{id}/config`, `GET /plugins/{id}/config/descriptor`,
  `GET /plugins/{id}/config/schema`, `POST /plugins/{id}/command`,
  `GET /plugins/{id}/command/{rid}/stream` (SSE), `POST /plugins/install`,
  `GET /registry/plugins`, `GET /plugin-runtimes`,
  `GET /plugin-runtimes/placements`, `POST /plugin-runtimes/{id}/reset`,
  `POST /plugins/{id}/query` (read-only actions), `POST /plugins/{id}/upload/{action}`,
  `GET /alert-rules/{id}/automations`, `GET /alerts/kinds`,
  `PUT /alerts/kinds/{plugin_id}/{topic}`;
  a plugin's log is `GET /logs/stream?plugin_id=` (WebSocket, admin)
- `GET /plugins/matter/nodes`, `POST /plugins/matter/commission`,
  `POST /plugins/matter/reinterview`, `DELETE /plugins/matter/nodes/{id}`

All paths are under `/api/v1`.
