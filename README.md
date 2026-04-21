<p align="center">
  <img src="resources/icons/r-wg.svg" alt="r-wg app icon" width="128" height="128" />
</p>

# r-wg

**A native WireGuard desktop client that behaves like an app, not a setup script.**

r-wg is a Rust-based WireGuard client with a native desktop UI, a managed privileged backend, and built-in tunnel diagnostics.

For normal use, the workflow is simple:

1. launch the app,
2. install the backend once,
3. import or paste a tunnel,
4. click **On**.

That is the main experience this project is built around.

## Why r-wg exists

A lot of WireGuard desktop tooling still feels like infrastructure work: manual privilege setup, shell wrappers, `sudo`, `setcap`, DNS cleanup, and a mental model that leaks too much of the networking stack into the user flow.

r-wg takes a different approach. It keeps privileged operations in a dedicated backend, keeps the UI unprivileged, and turns the everyday tunnel workflow into something that feels like a normal desktop app.

This is not just a parser plus a start button. The app understands configs, validates them, keeps a local tunnel library, surfaces runtime health, and gives you a route/DNS view when you need to understand what the system is actually doing.

## Everyday workflow

### 1) Launch r-wg

Open the desktop app normally.

### 2) Install the privileged backend once

Open:

**Preferences -> System -> Privileged Backend**

Then click:

**Install**

This is a one-time setup step for the machine.

After that, r-wg can manage tunnel startup, routes, DNS changes, and cleanup through the backend automatically. You do **not** need to run the whole app as root/Administrator for day-to-day use.

### 3) Import or paste a WireGuard config

Open **Configs** and either:

- import one or more `.conf` files, or
- paste a config directly into the editor.

The app validates the config, keeps it in the local library, and shows a preview/diagnostics view before you start it.

### 4) Save the tunnel

Save the config into the app library.

### 5) Turn it on

Use the top bar **On** button, or connect it from the saved tunnel/library views.

Once the backend is installed, that is the normal operating model: select tunnel, click on, monitor status, click off.

## What the app already does well

### App-first tunnel control

- one-time privileged backend installation from inside the UI
- normal tunnel control from an unprivileged desktop app
- no daily `sudo`, `setcap`, or wrapper-script workflow

### Config library and editor

- import multiple WireGuard configs
- paste configs directly into the app
- edit, validate, save, and export configs
- keep a persistent library of saved tunnels

### Runtime visibility

- connection state and tunnel status
- peer stats and handshake age
- upload/download counters and short-term traffic history
- recent traffic summaries and trends
- in-app log viewer with copy/clear controls

### Smarter networking defaults

- supports standard WireGuard config files plus common `wg-quick` fields
- DNS handling can follow the config, follow the system, fill missing families, or override with presets
- Route Map helps explain planned routes, guardrails, and runtime apply results

### Desktop integration

- tray support
- desktop notifications for connect, disconnect, and failure states
- a settings surface for backend diagnostics, repair, and recovery actions

## Platform support

### Linux

Supported.

Linux uses a socket-activated privileged backend managed by `systemd`. The backend is started when needed, handles tunnel control and DNS/route operations, and supports repair/recovery flows for stale state after failures.

### Windows

Supported.

Windows uses a managed privileged backend service. The build also places `wintun.dll` into the output directory automatically so the app can run from the build output without extra manual copying.

### macOS

Not implemented yet.

The UI can exist there, but the network configuration backend is still a placeholder.

## Supported configuration format

r-wg supports standard WireGuard `.conf` files and common `wg-quick` style fields, including:

- `Address`
- `DNS`
- `MTU`
- `Table`

Example:

```ini
[Interface]
PrivateKey = <base64>
Address = 10.0.0.2/32
DNS = 1.1.1.1, 8.8.8.8
MTU = 1420
Table = auto

[Peer]
PublicKey = <base64>
AllowedIPs = 0.0.0.0/0, ::/0
Endpoint = example.com:51820
PersistentKeepalive = 25
```

## How privilege is handled

Bringing up a WireGuard tunnel requires privileged operations such as:

- creating or controlling the tunnel device,
- applying interface addresses,
- installing routes,
- changing DNS,
- cleaning up system state on stop or failure.

r-wg keeps those responsibilities in a dedicated backend instead of turning the entire desktop UI into a privileged process.

That is why the primary setup step is **Install backend**, not **run the whole app as root**.

## Building from source

### Requirements

- Rust stable toolchain
- Linux desktop builds need the native dependencies used in CI
- Linux runtime needs `systemd` for the privileged backend integration
- DNS changes on Linux use `resolvectl` or `resolvconf` when available

### Linux build dependencies

```sh
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential pkg-config \
  libx11-dev libx11-xcb-dev libxcb1-dev \
  libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev \
  libfontconfig1-dev libfreetype6-dev \
  libxrandr-dev libxi-dev libxcursor-dev libxinerama-dev \
  libxrender-dev libxfixes-dev libxext-dev libxdamage-dev \
  libegl1-mesa-dev libgl1-mesa-dev \
  libudev-dev
```

### Run the app for development

```sh
cargo run
```

For an optimized build:

```sh
cargo build --release
./target/release/r-wg
```

## Recommended desktop setup

### Linux

1. build and launch the app,
2. open **Preferences -> System -> Privileged Backend**,
3. click **Install**,
4. import a config,
5. save it,
6. click **On**.

You do not need to wrap the app in helper scripts, run the UI with `sudo`, or manually apply `setcap` for the normal desktop path.

### Windows

1. build and launch the app,
2. open **Preferences -> System -> Privileged Backend**,
3. click **Install**,
4. approve the elevation prompt,
5. import a config and connect.

After the backend is installed, normal use stays inside the desktop UI.

## Manual Linux installation (advanced)

The in-app install flow is the recommended path.

Manual installation still exists for packaging, debugging, or administrator-managed deployment.

```sh
cargo build --release
sudo install -Dm755 target/release/r-wg /usr/local/libexec/r-wg/r-wg
sudo groupadd --system r-wg 2>/dev/null || true
sudo usermod -aG r-wg "$USER"
sudo install -Dm644 resources/linux/r-wg.desktop /usr/share/applications/r-wg.desktop
sudo install -Dm644 resources/icons/r-wg.svg /usr/share/icons/hicolor/scalable/apps/r-wg.svg
sudo install -Dm644 resources/icons/hicolor/256x256/apps/r-wg.png /usr/share/icons/hicolor/256x256/apps/r-wg.png
sudo install -Dm644 resources/linux/r-wg.service /etc/systemd/system/r-wg.service
sudo install -Dm644 resources/linux/r-wg.socket /etc/systemd/system/r-wg.socket
sudo install -Dm644 resources/linux/r-wg-repair.service /etc/systemd/system/r-wg-repair.service
sudo systemctl daemon-reload
sudo systemctl enable --now r-wg.socket
sudo systemctl enable r-wg-repair.service
```

After adding your user to the `r-wg` group, start a new login session before launching the UI.

The Linux backend starts on demand when the UI needs it and exits again after it becomes idle.

If a previous backend crash left recovery data behind, `r-wg-repair.service` can restore DNS state and clean stale Linux routing residue during boot before the UI is launched.

The recovery journal is stored at:

```text
/var/lib/r-wg/recovery.json
```

## Inspecting the backend on Linux

```sh
journalctl -u r-wg.socket -f
journalctl -u r-wg.service -f
journalctl -u r-wg-repair.service -f
```

For more logging controls and scopes, see `docs/logging.md`.

## Repair and removal

The app exposes backend lifecycle actions directly in **Preferences -> System -> Privileged Backend**:

- **Install**
- **Repair**
- **Remove**
- **Copy Diagnostics**

That is the preferred place to manage backend state unless you are packaging or debugging at the system level.

## Project layout

- `src/backend/wg` — WireGuard engine, service integration, IPC, route planning
- `src/platform/linux` — Linux network apply/cleanup and recovery behavior
- `src/platform/windows` — Windows network apply/cleanup and integration code
- `src/ui` — desktop UI state, actions, views, tray, and persistence
- `assets/themes` — built-in theme definitions
- `resources/linux` — desktop entry and `systemd` unit files
- `resources/windows` — Windows icon/resource files
- `scripts/` — platform-specific diagnostics and helpers

## Architecture notes

- UI: GPUI
- backend engine: `gotatun`
- logging: `tracing` with a UI ring buffer plus optional stderr output
- Linux privilege model: `systemd` + Unix socket backend
- Windows privilege model: SCM-managed Windows service

## Development notes

- CI builds on Linux and Windows
- release packaging currently targets Linux (`tar.gz`) and Windows (`zip`)
- the temporary local `ashpd` patch is no longer required; the project uses the crates.io release directly

## License

Unless noted otherwise, the `r-wg` project is licensed under the Apache License 2.0. See `LICENSE`.

## In one sentence

r-wg is meant to feel like a real desktop WireGuard client: install the backend once, choose a tunnel, click on, and let the app handle the privileged networking work behind the scenes.
