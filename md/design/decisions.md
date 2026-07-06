# Architecture Decisions

This page indexes the **load-bearing decisions** behind Jamsession's design — the choices that
cut across the whole system and constrain everything built on top of them. Each has a stable
code (`D1`, `D2`, …) so other docs and RFDs can point to it without restating it. The codes are
a documentation handle only — there is no need to tag the code with them; a comment may cite a
`D<n>` where it genuinely helps a reader, but that is optional, not expected.

Treat these as invariants: an agent working in the codebase should not quietly violate one.
If a decision genuinely needs to change, that is itself a design change — work it through the
[Architecture & Design](./README.md) section and, where it affects behavior, an RFD; then
update the entry here. A decision may also be *proposed* rather than *adopted* — record it here
with that status while the design is still being worked out.

Decisions that are local to a single feature live in that feature's RFD (with its rationale and
alternatives), not here. The [Feature-local decisions](#feature-local-decisions) section at the
end links to them so they are discoverable without duplicating their reasoning.

## D1 — Single-writer dispatcher

- **Decision:** One `Dispatcher` actor is the sole owner of core session state
  (`sessions: HashMap<SessionId, Session>`) and processes a serialized stream of
  `DispatcherMessage`s. No mutexes guard session state.
- **Why:** Serializing every state change through one actor loop removes lock-ordering hazards
  and data races by construction, and keeps the routing/lifecycle logic in one place that is
  easy to reason about. Concurrency lives at the edges (per-client and per-agent pipe tasks,
  timer tasks) — never on the shared state.
- **Grounding:** `src/jamsession/src/dispatcher.rs` (`struct Dispatcher`, the single receive
  loop); [Architecture & Design](./README.md) → "Single writer".
- **Status:** Adopted.

## D2 — Timers as messages, invalidated by a generation counter

- **Decision:** Timers (quiescence, idle) do not hold cancellation handles. Each timer is a
  lightweight task that sleeps and then sends a message (`AgentQuiescent`,
  `IdleTimeoutElapsed`) carrying the session `generation` it was spawned at. Each session
  holds a monotonic generation; the dispatcher discards a timer message whose generation no
  longer matches.
- **Why:** Cancelling timer tasks would mean tracking and aborting handles, and reintroduce
  the shared-mutable-state / locking problem that [D1](#d1--single-writer-dispatcher) avoids.
  Bumping a counter on any relevant event (e.g. a client reconnecting) invalidates stale timers
  for free, with no handles to track.
- **Grounding:** `src/jamsession/src/dispatcher.rs` (`AgentQuiescent { generation }`,
  `IdleTimeoutElapsed { generation }`); [Architecture & Design](./README.md) → "Timers as
  messages" and "Generation-counter timers".
- **Status:** Adopted.

## D3 — Ephemeral agents; the daemon owns and replays history

- **Decision:** Agent processes are disposable. They are killed after an idle timeout and
  respawned on demand via `session/resume`. Conversation history is persisted and replayed by
  the daemon, never by the agent — agents only ever receive `session/new` (first time) or
  `session/resume` (every reconnect).
- **Why:** In ACP, the agent is typically responsible for "replaying" sessions to the client. The Daemon may be proxying multiple clients to one background agent and therefore may have to replay the same session multiple times to each client. Storing the data simplifies this. It also permits the daemon to add additional "meta-events" (e.g., joining a team) that the agent is not aware of.
- **Grounding:** [Architecture & Design](./README.md) → "Ephemeral agents";
  [Session persistence RFD](../rfds/session-persistence/README.md);
  [terminology](../terminology.md) → "Agent", "Respawn".
- **Status:** Adopted.

## D4 — An agent's identity is its session id

- **Decision:** A session is identified by its `session_id`, and that same id serves as the
  agent's identity for team membership and messaging. The id is reused across resume/respawn
  (stable for the life of the session). There is no separate `participant_id` concept.
- **Why:** Because the daemon owns session state ([D1](#d1--single-writer-dispatcher)) and
  respawns agents under their existing id ([D3](#d3--ephemeral-agents-the-daemon-owns-and-replays-history)),
  keying team membership and the pending-message queue on `session_id` means that state
  survives a respawn with no extra bookkeeping. A second identity would have to be reconciled
  with the session id on every reconnect for no gain.
- **Grounding:** [terminology](../terminology.md) → "session id"; `src/jamsession/src/db.rs`
  (`TeamMembership`, `PendingMessage` keyed by session id);
  `src/jamsession/src/jamsession_tool/command.rs` (`MemberInfo::id` — "currently the session
  id").
- **Status:** Adopted. *Note:* the coupling of session identity and agent identity is
  deliberate today but is the most likely of these decisions to be revisited if agents ever
  need an identity independent of a single session.

## D5 — The daemon is the sole ACP endpoint

- **Decision:** Clients (editors/CLIs) connect only to the daemon over its Unix socket; they
  never speak ACP to an agent directly. The daemon sits in the middle and bridges. A session
  may have multiple connected clients; agent output routes to the most-recently-connected one.
- **Why:** A single intermediary is what makes ephemeral agents, history replay, tracing, and
  team messaging possible at all — every message already flows through the daemon, so it can
  persist, route, and inject without the client or agent cooperating. Direct client↔agent
  connections would bypass all of that.
- **Grounding:** [terminology](../terminology.md) → "Daemon", "ACP", "Bridge / relay";
  `src/jamsession/src/daemon.rs` (accept loop spawning a `client_pipe` per connection);
  [Architecture & Design](./README.md) → "Multiple clients per session".
- **Status:** Adopted.

## D6 — Persistence is SQLite via Toasty

- **Decision:** Session metadata, conversation history, optional traces, team membership,
  pending team messages, the worklist, and the shared key-value store are all persisted in a
  single SQLite database (`jamsession.db`) through Toasty models. This replaced an earlier
  in-memory buffer plus `state.json`.
- **Why:** Session state must survive daemon restarts (a prerequisite for
  [D3](#d3--ephemeral-agents-the-daemon-owns-and-replays-history)). SQLite gives durable,
  queryable, transactional storage in-process; Toasty provides derive-based async models and
  migration tooling, keeping data access concise and type-safe without hand-written SQL.
- **Grounding:** [Session persistence RFD](../rfds/session-persistence/README.md);
  `src/jamsession/src/db.rs` (Toasty models: `Session`, `Message`, `Trace`, `TeamMembership`,
  `PendingMessage`, `WorklistItem`, `StoreEntry`).
- **Status:** Adopted.

## D7 — A session belongs to at most one team

- **Decision:** Team membership is keyed by `session_id` in a single `TeamMembership` row per
  session, so a session (agent) can be on at most one team at a time. Joining a new team
  replaces the prior membership.
- **Why:** The one-team-per-session rule *is* the primary key — the invariant is enforced by
  the schema rather than by application logic that could drift. It keeps "who can see and
  message whom" simple to reason about, consistent with the human-orchestrator model (see
  [Feature-local decisions](#feature-local-decisions)).
- **Grounding:** `src/jamsession/src/db.rs` (`TeamMembership`, keyed by `session_id`);
  [inter-agent communication implementation](../rfds/inter-agent-communication/implementation.md).
- **Status:** Adopted.

## Feature-local decisions

These are settled within a single feature's RFD, which carries the full rationale and the
alternatives considered. They are linked here for discoverability; the RFD is the source of
truth.

**Inter-agent communication** — [RFD](../rfds/inter-agent-communication/README.md):

- **Humans control team membership; agents do not self-join** — a human is the orchestrator,
  agents are workers; autonomous joining creates coordination hazards.
  [Why](../rfds/inter-agent-communication/README.md#why-does-the-human-control-team-membership).
- **No direct agent-to-agent communication** — messaging goes through the daemon.
  [Why](../rfds/inter-agent-communication/README.md#why-not-direct-agent-to-agent-communication).
- **At-least-once team-message delivery** — messages are durably queued in SQLite *before*
  injection and deleted only after handoff, so an agent going away mid-flush leaves the
  remainder queued.
  [Detail](../rfds/inter-agent-communication/implementation.md).

**The `jamsession` MCP tool** —
[RFD](../rfds/inter-agent-communication/jamsession-tool/README.md):

- **One MCP tool with subcommands, not one tool per feature** — avoids paying per-tool prompt
  overhead for features an agent may never use.
  [Why](../rfds/inter-agent-communication/jamsession-tool/README.md#why-not-register-separate-tools-per-feature-area).
- **JSON subcommands, not a free-text CLI string** — deserializes directly to a tagged Rust
  enum.
  [Why](../rfds/inter-agent-communication/jamsession-tool/README.md#why-json-subcommands-instead-of-a-free-text-cli-string).
- **No `tools/list_changed` for dynamic descriptions** — it invalidates client-side prompt
  caching; the tool lists all commands statically and errors clearly when one needs a team.
  [Why](../rfds/inter-agent-communication/jamsession-tool/README.md#why-not-use-toolslist_changed-to-update-the-description-dynamically).

**Message trace & debug viewer** — [RFD](../rfds/message-trace/README.md):

- **Responses routed back through the actor loop (wrapped responders)** — gives a single
  canonical ordering for all trace events.
  [Why](../rfds/message-trace/README.md#why-route-responses-through-the-actor-loop).
- **Tracing is opt-in, not always-on** — [Why](../rfds/message-trace/README.md#why-not-always-on).
