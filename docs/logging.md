# Logging

r-wg uses `tracing` plus a small custom layer in `src/log.rs`.

The logging path is:

```text
event helpers / tracing macros
        -> global tracing subscriber
        -> app target filter
        -> scope filter
        -> sinks
```

There are two sinks today:

- A per-process ring buffer used by the UI Logs page and backend `LogSnapshot`.
- Optional stderr output for local debugging.

## Initialization

`log::init()` installs the global subscriber once per process.

- UI mode initializes logging in `main.rs` before single-instance startup.
- Linux privileged service mode initializes logging in `linux_service::entry`.
- Windows service/manage mode initializes logging in `windows_service::maybe_run_service_mode`.

If another subscriber is already installed, `log::event()` falls back to direct buffer/stderr writes
for event-helper calls. Plain `tracing::*` calls require the subscriber path and are not covered by
that fallback.

## Environment

These variables are read once during `log::init()` in each process.

| Variable | Default | Effect |
| --- | --- | --- |
| `RWG_LOG` | off | Enables stderr output for the current process. Accepted values: `1/0`, `true/false`, `yes/no`, `on/off`. Unknown values are treated as enabled when the variable is present. |
| `RWG_LOG_LEVEL` | `info` | Minimum level. Accepted values: `error`, `warn`, `info`, `debug`, `trace`, or `1..5`. Unknown values fall back to `info`. |
| `RWG_LOG_SCOPES` | all | Comma-separated scope allow-list. Matching is case-insensitive. Empty, `*`, or `all` means all scopes. |
| `RWG_LOG_BUFFER` | on | Enables the in-process ring buffer. This is independent from stderr. |

`RWG_LOG_BUFFER` is per-process. Setting it for the UI process does not change the privileged
backend process unless the backend is started with the same environment.

## Scopes

Known scopes are:

```text
app
ui
engine
net
dns
stats
ipc
service
```

Event helpers in `src/log/events/*` pass an explicit scope.

Plain `tracing::*` calls under the app namespace are also captured. When they do not provide a
`scope` field, `src/log.rs` maps their target:

| Target | Scope |
| --- | --- |
| `r_wg` | `app` |
| `r_wg::ui::*` | `ui` |
| `r_wg::backend::wg::ipc*` | `ipc` |
| `r_wg::backend::wg::linux_service::client*` | `ipc` |
| `r_wg::backend::wg::windows_pipe*` | `ipc` |
| `r_wg::backend::wg::linux_service::*` | `service` |
| `r_wg::backend::wg::windows_service*` | `service` |
| `r_wg::backend::*` | `engine` |
| `r_wg::application::*` | `engine` |
| `r_wg::core::route_plan::*` | `engine` |
| `r_wg::platform::*` | `net` |
| `r_wg::dns::*` | `dns` |
| `r_wg::core::dns::*` | `dns` |
| `r_wg::log::*` | `app` |
| other `r_wg::*` | `app` |

Third-party targets are ignored by the custom sinks. There is no current environment variable that
enables third-party logs in the UI buffer or stderr sink.

## Format

Every buffered/stderr line is formatted as:

```text
[YYYY-MM-DD HH:MM:SS][r-wg][scope] message
```

Timestamps use local time and have second precision.

The formatter uses the `message` field when present. Other structured fields are appended as
`name=value` only when there is no message field.

## Buffers

Each process owns one lock-free `ArrayQueue<String>` ring buffer with capacity `2000`.

- New lines beyond capacity evict old lines.
- `log::snapshot()` returns a best-effort snapshot by temporarily popping and re-pushing lines.
- `log::clear()` clears the current process buffer.
- `log::set_buffer_enabled(false)` stops new buffer writes but does not delete existing lines.

The UI merged view also caps output at `2000` lines after combining local and backend logs.

Large individual log messages can still make rendering or copying expensive. Avoid dumping large
command output or multi-megabyte payloads into one log line.

## Logs Page

The Logs page reads:

```text
UI process buffer
+ privileged backend process buffer
+ backend sync error line, when sync fails
```

Backend lines are retrieved through IPC:

- `LogSnapshot` returns `Vec<String>` from the backend process buffer.
- `LogClear` clears the backend process buffer.

The UI behavior is:

- Opening the Logs page starts backend polling.
- Polling happens at most once every two seconds.
- Leaving the Logs page stops active polling.
- Clear clears the UI buffer, current display, backend buffer, and then requests a fresh backend
  snapshot.
- Merge order is string sort over the rendered log line. This works with the current timestamp
  prefix but is not a structured timestamp sort.
- Duplicate rendered lines are removed.

## Preferences

Preferences -> Monitoring -> Logs contains two UI preferences:

| Preference | Default | Effect |
| --- | --- | --- |
| Enable Log Viewer | on | Enables the UI process log buffer and backend log syncing while the Logs page is open. Turning it off stops UI buffer writes, stops polling, clears cached backend lines, disables Copy/Clear in the Logs page, and shows a disabled notice. |
| Auto Follow Logs | on | When the Logs page is enabled and the cursor is already at the end, new merged text scrolls to the latest line. |

`Enable Log Viewer` does not send a command to disable backend logging. The privileged backend keeps
its own process buffer unless that process was started with `RWG_LOG_BUFFER=0`.

## Code Guidelines

Prefer event helpers for user-visible or diagnostic lifecycle events:

```rust
r_wg::log::events::ui::tunnel_start_requested(name);
r_wg::log::events::engine::tunnel_start_failed(&err);
```

Current event modules:

```text
app
ui
engine
net
dns
stats
ipc
service
```

Direct `tracing::*` is acceptable for low-level fallback errors, temporary debug detail, or local
wrapper internals. It will be captured only when the target is `r_wg` or `r_wg::*`.

For expensive formatting, guard it:

```rust
if r_wg::log::enabled_for(r_wg::log::LogLevel::Debug, "net") {
    r_wg::log::event(
        r_wg::log::LogLevel::Debug,
        "net",
        format_args!("expensive value: {}", build_debug_value()),
    );
}
```

The exported macros (`log_info!`, `log_warn!`, `log_error!`, `log_debug!`, `log_trace!`) already
call `enabled_for`.

## Examples

Show app logs on stderr while keeping the UI buffer enabled:

```sh
RWG_LOG=1 RWG_LOG_LEVEL=debug r-wg
```

Show only network and engine scopes:

```sh
RWG_LOG=1 RWG_LOG_LEVEL=debug RWG_LOG_SCOPES=net,engine r-wg
```

Start with no in-process ring buffer writes:

```sh
RWG_LOG_BUFFER=0 r-wg
```

Inspect Linux service logs outside the UI:

```sh
journalctl -u r-wg.service -f
```

## Important Compatibility Note

Backend log IPC was added in protocol v12. After upgrading from an older build, restart or repair
the privileged backend service so the UI and backend use the same protocol version.
