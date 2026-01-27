# Logging

r-wg uses `tracing` with a custom subscriber layer to keep logging centralized and eventized.
All message text lives under `src/log/events/*`, and call sites only trigger event functions.

## Environment variables

- `RWG_LOG`: global switch (default: off). Accepts `1/0`, `true/false`, `yes/no`, `on/off`.
- `RWG_LOG_LEVEL`: `error|warn|info|debug|trace` (or `1..5`). Default: `info`.
- `RWG_LOG_SCOPES`: comma-separated scopes (e.g. `net,engine,dns`).
  - If unset, all scopes are enabled.
  - `*` or `all` enables all scopes.
- `RWG_LOG_BUFFER`: enable UI ring buffer (default: on when `RWG_LOG=1`).

Note: `RWG_LOG` is the master switch. If `RWG_LOG` is off, both stderr output and the buffer are disabled,
regardless of `RWG_LOG_BUFFER`.

## Outputs

Two sinks are registered:

- **Stderr sink** for developer debugging (controlled by `RWG_LOG`).
- **Ring buffer sink** for UI (controlled by `RWG_LOG_BUFFER`).

The ring buffer uses a lock-free `ArrayQueue` with a capacity of 2000 lines. UI reads it via
`log::snapshot()` and clears via `log::clear()`.

## Log format

```
[YYYY-MM-DD HH:MM:SS][r-wg][scope] message
```

`scope` is provided by the event functions (e.g. `net`, `engine`, `dns`, `stats`, `ui`).

## Usage in code

- Add new message text in `src/log/events/*`.
- Call event functions from feature code; avoid ad-hoc logging at call sites.
- Use `log::enabled_for(level, scope)` if expensive formatting is needed, or rely on
  `log_info!` / `log_debug!` macros (they already gate on `enabled_for`).

## Example

```sh
RWG_LOG=1 RWG_LOG_LEVEL=debug RWG_LOG_SCOPES=net,engine \
  scripts/linux/run_with_cap.sh
```
