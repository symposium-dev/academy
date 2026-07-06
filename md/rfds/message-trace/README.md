# Message Trace & Debug Viewer

## TL;DR

- Record every ACP dispatch flowing through the daemon's central actor to a `traces` table in SQLite.
- Capture responses via wrapped responders routed back through the actor loop — giving canonical ordering.
- Serve an interactive sequence-diagram viewer via `jamsession debug` on localhost:3000.
- Opt-in via `trace = true` in config. Traces retained as long as session data.

## Motivation

When debugging daemon behavior (e.g., why a prompt returns empty text, why a `session/update` notification doesn't reach the client), we have no structured visibility into the messages flowing through the dispatcher. `RUST_LOG=debug` produces overwhelming, unstructured output that's hard to correlate across client/agent/session boundaries.

We need:
1. A machine-parsable trace of every dispatch flowing through the daemon's central actor.
2. A way to browse these traces after the fact (or live) in a human-friendly format.

## Change in a nutshell

Add a `traces` table to the existing `jamsession.db`. When `trace = true`, the dispatcher records every dispatch and lifecycle event as a row. A new `jamsession debug` subcommand serves a web-based sequence diagram viewer that queries this table.

The key design choice: responses to locally-handled requests (like `session/new`) are captured by wrapping the `Responder` — the wrapper routes the response back through the actor loop before delivering it on the wire. This gives a single canonical ordering for all trace events.

## Detailed plans

### Storage: SQLite

Traces are stored in the existing `jamsession.db` database alongside session data:
- Queryable (filter by session, method, direction, time range)
- Live tailing via poll (`SELECT ... WHERE rowid > ? LIMIT ...` on 200ms interval)
- Natural retention — traces live as long as session data. When a session is archived or deleted (via `session/delete`), its trace rows are removed too.
- No additional dependencies or file management

#### Schema

```sql
CREATE TABLE traces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts TEXT NOT NULL,                    -- ISO 8601 timestamp
    session_id TEXT,                     -- ACP session ID (NULL for unrouted)
    dir TEXT NOT NULL,                   -- client_to_daemon, daemon_to_agent, agent_to_daemon, daemon_to_client, internal
    role TEXT,                           -- source/target role: "acp-client", "github", "agent", etc.
    kind TEXT NOT NULL,                  -- request, response, notification, event
    method TEXT,                         -- JSON-RPC method name
    request_id TEXT,                     -- JSON-RPC request ID (from responder.id() / router.id())
    payload TEXT NOT NULL                -- full JSON-RPC params/result/error as JSON
);

CREATE INDEX idx_traces_session ON traces(session_id);
CREATE INDEX idx_traces_ts ON traces(ts);
```

#### Participants and roles

A session may have multiple concurrent clients (e.g., an ACP client in a terminal and a GitHub integration posting comments). Instead of opaque numeric IDs, each participant is identified by a **role** string:

- `"acp-client"` — a standard ACP client connected via the Unix socket
- `"github"` — the GitHub integration (future)
- `"agent"` — the agent process

Lifecycle events (connect, disconnect) are recorded as `kind: "event"` rows, so the trace shows when participants come and go without needing to track IDs across rows.

#### Record format

Each row corresponds to one dispatch or lifecycle event passing through the dispatcher:

| Field | Description |
|-------|-------------|
| `ts` | ISO 8601 timestamp with millisecond precision |
| `session_id` | ACP session ID (NULL if not yet associated) |
| `dir` | Direction: `client_to_daemon`, `daemon_to_agent`, `agent_to_daemon`, `daemon_to_client`, `internal` |
| `role` | The participant role (e.g., `"acp-client"`, `"github"`, `"agent"`) |
| `kind` | `request`, `response`, `notification`, or `event` (for lifecycle: connect, disconnect, model set, etc.) |
| `method` | JSON-RPC method name (e.g., `session/prompt`, `session/update`) or event name (e.g., `client_connected`, `client_disconnected`) |
| `request_id` | JSON-RPC request ID for correlating requests with responses (from `responder.id()` for requests, `router.id()` for responses) |
| `payload` | Full JSON-RPC params/result/error — no truncation |

### Trace entry kinds

There are two root categories of trace entries:

1. **Messages** — directed communication from one participant to another (`a → b`). These have a `dir` indicating source and destination.
2. **Events** — things that happen locally within a participant (no direction). These represent state changes, lifecycle transitions, or internal decisions.

Both use the same table row; the `kind` field discriminates:

#### Messages (`dir` is set)

| `kind` | Meaning | Example |
|--------|---------|---------|
| `request` | JSON-RPC request from source to destination | Client sends `session/prompt` to daemon |
| `response` | JSON-RPC response flowing back | Daemon returns prompt result to client |
| `notification` | JSON-RPC notification (no response expected) | Agent sends `session/update` to daemon |

A `Dispatch` in ACP is an enum covering all three — so wherever we intercept a dispatch, we record one row.

#### Events (`dir` is `internal`, no destination)

| `kind` | Meaning | Example |
|--------|---------|---------|
| `event` | Lifecycle or internal state change | `client_connected`, `client_disconnected`, `agent_spawned`, `agent_killed_idle`, `model_set`, `session_created` |

Events use `method` as the event name and `payload` for any relevant context (e.g., `{"model": "default"}` for `model_set`).

This is extensible — new event names can be added without schema changes. New message kinds (if ACP ever adds them) would just be new `kind` values.

### Response capture via wrapped responders

When the dispatcher handles a request itself (e.g., `session/new`, `initialize`, `session/list`), it calls `responder.respond(...)` directly. That response goes straight to the client via an internal oneshot channel — it never passes through the central actor loop, so it would be invisible to tracing.

The solution: **wrap the responder using `Responder::wrap_params`**. In the dispatcher, all dispatches arrive as `Dispatch<UntypedMessage, UntypedMessage>` with `Responder<serde_json::Value>` — the type erasure has already happened at the transport layer. Before passing the dispatch into `MatchDispatch`, if tracing is enabled and the dispatch is a `Request`, we wrap its responder:

```rust
if let Dispatch::Request(ref msg, responder) = dispatch {
    let tx = self.dispatcher_tx.clone();
    let method = msg.method.clone();
    let request_id = responder.id();
    let wrapped = responder.wrap_params(move |_method, result| {
        let payload = serialize_response(&result);
        let _ = tx.send(DispatcherMessage::ResponseSent { method, request_id, payload });
        result // pass through unchanged — original delivery still happens
    });
    dispatch = Dispatch::Request(msg, wrapped);
}
```

The `wrap_params` closure runs synchronously inside `respond_with_result`. Since `dispatcher_tx` is an unbounded channel, the send is non-blocking — no async needed. The response still delivers to the wire immediately; the closure just piggybacks a notification to the actor loop.

When `ResponseSent` arrives in the actor loop, it records the trace row. The row appears *after* the response is on the wire, but it's still in canonical order from the actor's perspective — the response trace appears after all the internal events that produced it.

Events from spawned tasks (like `model_set` inside `agent_pipe`) use the same pattern: they send a `DispatcherMessage` variant back to the actor loop, which records the trace.

### Trace points in the dispatcher

The trace is recorded at these points — all within the central actor loop:

| Point | Category | `dir` | `role` | What's captured |
|-------|----------|-------|--------|-----------------|
| `handle_from_client` receives dispatch | message | `client_to_daemon` | source role (e.g., `acp-client`) | All incoming client dispatches (requests, notifications, responses) |
| `route_to_agent` forwards dispatch | message | `daemon_to_agent` | `agent` | Dispatches forwarded to the agent |
| `handle_from_agent` receives dispatch | message | `agent_to_daemon` | `agent` | Agent notifications and responses flowing back |
| Forward to client's `outgoing_tx` | message | `daemon_to_client` | target role | Dispatches delivered to clients |
| `ResponseReady` arrives in actor loop | message | `daemon_to_client` | target role | Responses to locally-handled requests (session/new, initialize, etc.) |
| Client connects | event | `internal` | connecting role | `client_connected` |
| Client disconnects | event | `internal` | disconnecting role | `client_disconnected` |
| Agent spawned | event | `internal` | `agent` | `agent_spawned` |
| Agent quiescent | event | `internal` | `agent` | `agent_quiescent` |
| Agent killed (idle/crash) | event | `internal` | `agent` | `agent_killed_idle` / `agent_crashed` |
| Model set | event | `internal` | `daemon` | `model_set` |
| Session created/loaded/resumed | event | `internal` | `daemon` | `session_created` / `session_loaded` / `session_resumed` |

### Configuration

```toml
[daemon]
# Enable message tracing (default: false)
trace = true
```

Tracing is opt-in. When disabled, no trace rows are written to the database.

### Trace Inspection: `jamsession debug`

The debug command can serve a web page (localhost only) or dump trace data as JSON:

```
jamsession debug serve [--port 3000] [--session-id <id>] [--since <time>] [--today] [--ago <duration>]
jamsession debug dump [--session-id <id>] [--since <time>] [--today] [--ago <duration>] [--limit <limit>]
```

Time filters:
- `--since 2026-06-30T10:00:00` — absolute timestamp (parsed via `chrono`)
- `--today` — shorthand for midnight today
- `--ago 1h` — relative duration (e.g., `30m`, `2h`, `1d`)

The viewer shows:
- A timeline/sequence diagram of messages
- Live tailing (polls DB every 200ms for new rows)
- Filtering by session, method, direction
- Expandable payloads
- Color-coded by direction (client=blue, agent=green, internal=gray)
- Correlation: clicking a request highlights its response

Implementation:
- Static HTML + inline JS (no build step), served from an embedded `include_str!`
- JSON API endpoints for the viewer to query traces from the DB
- Default port: 3000
- Visual inspiration: `agent-client-protocol-trace-viewer` in the acp-rust-sdk repo — renders SVG sequence diagrams with rainbow-colored request/response pairs, timeline spans showing processing duration, delta times between events, and inline content previews for `session/update` notifications. Same dark-theme aesthetic (VS Code-like). We can reuse the same rendering approach (vanilla JS, SVG swimlanes) adapted for our schema.

### Example: Full session lifecycle

This walkthrough shows the trace rows generated for a typical session:
1. Client connects, creates a session, sends a prompt, gets a response
2. Client disconnects while agent is still alive
3. Agent finishes its turn and goes quiescent
4. Client reconnects and sends another prompt

Session ID: `sess-abc123`

#### Phase 1: Client connects and prompts

| id | ts | session_id | dir | role | kind | method | request_id | payload (abbreviated) |
|----|-----|-----------|-----|------|------|--------|----------------|----------------------|
| 1 | ...000 | NULL | internal | acp-client | event | client_connected | NULL | `{}` |
| 2 | ...001 | NULL | client_to_daemon | acp-client | request | session/new | req-1 | `{"cwd": "/home/user/project"}` |
| 3 | ...002 | sess-abc123 | internal | daemon | event | session_created | NULL | `{"session_id": "sess-abc123"}` |
| 4 | ...003 | sess-abc123 | internal | agent | event | agent_spawned | NULL | `{}` |
| 5 | ...004 | sess-abc123 | internal | daemon | event | model_set | NULL | `{"from": "claude-opus-4-8", "to": "default"}` |
| 6 | ...050 | sess-abc123 | daemon_to_client | acp-client | response | session/new | req-1 | `{"sessionId": "sess-abc123"}` |
| 7 | ...051 | sess-abc123 | client_to_daemon | acp-client | request | session/prompt | req-2 | `{"prompt": [{"type": "text", "text": "Hello!"}]}` |
| 8 | ...052 | sess-abc123 | daemon_to_agent | agent | request | session/prompt | req-2 | `{"prompt": [{"type": "text", "text": "Hello!"}]}` |
| 9 | ...100 | sess-abc123 | agent_to_daemon | agent | notification | session/update | NULL | `{"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Hi there!"}}` |
| 10 | ...100 | sess-abc123 | daemon_to_client | acp-client | notification | session/update | NULL | `{"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Hi there!"}}` |
| 11 | ...200 | sess-abc123 | agent_to_daemon | agent | response | session/prompt | req-2 | `{"result": null}` |
| 12 | ...200 | sess-abc123 | daemon_to_client | acp-client | response | session/prompt | req-2 | `{"result": null}` |

#### Phase 2: Client disconnects, agent continues working

The client closes its socket. The agent is still alive (maybe doing background work or awaiting the next prompt).

| id | ts | session_id | dir | role | kind | method | request_id | payload (abbreviated) |
|----|-----|-----------|-----|------|------|--------|----------------|----------------------|
| 13 | ...300 | sess-abc123 | internal | acp-client | event | client_disconnected | NULL | `{}` |

Note: if the agent sends notifications after the client disconnects, they're still recorded (they go into the buffer) but there's no `daemon_to_client` row since no client is connected:

| id | ts | session_id | dir | role | kind | method | request_id | payload (abbreviated) |
|----|-----|-----------|-----|------|------|--------|----------------|----------------------|
| 14 | ...350 | sess-abc123 | agent_to_daemon | agent | notification | session/update | NULL | `{"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "(still thinking...)"}}` |

#### Phase 3: Agent goes quiescent

After the quiescence timeout (10s of silence), the daemon marks the agent quiescent. The idle timer starts.

| id | ts | session_id | dir | role | kind | method | request_id | payload (abbreviated) |
|----|-----|-----------|-----|------|------|--------|----------------|----------------------|
| 15 | ...10300 | sess-abc123 | internal | agent | event | agent_quiescent | NULL | `{}` |

#### Phase 4: Client reconnects and sends another prompt

A new client connects and resumes the session.

| id | ts | session_id | dir | role | kind | method | request_id | payload (abbreviated) |
|----|-----|-----------|-----|------|------|--------|----------------|----------------------|
| 16 | ...15000 | NULL | internal | acp-client | event | client_connected | NULL | `{}` |
| 17 | ...15001 | sess-abc123 | client_to_daemon | acp-client | request | session/resume | req-3 | `{"sessionId": "sess-abc123"}` |
| 18 | ...15002 | sess-abc123 | daemon_to_client | acp-client | response | session/resume | req-3 | `{}` |
| 19 | ...15010 | sess-abc123 | client_to_daemon | acp-client | request | session/prompt | req-4 | `{"prompt": [{"type": "text", "text": "What were you thinking about?"}]}` |
| 20 | ...15011 | sess-abc123 | daemon_to_agent | agent | request | session/prompt | req-4 | `{"prompt": [{"type": "text", "text": "What were you thinking about?"}]}` |
| 21 | ...15100 | sess-abc123 | agent_to_daemon | agent | notification | session/update | NULL | `{"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "I was considering..."}}` |
| 22 | ...15100 | sess-abc123 | daemon_to_client | acp-client | notification | session/update | NULL | `{"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "I was considering..."}}` |
| 23 | ...15200 | sess-abc123 | agent_to_daemon | agent | response | session/prompt | req-4 | `{"result": null}` |
| 24 | ...15200 | sess-abc123 | daemon_to_client | acp-client | response | session/prompt | req-4 | `{"result": null}` |

#### Key observations

- **Request/response correlation**: `request_id` lets you pair row 7 (client sends prompt) → row 8 (daemon forwards to agent) → row 11 (agent responds) → row 12 (daemon forwards to client).
- **Disconnected client**: Row 14 shows the agent sending a notification with no corresponding `daemon_to_client` row — the viewer can highlight this as "delivered to buffer only."
- **Session association**: Rows 1–2 have `session_id = NULL` because the session doesn't exist yet. Everything after row 3 is tagged with the session.
- **Extensibility**: Adding a new event (e.g., `idle_timer_started`) is just a new row with `kind = "event"` and a new method name. No schema migration needed.

## Frequently asked questions

### Why SQLite instead of JSONL files?

Queryability (filter by session/method/time without parsing), live tailing via poll, atomic writes, and natural retention tied to session lifetime. We already have SQLite for session persistence — one fewer dependency.

### Why route responses through the actor loop?

Responses to locally-handled requests (`session/new`, `initialize`, etc.) go directly from the dispatcher to the wire via `Responder::respond()`. Without wrapping, they'd be invisible to tracing. Routing through the actor loop gives us a single canonical ordering for all trace events — the row `id` order reflects causality. The latency cost (one mpsc hop) is negligible.

### Why not always-on?

Full payload recording for every dispatch can generate significant data. Opt-in keeps the default lean. In the future we could add a "lite" always-on mode that records only method/direction/timestamp without payloads.

### Why roles instead of numeric IDs?

Numeric IDs are opaque — they don't tell you what kind of participant sent a message. Roles give semantic meaning (`"acp-client"` vs `"github"` vs `"agent"`) and are sufficient when combined with lifecycle events (connect/disconnect) to reconstruct the timeline. If we later need to distinguish multiple concurrent clients of the same role, we can add an optional instance qualifier (e.g., `"acp-client:2"`).

## Implementation

See [Implementation plan and status](./implementation.md).
