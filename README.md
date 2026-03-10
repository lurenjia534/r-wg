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
- Linux: `cap_net_admin` to configure TUN and routes (or run as root).
- Optional: `resolvectl` or `resolvconf` for DNS changes.

## Build and Run

### Linux (non-root with capabilities)

```sh
scripts/linux/build_with_cap.sh
scripts/linux/run_with_cap.sh
```

To run with logs:

```sh
RWG_LOG=1 scripts/linux/run_with_cap.sh
```

For more options (levels, scopes, buffer), see `docs/logging.md`.

If you build manually, set the capability on the binary:

```sh
cargo build
sudo setcap cap_net_admin+ep target/debug/r-wg
./target/debug/r-wg
```

Note: file capabilities are not preserved in release archives, so after downloading a release
you must run `sudo setcap cap_net_admin+ep r-wg` (or run the binary with `sudo`).

### Release build

```sh
scripts/linux/build_with_cap.sh --release
scripts/linux/run_with_cap.sh --release
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
