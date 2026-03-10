# Changelog

All notable changes to this project will be documented in this file.

## 0.2.6 - 2026-03-10

- Downgraded `gotatun` to `0.3.1` to keep Linux tunnels usable while the `0.4.0` full-tunnel regression remains unresolved.
- Let `zerocopy` and `zerocopy-derive` resolve back to `0.8.42` with the `gotatun 0.3.1` downgrade.
- Removed the temporary local `ashpd` patch and switched back to the upstream crates.io release.

## 0.2.5 - 2026-03-07

- Restored `gotatun` to `0.4.0` while pinning `zerocopy` and `zerocopy-derive` to `0.8.27`.
- Worked around the Linux full-tunnel regression reproduced with `gotatun 0.4.0` when `zerocopy >= 0.8.33`.
- Added troubleshooting notes and Linux diagnostic scripts for the `RX=0` / handshake-only tunnel failure mode.

## 0.2.4 - 2026-03-07

- Downgraded `gotatun` to `0.3.1` to avoid the Linux tunnel regression observed with `0.4.0`.

## 0.2.3 - 2026-03-06

- Added cross-platform tray controls, with Windows tray notifications for tunnel lifecycle changes.
- Improved proxy management with endpoint family tags and multi-select deletion confirmation.
- Hardened Windows full-tunnel DNS handling and updated Win32 integration for `windows 0.62.2`.
- Upgraded `gotatun` to `0.4.0` and documented the local `ashpd` patch workflow.
- Trim allocator state after tunnel stop on Linux to reduce lingering memory usage.

## 0.2.2 - 2026-02-02

- Version bump for release.

## 0.2.1 - 2026-01-25

- Added rolling traffic summary tracking (24h/30d) with per-config aggregates.
- Added Traffic Summary card to Overview (donut + upload/download + ranking).
- Fixed Overview page scrolling to allow full panel access.
