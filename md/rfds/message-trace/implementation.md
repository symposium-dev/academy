# Implementation plan and status

### Step 1: Trace storage layer (insert + query)

**Type**: New functionality — needs tests.

Add the `traces` table to the SQLite schema and expose `Store::insert_trace` and `Store::query_traces` as a single testable unit. The query API supports filtering by session, time range, and cursor-based pagination (for live tailing).

**Test (red first)**: Insert several traces via `store.insert_trace(...)` with different sessions and timestamps, then call `store.query_traces(...)` with various filters and assert correct results. This tests the public `Store` interface — no internal DB details leak.

- [x] Add `CREATE TABLE traces` + indexes to `configure_sqlite_file`
- [x] Schema migration from v1 → v2
- [x] `TraceRecord` struct (the public query result type)
- [x] `Store::record_trace(...)` 
- [x] `Store::traces(TraceQuery) -> Vec<TraceRecord>`
- [x] `Store::delete_traces_for_session(session_id)` — called from existing `remove_session`
- [x] Tests: insert then query with each filter dimension; assert no-op when session deleted

### Step 2: `TraceRecorder` with config gating

**Type**: New functionality — needs tests.

A struct that wraps a `Store` and a `trace_enabled` flag. Provides semantic methods (`record_message`, `record_event`) that call `store.insert_trace`. No-ops when disabled. Also add the `trace` field to `DaemonConfig`.

**Test (red first)**: Create a recorder with an in-memory store, call `record_message(...)` and `record_event(...)`, then use `store.query_traces(...)` to verify the rows. Also test that a disabled recorder produces no rows. All tests go through the public `Store::query_traces` interface — no private state inspection.

- [x] Add `trace` field to `DaemonConfig` + `Config::trace_enabled()` accessor
- [x] Dispatcher trace helpers supersede `TraceRecorder::new(store, enabled)`
- [x] Dispatcher trace helpers record messages with `dir`, `role`, `session_id`, `kind`, `method`, `request_id`, and `payload`
- [x] Dispatcher trace helpers record events with `role`, `session_id`, event name, and payload
- [x] Tests: record → query → assert; disabled → query → assert empty

### Step 3: Extract trace metadata from `Dispatch`

**Type**: New functionality — needs test.

A helper `fn trace_metadata(dispatch: &Dispatch) -> (kind, method, request_id, payload)` that borrows a dispatch and extracts the fields needed for recording.

The `UntypedMessage` inside Request/Notification has public `method` and `params` fields. For Response, serialize the Result. The Responder isn't touched (only borrowed for its id).

**Test (red first)**: Construct each `Dispatch` variant, call `trace_metadata`, assert correct extraction. This is a pure function test — no DB, no dispatcher.

- [x] `trace_metadata(&Dispatch) -> TraceFields` 
- [x] Integration tests cover request, notification, and response trace extraction

### Step 4: Wire tracing into dispatcher — messages

**Type**: New functionality (additive, no behavior change to existing flows).

Add a `TraceRecorder` to the `Dispatcher`. At each message routing point, call `recorder.record_message(...)`. Thread `trace_enabled` from config through `Daemon` → `Dispatcher` → `TraceRecorder`.

**Prerequisite**: Expose a `Store` clone from `TestDaemon` so integration tests can query traces. Add `pub fn store(&self) -> Store` to `TestDaemon` (stash a clone during construction before passing it to the daemon).

**Test (red first)**: Integration test using `TestDaemon` — start daemon with tracing enabled, send a prompt via rhaicp, then call `daemon.store().query_traces(session_id)`. Assert we see the prompt flow: `client_to_daemon` request (`session/prompt`), `daemon_to_agent` request, `agent_to_daemon` notification(s), `daemon_to_client` notification(s), `agent_to_daemon` response, `daemon_to_client` response. (Note: `session/new` response is *not* asserted here — that requires the wrapped responder from Step 6.)

- [x] Tests query traces by reopening `Store` at the daemon database path
- [x] Dispatcher owns trace helper methods instead of a separate `TraceRecorder`
- [x] Thread config through `Daemon::with_trace_enabled` → `Dispatcher::new`
- [x] Trace in `handle_from_client` (client_to_daemon)
- [x] Trace in `route_to_agent` (daemon_to_agent)
- [x] Trace in `handle_from_agent` (agent_to_daemon)
- [x] Trace when forwarding to client's `outgoing_tx` (daemon_to_client)
- [x] Integration test asserting prompt trace row sequence

### Step 5: Wire tracing into dispatcher — lifecycle events

**Type**: New functionality — needs test.

Record events for connect/disconnect/spawn/quiescent/kill/session lifecycle.

**Test (red first)**: Integration test — start `TestDaemon` with tracing, create a session, disconnect client, wait for quiescent. Query traces and assert event rows appear in order: `client_connected`, `session_created`, `agent_spawned`, `client_disconnected`, `agent_quiescent`.

- [x] Trace `client_connected` in `ClientRegistered` handler
- [x] Trace `client_disconnected` in `handle_client_disconnected`
- [x] Trace `agent_spawned` in `handle_agent_ready`
- [x] Trace `agent_quiescent` in `handle_agent_quiescent`
- [x] Trace `agent_killed_idle` in `handle_idle_timeout`
- [x] Trace `session_created` / `session_loaded` / `session_resumed`
- [x] Add `DispatcherMessage::ModelSet` variant, trace `model_set` in actor loop

### Step 6: Wrapped responder for response capture

**Type**: New functionality — needs test.

When tracing is enabled, wrap incoming `Dispatch::Request` responders using `Responder::wrap_params` *before* passing into `MatchDispatch`. The closure synchronously sends `DispatcherMessage::ResponseSent` to the actor loop (unbounded channel send is non-blocking), then passes the response value through unchanged so original wire delivery still happens immediately.

Since dispatches in the dispatcher are `Dispatch<UntypedMessage, UntypedMessage>` with `Responder<serde_json::Value>`, there are no generics issues — the wrapping happens at the type-erased level before `MatchDispatch` downcasts to typed handlers.

**Test (red first)**: Integration test — send `session/new` to a traced daemon, query traces via `daemon.store().query_traces(session_id)`, assert there's a `daemon_to_client` response row for `session/new` with a `request_id` matching the corresponding request row.

- [x] Add `DispatcherMessage::ResponseSent { method, request_id, session_id, payload }`
- [x] In `handle_from_client`, wrap typed local responders when tracing is enabled
- [x] Handle `ResponseSent` in actor loop: record `daemon_to_client` response trace row
- [x] Integration test: `session/new` request row + response row both present with matching `request_id`

### Step 7: `jamsession debug` subcommand — server skeleton

**Type**: New functionality — needs test.

Add the `Debug` variant to the CLI. Localhost server with `GET /` (embedded HTML) and `GET /api/traces?session=...&after_id=...&since=...`.

**Test (red first)**: Start the server programmatically in a test, insert some traces into the store, fetch `/api/traces?session=X`, assert the JSON response matches what `store.query_traces` would return. This tests the HTTP layer as a public interface without mocking.

- [x] Add `Debug` command with `serve` and `dump` subcommands plus `--port`, `--session-id`, `--since`, `--today`, `--ago` args
- [x] Time parsing via existing `chrono` dependency
- [x] Localhost server: `GET /` serves embedded HTML, `GET /api/traces` queries store
- [x] Integration test: insert traces → HTTP GET → assert JSON

### Step 8: Debug viewer HTML/JS

**Type**: New functionality — manual verification (visual).

Embedded single-page app. Polls `/api/traces?after_id=N` every 200ms for live tailing.

No automated tests — verify by running `jamsession debug serve` against a real or test session.

- [x] `viewer.html` with inline CSS/JS
- [x] SVG swimlane rendering (client / daemon / agent columns)
- [x] Rainbow request/response correlation
- [x] Expandable payload panel on click
- [x] Filter controls (session picker, method filter)
- [x] Live tail toggle

### Step 9: Use it to debug the empty-response issue

**Type**: Investigation — no code changes expected.

Run the live agent test with `trace = true`, open the viewer, identify where the response text goes missing.

- [x] Run test, capture trace
- [x] Document findings

## Implementation progress log

### 2026-07-01

Current production slice:

- Added the `traces` table and indexes in the existing SQLite database setup.
- Added `Store::record_trace`, `Store::traces`, `TraceQuery`, and `TraceRecord`.
- Added `trace = true` to `[daemon]` config and threaded it through `Daemon` into the dispatcher.
- Recorded opt-in trace rows for client/agent dispatch flow, local daemon responses, and lifecycle events.
- The debug CLI provides `serve` and `dump` subcommands with `--session-id`, `--since`, `--today`, and `--ago`.
- Added a localhost-only static HTML/JS viewer that polls `/api/traces` every 200ms and supports session/method/direction filters.
- Documented `trace` and `jamsession debug` in the user and design guides.

Deviations and surprises:

- Trace storage is schema version `2`; opening a v1 database creates the trace table and updates `schema_version`.
- I did not introduce a separate `TraceRecorder` type. The dispatcher now has small trace helper methods over `Store`, which kept the change narrower.
- `Responder::wrap_params` is available, but by the time the dispatcher uses `MatchDispatch`, responders are typed. The implementation wraps the typed local responders in each local request arm rather than doing one type-erased pre-match transform.
- Successful response payloads are serialized via `JsonRpcResponse::into_json`. Error response traces preserve a structured `{ "error": "..." }` string because the SDK does not expose a stable public JSON representation for errors at this interception point.
- The debug server is dependency-free over `tokio::net::TcpListener`, not Axum. That avoided a new direct web dependency for two simple routes.
- Time parsing uses existing `chrono` instead of adding `jiff`.
- The viewer now includes SVG swimlanes, colored request/response correlation, clickable row highlighting, expandable payloads, and a live-tail toggle.
- `model_set` is traced through the actor loop with `DispatcherMessage::ModelSet`.

Review follow-ups:

- Added trace coverage for alive `session/load` and `session/resume`, not just respawned load/resume.
- Removed the cwd-cleanup `session_deleted` trace row so deleted sessions do not retain trace rows.
- Replaced viewer `innerHTML` rendering with DOM construction and `textContent` for trace payloads.

Verification so far:

- `cargo fmt --check`
- `cargo clippy --all --workspace`
- `cargo test --all --workspace`
- `mdbook build`

Follow-up slice:

- Added `DispatcherMessage::ModelSet` so successful model configuration changes are recorded as actor-loop trace events.
- Expanded the debug viewer from a table into SVG swimlanes with colored request/response correlation, clickable row highlighting, expandable payloads, and a live-tail toggle.
- Added an HTTP-level test for `/api/traces?session=...`.

Live-agent investigation:

- Ran `cargo test -p jamsession-test live_agent_responds_to_prompt -- --ignored --nocapture` with trace enabled.
- The first run failed during startup guideline delivery because the live Claude ACP agent returned `Authentication required`; no user prompt reached the daemon.
- Reran with guideline injection disabled for this ignored diagnostic test. The trace captured `session/new`, `model_set`, `session_created`, an agent `session/update`, the user `session/prompt`, daemon forwarding to the agent, an agent error response, daemon forwarding the error to the client, a usage update, and client disconnect.
- The empty-response issue could not be reproduced because the live agent rejected the prompt with `Authentication required`.
- The trace did reveal a correlation caveat: ACP proxying sends daemon-to-agent requests through `send_request_to`, which assigns a fresh SDK request ID for the agent leg. As a result, the client-side `session/prompt` request ID and the agent-side response request ID differ in the trace. The response still routes correctly, but strict end-to-end request ID equality is not available at the dispatcher interception points.
