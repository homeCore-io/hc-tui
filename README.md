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
  - `a` activate selected scene (Scenes tab)

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
