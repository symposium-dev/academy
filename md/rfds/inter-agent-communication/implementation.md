# Implementation plan and status

This plan covers both this RFD and its sub-RFD, the
[`jamsession` tool](./jamsession-tool/README.md). The tool is the delivery
vehicle for every team command, so the two are implemented together.

The steps are ordered for **red-green TDD**: each introduces one concept, is
driven by a failing test first, and leaves the tree building. Behavior is kept
separate from transport wherever possible so most logic is unit-testable without
any ACP/MCP plumbing.

## Background: how the tool reaches the agent

The daemon is the ACP *client* toward each agent. It offers the `jamsession`
tool as an **MCP-over-ACP** server attached to the session:

- Build the server with
  `agent_client_protocol_rmcp::McpServerExt::builder("jamsession")`, register one
  tool, and `build()`.
- `McpServer::into_handler_and_responder()` →
  `handler.into_dynamic_handler(&mut new_session_request, &cx)?` mutates the
  outgoing `NewSessionRequest` to push an `McpServer::Http { url: "acp:<uuid>" }`
  entry and registers a dynamic handler that answers the agent's `_mcp/*`
  traffic. Keep the registration alive with `.run_indefinitely()`.
- The tool's `call_tool` closure captures the `agent_id` and a `DispatcherMessage`
  sender at construction (the `McpConnectionTo` context does **not** carry our
  session id), so each tool call can be routed back to the right session's team.

Because `claude-acp` does not support `mcpCapabilities.acp`, the daemon must run
the target agent behind **its own conductor** that includes the MCP-over-ACP
polyfill, rather than relying on acpr's built-in `AgentOnly` conductor. The
factory wraps whatever agent it produces (a `DynConnectTo<Client>`) as:

```rust
ConductorImpl::new_agent(
    name,
    ProxiesAndAgent::new(inner_agent).proxy(McpOverAcpPolyfill::http()),
)
```

Nested conductors compose: acpr runs its own inner conductor in front of
`claude-acp`; from our outer conductor it is just an agent speaking ACP. The
polyfill rewrites the daemon's `acp:` URL into a localhost bridge and tunnels
`_mcp/*` back over ACP.

> **Known limitation — MCP-over-ACP is new-session-only (upstream).** In the
> current SDK and polyfill, MCP servers are attached exclusively through
> `NewSessionRequest`: the SDK's `McpNewSessionHandler` matches only
> `NewSessionRequest`, `into_handler_and_responder` is `pub(crate)` (so
> `SessionBuilder::with_mcp_server` is the only public attach path), and the
> polyfill rewrites `acp:` URLs only on `NewSessionRequest` — never on
> `ResumeSessionRequest`/`LoadSessionRequest`, despite both carrying an
> `mcp_servers` field. **Consequence for the daemon:** an agent respawned after
> idle-kill comes back via `session/resume` *without* the `jamsession` tool. New
> sessions are fine; resumed ones lose the tool until the next new session. This
> directly affects Step 6 (a queued team-message wakes a dead agent, but the
> respawned agent cannot `send`/`broadcast` back). The daemon-side rework of
> mcp-over-acp is tracked separately; what the daemon needs from it: (1) an
> MCP-serve path that survives respawn (available on resume/load, or a host
> re-attach contract on every session-start variant); (2) per-agent handler
> identity so the tool closure can keep capturing `agent_id`; (3) ideally native
> `mcpCapabilities.acp` so production needs no localhost-bridge polyfill hop. Until
> then, Step 2 wires the tool through the current new-session-only polyfill.

The integration-test harness mirrors this: the test `RhaiAgentFactory` wraps the
`RhaiAgent` the same way, so a Rhai script can call
`mcp::call_tool("jamsession", "jamsession", #{ command: "help" })`. This is the
pattern proven by `rhaicp`'s own `tests/mcp_tools.rs`.

Slash commands (`/jamsession:*`) are **not** a separate ACP RPC — the agent
advertises commands via `SessionUpdate::AvailableCommandsUpdate`, and invocation
arrives as an ordinary `session/prompt`. So the daemon detects a leading
`/jamsession:` text block in `handle_from_client`, handles it locally (updating
team state, persisting the user message and the daemon's reply for replay), and
does **not** forward it to the agent.

Context/message injection reuses the guideline-delivery path: the daemon sends a
`PromptRequest` to the agent. Since the central dispatcher only holds an
`mpsc::UnboundedSender<Dispatch>` per agent, injection needs a small new seam — a
per-agent control channel drained by the `agent_pipe` task, which turns each
request into `cx.send_request(PromptRequest…)`.

## Step 1: Command core (pure logic, no ACP)

New module `src/jamsession/src/jamsession_tool/command.rs`: the full
`JamsessionCommand` enum (`#[serde(tag = "command", rename_all = "kebab-case")]`)
with **every** variant from both RFDs present, plus a
`dispatch(state, cmd) -> serde_json::Value`. At this step only `help` is live;
every team command returns `{"error": "not a team member", …}`; unknown commands
return `{"error": "unknown command …", "hint": …}`. The `help` output is the full
static command table matching the RFD.

**Status: done.** Implemented as `jamsession_tool/command.rs` with a single
`COMMANDS` spec table driving both the general help and per-command detail (no
drift). `dispatch_json` parses and dispatches; malformed known commands return an
`invalid arguments` error.

**Red (unit):**

- [x] general `help` returns the full command table
- [x] `help` with `subcommand: "send"` returns the detailed `send` docs
- [x] an unknown command returns the structured error + hint
- [x] a team command (e.g. `send`) returns not-a-member when no team state exists

## Step 2: MCP transport wiring (first, riskiest integration)

Attach the `jamsession` MCP server in `agent_pipe`. The tool forwards raw JSON
to a new `DispatcherMessage::JamsessionCommand { agent_id, input, respond }`; the
dispatcher calls Step 1's `dispatch` (no team state yet) and replies over the
oneshot. Wrap the test `RhaiAgent` in conductor + `McpOverAcpPolyfill::http()`
(behind a flag so existing tests are undisturbed); add
`agent-client-protocol-conductor`, `-polyfill`, and `-rmcp` to `jamsession-test`
dev-deps.

Per the **Known limitation** above, the tool can only be attached on the
`New` session path today (`SessionBuilder::with_mcp_server` mutates a
`NewSessionRequest`, and the polyfill bridges `acp:` URLs only there). Step 2
therefore wires the `New` path; resumed/respawned agents will regain the tool
once the mcp-over-acp rework lands.

**Status: done.** The daemon wraps the agent transport (in `handle_session_new`)
as `ConductorImpl::new_agent(ProxiesAndAgent::new(inner).proxy(tool).proxy(polyfill))`;
the conductor/polyfill/rmcp deps live in the `jamsession` crate. The tool
forwards `JamsessionToolCall` over a channel; a forwarder task rewraps it as
`DispatcherMessage::JamsessionToolCall`. Serving is gated behind
`Daemon::with_serve_jamsession_tool` (off in the test harness by default).

**Red (integration):**

- [x] a Rhai script `say(mcp::call_tool("jamsession", "jamsession", #{command:"help"}))`
      returns the help table
- [x] unknown-command and not-a-member responses arrive over the real tool path

## Step 3: Team persistence (pure DB)

Add toasty models (`Team` + membership) and `Store` methods: `join_team`,
`leave_team`, `list_teams`, `team_of_session`, `team_members`.

**Status: done.** Implemented as a single `TeamMembership` model keyed by
`session_id`, so the one-team-per-session invariant *is* the primary key and
teams are implicit (no separate teams table). The Store reads team state
directly, so no in-memory rehydration cache was needed. `remove_session` now
also clears membership, and the trace-table migration was generalized to
`add_missing_tables` (adds any suffix of new tables to an existing database).

**Red (unit, db-level):**

- [x] join / leave / list transitions
- [x] one-team-per-session invariant enforced (joining a second team replaces)
- [x] team membership survives a daemon restart (file-backed store)
- [x] membership cleared on session removal; migration adds the table to an old db

## Step 4: Slash commands + injection seam

Intercept `/jamsession:{join-team,leave-team,teams}` in `handle_from_client`:
update team state, persist the user message and the daemon's reply (for replay),
respond via a client notification, and do **not** forward to the agent. On
successful join, inject the `<context>…now a member…</context>` prompt to the
live agent. Introduce the per-agent injection channel here.

**Status: done.** Pure parsing/rendering lives in `jamsession_tool/slash.rs`;
the dispatcher supplies effects. The reply is delivered as an `AgentMessageChunk`
notification followed by `PromptResponse(EndTurn)`. Both are handed to the client
pipe as one `SlashReply` unit and emitted on the connection in order, so the
reply text always reaches the client before the turn ends (an earlier version
sent the notification and response over two queues, which raced — caught in
review, fixed, and guarded by a multi-threaded regression test). The reply is
persisted for replay. The injection seam is a per-agent `inject_tx` in
`AgentHandle`, drained by a `tokio::select!` in `agent_pipe` that turns injected
text into a `session/prompt` (fire-and-forget). Injecting to a dead agent is a
silent no-op.

**Deviation:** advertising the commands via a merged `AvailableCommandsUpdate` is
deferred — the rhaicp mock agent never emits `AvailableCommandsUpdate`, so it is
untestable in this harness, and its effect is only a client-side command menu.
Tracked as a follow-up.

**Red (integration):**

- [x] `/jamsession:join-team foo` produces a daemon reply, the agent is **not**
      prompted with the command, and the join `<context>` is injected into the agent
- [x] `/jamsession:teams` lists the active team; `leave-team` and invalid commands
      report appropriately

## Step 5: Wire team state into dispatch + `list-members`

`dispatch` now consults membership (agent_id → session → team). Implement
`list-members`, returning each member's id, working_dir, and status.

**Status: done.** `dispatch_json` takes a `TeamContext { team, me, members }`
(plain data), keeping `command.rs` free of storage concerns. The dispatcher's
`team_context_for` resolves it from the Store (team, roster) and in-memory
session state (working_dir from the session record; `active` when a live agent
backs the session, else `idle`). `list-members` returns the roster; the other
team commands are membership-gated and report `command not yet implemented`
until their steps land.

**Red:**

- [x] `list-members` returns the roster (unit) and, end-to-end, the joining
      session sees itself as an active member (integration)
- [x] before join, `list-members` returns not-a-member

## Step 6: Messaging (`send` + `broadcast`)

The dispatcher resolves team members and injects
`<team-message from="…" type="…">…</team-message>` to live recipients via the
Step-4 seam. Messages for dead recipients are queued in a DB table and flushed on
the recipient's next session activation. `send` to an unknown or off-team agent
returns the structured error.

**Status: done.** Message rendering is pure (`jamsession_tool/message.rs`).
`send`/`broadcast` are side-effecting, so the dispatcher handles them (rather
than the pure command core): `parse_message_command` extracts them only for an
on-team caller, and `deliver_team_message` injects to a live recipient or queues
to the `PendingMessage` table otherwise. `flush_pending_messages` drains the
queue (in send order) when an agent becomes ready in `handle_agent_ready`.

While building this, the injection seam revealed a latent defect: `inject_prompt`
sent the prompt via `on_receiving_result`, whose canceled response — which occurs
when a client disconnects right after triggering an injection — surfaced as a
connection error and killed the agent. Injection now spawns the request and
swallows its result, so it is truly fire-and-forget. (This also hardens Step 4's
join-context injection.)

**Red (integration):**

- [x] two team members: a broadcast reaches a live peer as a `<team-message>`
- [x] `broadcast` reports `delivered_to`; `send` to an unknown agent errors
- [x] a message queued for a dead agent is delivered after it respawns

## Step 7: Worklist (`post-worklist`, `remove-worklist`, `show-worklist`)

Add the worklist DB table and `Store` methods, then the dispatch arms.

**Status: done.** `WorklistItem` model scoped by `team`; the public id is
`wl-<n>`. The dispatch arms live in the dispatcher (they write the DB), reached
via a generalized `effectful_command` seam that routes every side-effecting
on-team command (messaging, worklist, store) to the dispatcher while pure
commands stay in the command core.

**Red:**

- [x] db-level unit tests for post / remove / show, item counts, and team scoping
- [x] integration: post → show → remove with correct `id` and `items_count`,
      plus an unknown-id error

## Step 8: Key-value store (`store`, `retrieve`)

Add the shared-store DB table and `Store` methods, then the dispatch arms.

**Status: done.** `StoreEntry` model scoped by `team`; `store_put` replaces any
prior value for a key (last-write-wins), and values are arbitrary JSON. Dispatch
arms sit alongside the worklist arms behind the same `effectful_command` seam.

**Red:**

- [x] db-level unit tests for store / retrieve (string + object values), replace,
      and team scoping
- [x] integration: store → retrieve round-trips, overwrite, and missing-key error
