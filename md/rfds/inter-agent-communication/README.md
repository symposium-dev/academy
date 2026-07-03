# Inter-Agent Communication

## TL;DR

- Adds team-based communication commands to the [`jamsession` tool](./jamsession-tool/README.md).
- Team membership is controlled by the human via slash commands (`/jamsession:join-team`), not by the agent.
- Commands: `list-members`, `broadcast`, `send`, `post-worklist`, `remove-worklist`, `show-worklist`, `store`, `retrieve`.

## Motivation

Agents working on related tasks (e.g., frontend + backend of the same feature) need to coordinate: share status, divide work, and exchange information. Without a structured communication channel, the only option is file-based coordination, which is fragile and invisible to the daemon.

## Change in a nutshell

Building on the [`jamsession` tool RFD](./jamsession-tool/README.md) — a sub-RFD of this one, since the tool is the requisite delivery vehicle — this RFD adds commands for team-based inter-agent communication. The `jamsession` tool's static description already lists these commands. When invoked before team membership is established, they return an error directing the user to `/jamsession:join-team`.

Example once on a team:

```json
{"command": "send", "to": "agent-2", "message": "can you export UserService?"}
```

## Detailed plans

### Architecture

```
┌─────────┐   MCP    ┌────────┐   MCP    ┌─────────┐
│  Agent  │◄────────►│ Daemon │◄────────►│  Agent  │
└─────────┘          └────────┘          └─────────┘
     │                    │                    │
     └────── Team "foo" ──┴──── Team "foo" ────┘
```

The daemon is the hub. Agents never communicate directly — all messages route through the daemon. A **team** is a named group of agents that can see and message each other. An agent belongs to at most one team at a time.

### Teams and membership

Team join/leave is driven by the user via slash commands. These are registered by the **daemon** on top of whatever slash commands the agent provides:

| Command | Description |
|---------|-------------|
| `/jamsession:join-team TEAM` | Join the named team (creates it if new) |
| `/jamsession:leave-team` | Leave the current team |
| `/jamsession:teams` | List all active teams and their members |

These are single-message interactions — no multi-step interactive flow. If the user sends `/jamsession:join-team` without a team name (or with an invalid name), the daemon responds with a help message listing available teams and advising them to try again with `/jamsession:join-team $TEAM`. The agent is not involved in this exchange.

Both the user's slash command and the daemon's response are stored in the session database and replayed to the user on reconnect, even though the agent never sees them. They are part of the user-facing conversation history, not the agent's context.

When the join succeeds, the daemon injects context into the agent:

```xml
<context>
You are now a member of the jamsession team "frontend-refactor".
Team members: agent-1 (you), agent-2, agent-3.

You can now use team commands via the jamsession tool.
Use {"command": "help"} for the full command table, or
{"command": "help", "subcommand": "send"} for details on a specific command.
</context>
```

### New commands

All commands require team membership. If invoked without a team, they return:

```json
{"error": "not a team member", "hint": "Ask your user to run /jamsession:join-team."}
```

#### `list-members`

```json
{"command": "list-members"}
```

Response:
```json
{
  "members": [
    {"id": "agent-1", "working_dir": "/home/user/project/frontend", "status": "active"},
    {"id": "agent-2", "working_dir": "/home/user/project/backend", "status": "idle"}
  ]
}
```

#### `broadcast`

Send a message to all other team members. Delivered asynchronously.

```json
{"command": "broadcast", "message": "I've finished the auth module, ready for integration."}
```

Response:
```json
{"delivered_to": ["agent-2", "agent-3"]}
```

#### `send`

Send a direct message to a specific peer.

```json
{"command": "send", "to": "agent-2", "message": "Can you expose the UserService as pub?"}
```

Response:
```json
{"delivered": true}
```

If the recipient doesn't exist or isn't on the team:
```json
{"error": "unknown agent", "agent": "agent-2"}
```

#### `post-worklist`

Add an item to the team's shared worklist.

```json
{"command": "post-worklist", "item": "Refactor auth middleware to use new token format"}
```

Response:
```json
{"id": "wl-3", "items_count": 5}
```

#### `remove-worklist`

Remove a completed item from the worklist.

```json
{"command": "remove-worklist", "id": "wl-3"}
```

Response:
```json
{"removed": true, "items_count": 4}
```

#### `show-worklist`

```json
{"command": "show-worklist"}
```

Response:
```json
{
  "items": [
    {"id": "wl-1", "item": "Set up shared test fixtures", "posted_by": "agent-1"},
    {"id": "wl-2", "item": "Define API contract for /users endpoint", "posted_by": "agent-2"}
  ]
}
```

#### `store`

Store a key-value pair in the team's shared store.

```json
{"command": "store", "key": "api-base-url", "value": "http://localhost:3000"}
```

Response:
```json
{"stored": true}
```

#### `retrieve`

Retrieve a value from the shared store. Values are arbitrary JSON (`serde_json::Value`).

```json
{"command": "retrieve", "key": "api-base-url"}
```

Response:
```json
{"key": "api-base-url", "value": "http://localhost:3000"}
```

If the key does not exist:
```json
{"error": "key not found", "key": "api-base-url"}
```

### Message delivery

Messages (from `broadcast` and `send`) are delivered **asynchronously**. The daemon queues messages for each recipient. Messages are injected into the agent's conversation at the start of its next turn (or immediately if the agent is idle and gets woken).

The delivery mechanism reuses the existing daemon→agent context injection path.

#### Message format (as seen by recipient)

```xml
<team-message from="agent-1" type="broadcast">
I've finished the auth module, ready for integration.
</team-message>
```

### Daemon data model

```rust
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
enum JamsessionCommand {
    // ... existing variants from jamsession-tool RFD ...

    // Added by this RFD:
    ListMembers,
    Broadcast { message: String },
    Send { to: String, message: String },
    PostWorklist { item: String },
    RemoveWorklist { id: String },
    ShowWorklist,
    Store { key: String, value: serde_json::Value },
    Retrieve { key: String },
}
```

## Frequently asked questions

### Why does the human control team membership?

Letting agents autonomously join teams creates coordination hazards and makes it harder for the human to reason about what's happening. The human is the orchestrator; the agents are workers.

### Why not direct agent-to-agent communication?

Routing through the daemon gives us: message persistence, delivery guarantees, access control, observability (message traces), and the ability to inject messages even when the recipient agent process is dead (they'll see it on next wake).

### What happens if a command is invoked without team membership?

All team commands return a structured error: `{"error": "not a team member", "hint": "Ask your user to run /jamsession:join-team."}`. This avoids needing `tools/list_changed` notifications or dynamic tool descriptions.

## Implementation

See [Implementation plan and status](./implementation.md).
