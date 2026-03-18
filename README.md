# r-wg

r-wg is a Rust-based WireGuard client with a GPUI front end and a Rust backend (gotatun).

## Status

- Linux: TUN device + address/route configuration via netlink, DNS via resolvectl/resolvconf.
- Windows: network configuration implementation (addresses/routes/DNS/metrics + endpoint bypass).
- macOS: placeholder only (network configuration not implemented).

## Features

- Parse standard WireGuard `.conf` plus wg-quick style fields (Address, DNS, MTU, Table).
- UI import or paste config, select a tunnel, start/stop.
- Peer stats (handshake time and traffic counters) from the backend.

## Requirements

- Rust toolchain (rustup).
- Linux: `systemd` for the privileged backend service.
- Optional: `resolvectl` or `resolvconf` for DNS changes.

## Build and Run

### Linux (socket-activated privileged backend)

Recommended path for normal desktop use:

- Launch `r-wg` directly.
- Open `Advanced -> Privileged Backend`.
- Click `Install`.
- After installation finishes, keep launching the app normally; the privileged backend will be started automatically when a tunnel needs it.

You do not need to wrap the app in a shell helper, run the UI with `sudo`, or manually apply `setcap`.

Manual `systemd` installation is still available for packaging, debugging, or administrator-managed deployments:

```sh
cargo build
sudo install -Dm755 target/debug/r-wg /usr/local/libexec/r-wg/r-wg
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
r-wg
```

After adding your user to the `r-wg` group, start a new login session before launching the UI.

The backend service now starts on demand when the UI connects and exits again after it becomes idle.
If the previous backend crashed and left a recovery journal behind, `r-wg-repair.service`
will run once during boot to restore DNS state and clean stale Linux routing residue before the
user launches the UI.

The recovery journal is stored under `/var/lib/r-wg/recovery.json` via `StateDirectory=r-wg`.

To inspect the backend:

```sh
journalctl -u r-wg.socket -f
journalctl -u r-wg.service -f
journalctl -u r-wg-repair.service -f
```

For more options (levels, scopes, buffer), see `docs/logging.md`.

The app now expects a privileged backend instead of `setcap`. The Advanced page shows backend
status and supports `Install`, `Repair`, and `Remove` via `pkexec` for development builds.
That flow copies the current executable into `/usr/local/libexec/r-wg/r-wg` before enabling
`r-wg.socket`, so the root-managed backend does not point at your workspace binary.

### Release build

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
sudo systemctl restart r-wg.socket
sudo systemctl enable r-wg-repair.service
```

## Configuration Format

Example:

```
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
```

## Project Layout

- `src/backend/wg`: config parser and engine.
- `src/platform/linux`: Linux network apply/cleanup via netlink.
- `src/platform/windows`: Windows network apply/cleanup via IP Helper APIs.
- `src/ui.rs`: GPUI UI and tunnel management.
- `scripts/`: OS-specific helpers (`scripts/linux`, `scripts/windows`).

## Dependency Note

The temporary local `ashpd` patch is no longer needed. Upstream fixed the
`zvariant >= 5.9` dict-key `Basic` constraint mismatch in `ashpd 0.11.1`, so
this repository now uses the crates.io release directly.
