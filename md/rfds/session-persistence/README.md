# Session persistence with SQLite

## TL;DR

- Replace the in-memory message buffer and `state.json` with a SQLite database using [toasty](https://crates.io/crates/toasty).
- Session metadata and conversation history survive daemon restarts.
- The message buffer is no longer cleared when an agent disconnects — history is durable.

## Motivation

The daemon currently holds session conversation history in an in-memory `Vec<serde_json::Value>`. This has two problems:

1. **Buffer cleared on agent disconnect.** When the agent is killed (idle timeout, crash), `handle_agent_disconnected` clears the buffer. A subsequent `session/load` from a client finds nothing to replay.

2. **No persistence across daemon restarts.** If the daemon itself restarts, all conversation history is lost. The only persistent state is `state.json`, which stores session IDs and cwds but not messages.

Both problems make the daemon unsuitable for real use — clients reconnecting to a session should always see the conversation history.

## Change in a nutshell

Replace `state.json` + the in-memory buffer with a SQLite database at `<config_dir>/jamsession.db`, accessed via the toasty ORM with the `toasty-driver-sqlite` driver.

```rust
#[derive(Debug, toasty::Model)]
struct Session {
    #[key]
    id: String,

    cwd: String,

    created_at: String,

    updated_at: String,

    #[has_many]
    messages: toasty::Deferred<Vec<Message>>,
}

#[derive(Debug, toasty::Model)]
struct Message {
    #[key]
    #[auto]
    id: u64,

    #[index]
    session_id: String,

    #[belongs_to(key = session_id, references = id)]
    session: toasty::Deferred<Session>,

    payload: String, // JSON-serialized SessionNotification
}
```

On `session/load` (from client):
- Query `Message` rows by `session_id`, ordered by `id`.
- Replay them to the client as notifications.
- Spawn agent (if dead) with `session/resume` — the agent never replays history.

On agent notification received:
- Append a `Message` row to the database.

## Detailed plans

### Database location

The database file lives at `<config_dir>/jamsession.db`. When `--config-dir .debug` is used, it's `.debug/jamsession.db`. This keeps test instances fully isolated.

### Schema versioning

The database includes a `schema_version` table (single row) tracking the current schema version. On startup, the daemon checks this and runs any pending migrations. This lets us evolve the schema over time without manual intervention.

The old `state.json` code is deleted outright — no migration path from the JSON file.

### No in-memory buffer

The in-memory `Vec<serde_json::Value>` buffer is removed entirely. All replay reads from the database. Session load is not a hot path — it happens once per client reconnect — so the simplicity of a single source of truth outweighs any latency concern.

### Toasty setup

- Add `toasty` and `toasty-driver-sqlite` as dependencies.
- Define models in a new `src/jamsession/src/db.rs` module.
- The `Db` handle is created in `main.rs` and passed to the `Daemon`/`Dispatcher`.
- Call `db.push_schema().await` on startup to create tables if they don't exist.
- Set `PRAGMA journal_mode=WAL` for crash safety.

### Dispatcher becomes fully async

`Dispatcher::new` and `handle_from_agent` become async. The dispatcher already runs inside an async context (`scope` + `rx.recv().await`), so this is a natural fit. DB writes in `handle_from_agent` are awaited inline — acceptable latency for a single-user daemon, and keeps ordering guarantees trivial.

### Daemon owns replay — agents only get `session/resume`

The daemon is the single source of truth for session history. When a client sends `session/load`, the daemon:

1. Reads message history from the DB.
2. Replays it to the client as notifications.
3. Spawns the agent (if dead) with `session/resume` — not `session/load`.

The agent never needs to know about or replay history. This simplifies the agent contract: agents only receive `session/new` (first time) or `session/resume` (all reconnects). The `session/load` request from the client is handled entirely by the daemon.

This also means the daemon handles `session/load` consistently regardless of whether the agent is alive or dead — it always replays from DB and always sends `session/resume` to the agent.

### `seq` field eliminated

Messages are ordered by the auto-increment `id` column within a session. No separate per-session sequence counter is needed — `ORDER BY id` within a `session_id` gives the correct chronological order.

### Session removal cascades to messages

When a session is removed (e.g., cwd health check finds a deleted directory), its `Message` rows are also deleted from the database.

## Frequently asked questions

### Why toasty instead of raw rusqlite?

Toasty provides derive-based models, async support, and built-in migration tooling. It keeps the data access code concise and type-safe without hand-writing SQL. It also supports multiple backends if we ever want to switch.

### Why not keep state.json for session metadata?

Having two persistence mechanisms (JSON file + SQLite) creates synchronization problems. A single SQLite database is simpler and more atomic.

### What about WAL mode and concurrent access?

SQLite WAL mode allows concurrent readers with a single writer, which matches our architecture (one daemon process, sequential writes from the dispatcher). Toasty/SQLite handles this transparently.

### Does this affect the integration test harness?

Most tests use SQLite `:memory:` — same code path as production, fast, no cleanup. Tests that verify persistence across daemon restarts use a file-backed SQLite in a temp directory.

### Why only `session/resume` to agents, never `session/load`?

The daemon owns history replay. If the agent also replayed, you'd get duplicates. Sending `session/resume` means "you're live, prompts are coming" with no expectation of replay. This also means agents don't need their own session persistence — the daemon handles it all.

### What about failed DB writes?

If a write fails (disk full, etc.), the notification has already been forwarded to the connected client — so the client sees it. But a subsequent load would have a gap. For now: log the error and continue. This is an edge case in a local daemon where disk-full is a bigger problem anyway.

## Implementation

See [Implementation plan and status](./implementation.md).
