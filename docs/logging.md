# Logging

r-wg uses `tracing` with a custom subscriber layer to keep logging centralized and eventized.
All message text lives under `src/log/events/*`, and call sites only trigger event functions.

## Environment variables

- `RWG_LOG`: stderr switch (default: off). Accepts `1/0`, `true/false`, `yes/no`, `on/off`.
- `RWG_LOG_LEVEL`: `error|warn|info|debug|trace` (or `1..5`). Default: `info`.
- `RWG_LOG_SCOPES`: comma-separated scopes (e.g. `net,engine,dns`), case-insensitive.
  - If unset, all scopes are enabled.
  - `*` or `all` enables all scopes.
- `RWG_LOG_BUFFER`: enable UI ring buffer (default: on).

Note: `RWG_LOG` only controls stderr output. The UI buffer is controlled by `RWG_LOG_BUFFER`.
The UI also has a persisted **Enable Log Viewer** preference under Preferences -> Monitoring ->
Logs. Turning it off stops the UI log buffer and prevents the Logs page from syncing backend logs;
it does not disable the privileged backend service's own logging.

## Outputs

Two sinks are registered:

- **Stderr sink** for developer debugging (controlled by `RWG_LOG`).
- **Ring buffer sink** for UI (controlled by `RWG_LOG_BUFFER`).

Note: disabling `RWG_LOG_BUFFER` stops new writes, but `log::snapshot()` and `log::clear()` still
operate on the existing buffer contents.

The ring buffer uses a lock-free `ArrayQueue` with a capacity of 2000 lines. UI reads it via
`log::snapshot()` and clears via `log::clear()`.

## Log format

```
[YYYY-MM-DD HH:MM:SS][r-wg][scope] message
```

`scope` is provided by the event functions (e.g. `app`, `net`, `engine`, `dns`, `stats`, `ui`,
`ipc`, `service`). Plain `tracing::*` calls under `r_wg::*` are also captured and mapped by target:
`r_wg::ui::*` -> `ui`, `r_wg::backend::*`/`r_wg::application::*` -> `engine`,
`r_wg::platform::*` -> `net`, and `r_wg::dns::*` -> `dns`. Third-party targets are ignored by
default.

## Privileged backend logs

On Linux and Windows, the UI process and privileged backend process keep separate buffers. The Logs
page merges the local UI buffer with the backend buffer through IPC:

- `LogSnapshot` returns the backend process buffer.
- `LogClear` clears the backend process buffer.

The Logs page syncs immediately when opened, then polls at most once every two seconds while it is
visible. Leaving the page stops active polling.

## Usage in code

- Add new message text in `src/log/events/*`.
- Call event functions from feature code; avoid ad-hoc logging at call sites.
- Use `log::enabled_for(level, scope)` if expensive formatting is needed, or rely on
  `log_info!` / `log_debug!` macros (they already gate on `enabled_for`).

Current event modules cover app startup, UI actions, tunnel engine lifecycle, DNS, network apply,
runtime stats, IPC, and privileged service lifecycle. Direct `tracing::*` remains acceptable for
temporary debug detail and low-level fallback errors; it is still captured for `r_wg::*` targets.

## Example

```sh
RWG_LOG=1 RWG_LOG_LEVEL=debug RWG_LOG_SCOPES=net,engine r-wg
# or inspect the privileged backend directly:
journalctl -u r-wg.service -f
```
