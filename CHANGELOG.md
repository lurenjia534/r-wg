# Changelog

All notable changes to this project will be documented in this file.

## 0.3.0 - 2026-04-17

- Added a dedicated Tools workspace with CIDR exclusion/normalization and reachability audit flows, including saved-config audits, cancellation/progress snapshots, virtualized result lists, and host-side endpoint probing without extending privileged IPC.
- Added a Mullvad-only post-quantum tunnel upgrade path across the backend, privileged IPC, and desktop surfaces, introducing the new ephemeral-peer protobuf plumbing, Windows MTU support, and clearer UI copy around provider-specific availability.
- Introduced shared `core` and `application` service layers for config parsing, route planning, DNS, tunnel sessions, backend administration, diagnostics, and config-library workflows, reducing coupling between the backend engines and UI controllers.
- Completed a broad UI feature-slice migration across Configs, Route Map, Overview, Tools, Session, and Backend Admin, split commands by domain, removed dead shims, and restored regressions like direct button handlers and Route Map list layout during the transition.
- Rebuilt the Configs workspace around a cleaner responsive library/editor/inspector layout, extracted import/export and draft/storage flows into feature-local modules, and aligned the Overview and top bar chrome so the main workspaces share a more consistent structure.
- Added a connect-password gate before tunnel start, hardened password handling and persistence, and threaded the new session flow through the relevant commands, dialogs, and advanced preferences surfaces.
- Added the Apache 2.0 license, expanded module-level documentation/comments to match the refactored structure, and updated Linux CI/release runners to install `protoc` so protobuf-backed builds succeed in automation.

## 0.2.9 - 2026-03-25

- Rewrote the README around the app-first workflow, platform support, privileged-backend install/repair flow, and source-build guidance, so first-run setup and backend management are documented much more clearly on both Linux and Windows.
- Hardened Windows networking and service installation by copying `wintun.dll` alongside the installed service binary, relaxing endpoint bypass requirements to only require address families the host can actually route, and fixing staged apply abort accounting so failure reports stay aligned with the real progress that was reached.
- Improved Linux tunnel reliability by vendoring `gotatun 0.4.1` locally and applying the upstream IPv6 `recvmmsg` fix, while also tightening Linux policy-routing recovery and startup-repair structure.
- Fixed a long run of desktop UX regressions around single-instance startup and activation handoff, including Linux lock cleanup, control-pipe ownership, listener recovery, and Windows session-scoped activation so relaunch/focus behavior is more reliable.
- Restored several important UI behaviors across the shell and config workspace, including sidebar drawer/collapse recovery, responsive and compact Configs panes, Route Map scrolling/contrast, and access to running helper recovery actions from the UI.
- Broke the backend and UI into more focused modules across route planning, Linux service/recovery, Windows apply stages, themes, proxies, configs, overview data, and shared state boundaries, reducing code concentration ahead of the next round of feature work.
- Repaired the release workflow so GitHub release publishing resolves repository context correctly again after downloading artifacts.

## 0.2.8 - 2026-03-20

- Added a first-class route-map planning and apply pipeline: the backend now builds a shared route-plan truth model, Linux and Windows apply flows consume that plan directly, and the UI can render planned routes, applied results, explain views, graph steps, and richer inventory/inspector details from the same source.
- Expanded and polished the route-map experience with stronger layout/navigation, better data flow, clearer status presentation, and restored inspector scrolling so larger explain/result payloads remain usable.
- Reworked the Configs workspace around more local page ownership: draft/input handling was normalized, library rows now update incrementally, search fields are cached, re-entrant reads were removed, and the editor/render flow is more stable under import, save, and tunnel state changes.
- Refined the Overview experience with a more formal page shell, unified cards/chrome, and a much deeper traffic summary presentation, including a dedicated trend chart and clearer 24h / monthly hierarchy.
- Expanded theme and preferences support with semantic palette policy, palette preferences and preview flow, vendored upstream GPUI themes, tighter theme linting, and more reliable persistence/diagnostics behavior across the settings surfaces.
- Improved desktop integration and UX details by wiring the app icon into shell integration, hardening the Windows tray fallback path, and tightening settings/about copy, feedback, and diagnostics presentation.
- Hardened Linux behavior by removing the implicit privileged-backend socket-group fallback, so service installation/repair follows explicit configuration more predictably.
- Updated release/build hygiene for publishing, including the Node 24 release workflow refresh, broader Clippy cleanup across all targets/features, and a final round of route-plan/policy warning fixes before tagging.

## 0.2.7 - 2026-03-17

- Added a socket-activated privileged backend on Linux, along with lifecycle hardening, startup repair recovery, socket permission fixes, and PATH-independent DNS tool resolution.
- Moved the Windows privileged backend to an SCM-managed service, fixed tunnel-control pipe starvation, and shifted the full-tunnel DNS guard to WFP dynamic filters.
- Added desktop notification improvements across the tray flows, including Linux freedesktop notifications and Windows tray notification copy support.
- Redesigned the proxies management experience with a fixed gallery/grid layout, tighter metadata presentation, and more stable selection/filter behavior.
- Refined the About page into a viewport-aware release/status panel with clearer hierarchy, stronger icon contrast, and cleaner system diagnostics.
- Clarified the privileged backend install flow in the README and added Linux DNS resolution regression coverage.

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
