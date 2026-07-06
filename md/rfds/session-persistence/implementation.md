# Implementation plan and status

This is structured for red-green TDD and can land as a single commit.

### Step 1: Scaffolding — add toasty + sqlite, define models

Add dependencies, create the `db` module with `Session` and `Message` models. Wire up database creation so `Daemon` accepts a `Db` handle.

- [ ] Add `toasty`, `toasty-driver-sqlite` dependencies
- [ ] Create `src/jamsession/src/db.rs` with model definitions
- [ ] Add `schema_version` table
- [ ] Make `Daemon` accept a `Db` handle (pass through to dispatcher)

### Step 2: Test harness infra

Update `TestDaemon` to create a `:memory:` DB and pass it to the daemon. Add `TestDaemon::shutdown()` for graceful stop. Existing tests continue to pass (the buffer still works at this point).

- [ ] Pass `:memory:` DB handle through `TestDaemon` setup
- [ ] Add `TestDaemon::shutdown()` via `CancellationToken` or similar
- [ ] Verify all existing tests still pass

### Step 3: Write failing persistence tests (RED)

Write the new integration tests. They will fail because the daemon still uses the in-memory buffer (which gets cleared on agent disconnect / lost on restart).

- [ ] **Replay after agent death**: client creates session, sends prompts, disconnects. Wait for agent idle timeout (killing agent clears the buffer today). Second client loads the same session and asserts history is replayed via updates.
- [ ] **Replay across daemon restarts**: daemon starts with an on-disk SQLite file in a temp dir. Client creates session and prompts. Test calls `daemon.shutdown()`. A new daemon starts pointing at the same DB and socket path. Client loads session and asserts history is replayed.
- [ ] **Session list after restart**: after daemon restart, `list_sessions` still returns the previously created session.

### Step 4: Implement persistence (GREEN)

Swap the in-memory buffer for DB reads/writes. All persistence tests pass.

- [ ] Write messages to DB in `handle_from_agent`
- [ ] Query messages by `session_id` (ordered by `id`) on the load path
- [ ] Unify `session/load` handling: daemon always replays from DB, always sends `session/resume` to agent
- [ ] Remove in-memory buffer from dispatcher
- [ ] Remove both `buffer.clear()` calls (`handle_agent_disconnected` and `handle_idle_timeout`)
- [ ] Cascade-delete messages when a session is removed (cwd health check)

### Step 5: Replace state.json with Session table

Delete `state.rs` and the JSON persistence code. Session CRUD goes through toasty. No migration from the old format.

- [ ] Move session CRUD to DB
- [ ] Delete `state.rs`
- [ ] Update `main.rs` to create DB at `<config_dir>/jamsession.db`
