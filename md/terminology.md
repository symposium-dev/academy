# Terminology

Definitions for terms used throughout this book. Add an entry whenever a new term is
worth defining, so pages can use it without re-explaining. Ground each definition in the
real system; when a term names a type or module, it should match the code.

## Protocols

- **ACP (Agent Client Protocol)** — the protocol editor/CLI clients and agent processes
  speak. Jamsession sits in the middle as the sole ACP endpoint for clients.
- **MCP (Model Context Protocol)** — the protocol for exposing tools to an agent. The
  daemon exposes its capabilities to agents as a single MCP tool.
- **MCP-over-ACP** — serving MCP tools through the ACP transport (rather than a separate
  connection), via a conductor and, for agents lacking native support, a polyfill.

## Daemon and processes

- **Daemon** — the long-running `jamsession` process. It owns spawning, bridging, and
  lifecycle for all sessions and is the single Unix-socket endpoint clients connect to.
- **Dispatcher** — the daemon's central actor and single owner of session state
  (`sessions: HashMap<SessionId, Session>`). It routes messages, manages timers, records
  traces, and handles the team commands. No mutexes: one writer.
- **Client** — an editor or CLI connected to the daemon over the Unix socket. A session may
  have more than one client; outgoing messages route to the most-recently-connected one.
- **Agent** — an AI coding agent process managed by the daemon. Agents are **ephemeral**:
  disposable, killed after idle, respawned on demand.
- **Conductor** — the ACP SDK's term for a process sitting between client and agent that
  provides MCP tool serving over the ACP transport. The daemon wraps agents in a conductor
  to serve the jamsession tool.

## Sessions and lifecycle

- **Session** — a persistent conversation context, identified by a session id. It outlives
  any single agent process; the daemon owns and replays its history.
- **session id** — the identifier for a session. It is **reused across resume/respawn**
  (stable for the life of the session), and currently also serves as an agent's identity
  for team membership and messaging.
- **`session/new`, `session/load`, `session/resume`** — ACP requests: create a new session;
  reconnect and replay history (respawning the agent if it died); resume a dead agent under
  its existing session id.
- **Respawn** — bringing an idle-killed agent back via `session/resume`. Conversation
  history is replayed by the daemon, not the agent.
- **Quiescent** — the state after an agent finishes a turn; it becomes eligible for the idle
  timeout.
- **Idle spin-down** — killing a quiescent agent after an idle timeout to free resources.
  The session and its history persist; the agent respawns on demand.
- **Lifecycle event** — an observable outcome the dispatcher *emits* (connect, disconnect,
  spawn, quiescent, kill, session created/loaded/resumed). Tests subscribe and assert on
  these. Distinct from **`DispatcherMessage`**, which is what the dispatcher *processes*.
- **Generation-counter timer** — instead of tracking and aborting timer tasks, each session
  carries a monotonic generation; a timer message carries the generation it was spawned at,
  and stale timers are discarded on mismatch.

## Messaging and persistence

- **Bridge / relay** — steady-state bidirectional routing of messages between a client and
  its agent, through the daemon. Clients never talk to agents directly.
- **SQLite store** — session metadata, agent notifications, optional traces, and team state
  are persisted in `jamsession.db` (via the Toasty models in `db`).
- **Trace** — optional (`trace = true`) recording of ACP dispatches and lifecycle events in
  SQLite, inspectable via `jamsession debug`.

## Teams (inter-agent communication)

- **jamsession tool** — the single MCP tool the daemon exposes to each agent, offering
  CLI-style JSON subcommands (`help`, `list-members`, `broadcast`, `send`, worklist, and a
  shared key-value store).
- **Team** — a named group of agents that can see and message each other. An agent belongs
  to at most one team at a time. A **human** puts agents on a team; agents do not self-join.
- **Slash command (`/jamsession:*`)** — human-driven daemon commands (`join-team`,
  `leave-team`, `teams`) that the daemon intercepts and handles without involving the agent.
- **Worklist** — a team's shared, ordered list of work items.
- **Shared key-value store** — a per-team key-value store (`store` / `retrieve`) holding
  arbitrary JSON values.
- **Pending message** — a team message durably queued in SQLite for a recipient, delivered
  when its agent is next live. Delivery is **at-least-once**.
