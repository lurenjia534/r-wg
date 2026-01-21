# r-wg

r-wg is a Rust-based WireGuard client with a GPUI front end and a Rust backend (gotatun).

## Status

- Linux: TUN device + address/route configuration via netlink, DNS via resolvectl/resolvconf.
- macOS/Windows: scaffolding only (network configuration is a placeholder).

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

If you build manually, set the capability on the binary:

```sh
cargo build
sudo setcap cap_net_admin+ep target/debug/r-wg
./target/debug/r-wg
```

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
- `src/ui.rs`: GPUI UI and tunnel management.
- `scripts/`: OS-specific helpers (`scripts/linux`, `scripts/windows`).

## Dependency Note

We currently patch `ashpd 0.11.0` locally because `zvariant >= 5.9` requires dict keys to implement
`Basic`, which `ashpd 0.11.0` does not. The patch is applied through `[patch.crates-io]` in
`Cargo.toml` and lives in `vendor/ashpd`. When upstream fixes the mismatch (for example via newer
`gpui` or `ashpd`), remove `vendor/ashpd` and the patch block, then run `cargo update`.
