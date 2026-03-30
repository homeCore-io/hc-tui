# hc-tui

`hc-tui` is a terminal UI client for the HomeCore home automation server.

It is built with:
- Rust
- [`ratatui`](https://github.com/ratatui/ratatui/)
- `crossterm`
- `reqwest`

## What It Supports

- Login using HomeCore JWT auth (`/api/v1/auth/login`)
- Role-aware UI:
  - `user` / `read_only`: devices, scenes, areas, automations, events
  - `admin`: all of the above + users + plugins
- Device control:
  - Toggle selected device `on` state
  - Toggle selected media players between play and stop, with pause fallback
  - View and edit device `canonical_name` alongside name and area
- Scene control:
  - Activate selected scene
- Data views:
  - Devices
  - Scenes
  - Areas
  - Automations
  - Events
  - Users (admin)
  - Plugins (admin)
  - Manage > Matter (admin)
    - Commission
    - List commissioned nodes
    - Reinterview selected node
    - Remove selected node
- Live updates:
  - Connects to `/api/v1/events/stream?token=...` via WebSocket
  - Applies device state/availability/name changes in real time
  - Device search matches display names, raw `device_id`, and `canonical_name`

## Run

```bash
cargo run -- --base-url http://127.0.0.1:8080 --cache-dir ./cache
```

`--base-url` should point to the HomeCore server root (without `/api/v1`).
`--cache-dir` stores local JSON cache snapshots per user.

## Caching Behavior

- On login, the TUI loads cached state/config first (if present), then syncs from HomeCore.
- Manual refresh (`r`) pulls fresh data and updates cache files.
- Device/scene actions sync and re-cache after completion.
- There is no background auto-refresh loop while navigating selections.
- Real-time device status comes from WebSocket events. If the stream drops, the TUI auto-reconnects.

## Key Bindings

- Login screen:
  - `Tab` switch field
  - `Enter` submit login
  - `Esc` quit
- Main UI:
  - `Tab` / `Shift+Tab` switch tabs
  - `j` / `k` or `Down` / `Up` move selection
  - `r` refresh data
  - `q` quit
  - `t` toggle selected device (Devices tab)
    - switches/lights: on/off
    - media players: play/stop, or pause when stop is unsupported
  - `e` edit selected device metadata including canonical name (Devices tab, admin)
  - `a` activate selected scene (Scenes tab)

- Manage > Matter (admin):
  - `c` open commission form (pairing code, optional name/room/discriminator/passcode)
  - `r` refresh node inventory
  - `i` request reinterview for selected node
  - `d` request removal for selected node

## HomeCore API Integration

This client targets the existing endpoints in `homeCore`:
- `POST /api/v1/auth/login`
- `GET /api/v1/auth/me`
- `GET /api/v1/devices`
- `PATCH /api/v1/devices/{id}/state`
- `GET /api/v1/scenes`
- `POST /api/v1/scenes/{id}/activate`
- `GET /api/v1/areas`
- `GET /api/v1/automations`
- `GET /api/v1/events?limit=...`
- `GET /api/v1/auth/users` (admin)
- `GET /api/v1/plugins` (admin)
- `POST /api/v1/plugins/matter/commission` (admin)
- `GET /api/v1/plugins/matter/nodes` (admin)
- `POST /api/v1/plugins/matter/reinterview` (admin)
- `DELETE /api/v1/plugins/matter/nodes/{id}` (admin)
