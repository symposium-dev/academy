# The `jamsession` MCP Tool

## TL;DR

- The daemon exposes a single MCP tool (`jamsession`) to each agent as a CLI-style interface with subcommands.
- The tool description lists all commands upfront. `help` always returns the same full command table regardless of agent state.
- One tool registration keeps the agent's context cost near zero.

## Motivation

The daemon needs to offer capabilities to agents (coordination, shared state, etc.), but registering many MCP tools pollutes the agent's system prompt with descriptions it may never use. We want a single entry point that scales to many capabilities without paying upfront context cost.

## Change in a nutshell

Register one tool with the agent:

```json
{
  "name": "jamsession",
  "description": "Interface to jamsession daemon. Commands: help, list-members, broadcast, send, post-worklist, remove-worklist, show-worklist, store, retrieve. Use {\"command\":\"help\"} for usage or {\"command\":\"help\",\"subcommand\":\"send\"} for details on a specific command.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "command": { "type": "string" }
    },
    "required": ["command"],
    "additionalProperties": true
  }
}
```

The input is a flat JSON object: `command` selects the action, and any additional fields are command-specific arguments at the top level (no nested `args` object).

**Description-as-index:** The tool description itself lists available commands so the agent sees the menu at zero call cost (no round-trip needed). The `help` command still exists for detailed usage when needed.

The `help` command always returns the same full command table regardless of the agent's current state. Commands that require team membership return a clear error if invoked without one.

## Detailed plans

### Design principles

1. **Minimal context cost** — One tool registration in the agent's system prompt. The description is intentionally terse.
2. **Static help** — `help` always returns the full command table. No state-dependent filtering. Commands that aren't usable yet return clear errors.
3. **Internally tagged enum** — Every invocation is a flat object `{"command": "<name>", ...fields}`. Deserializes directly to a Rust enum via `#[serde(tag = "command")]`. Responses are JSON. Errors include a `help` hint.
4. **Extensible** — New commands are added by later RFDs. The tool description and `help` output are updated to include them.

### The `help` command

Two forms:

```json
{"command": "help"}
{"command": "help", "subcommand": "send"}
```

**General help** (`{"command": "help"}`) always returns the full command table:

```
jamsession commands

command          fields                description
─────────────────────────────────────────────────────
help             [subcommand]          show this help, or detail on one command
list-members     —                     list team members and status
broadcast        message               send to all members
send             to, message           send to one member
post-worklist    item                  add item to shared worklist
remove-worklist  id                    remove worklist item
show-worklist    —                     show shared worklist
store            key, value            store a key-value pair
retrieve         key                   retrieve a stored value

usage: jamsession({"command": "<cmd>", ...fields})

examples:
  jamsession({"command": "send", "to": "agent-2", "message": "done with header"})
  jamsession({"command": "store", "key": "status", "value": "blocked on API"})

note: team commands require membership. Ask your user to run
/jamsession:join-team if you are not yet on a team.
```

**Subcommand help** (`{"command": "help", "subcommand": "send"}`) returns detailed documentation for a single command — its fields, types, behavior, and an example response:

```
send — Send a direct message to a specific team member.

fields:
  to       (string, required)  agent id of recipient
  message  (string, required)  message text

example:
  jamsession({"command": "send", "to": "agent-2", "message": "can you export UserService?"})

response:
  {"delivered": true}

Messages are delivered asynchronously. The recipient sees it on their next turn.
```

### Error handling

Unknown commands return:

```json
{"error": "unknown command: foo", "hint": "Run {\"command\": \"help\"} to see available commands."}
```

### Daemon implementation

The daemon uses the MCP-over-ACP capabilities from the ACP Rust SDK. This requires launching the agent behind a **conductor** (the ACP SDK's term for a process that sits between client and agent, providing MCP tool serving over the ACP transport).

When the MCP tool is invoked, the conductor deserializes the JSON into the command enum and forwards it along with the session ID and a oneshot response channel to the central daemon actor. The daemon actor pattern-matches on the variant, executes the logic, and sends the result back through the channel.

```rust
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
enum JamsessionCommand {
    Help { subcommand: Option<String> },
    ListMembers,
    Broadcast { message: String },
    Send { to: String, message: String },
    Store { key: String, value: serde_json::Value },
    Retrieve { key: String },
    PostWorklist { item: String },
    RemoveWorklist { id: String },
    ShowWorklist,
}
```

Adding a new command is: add a variant, add a match arm. The `help` command returns a static table of all commands.

## Frequently asked questions

### Why not use `tools/list_changed` to update the description dynamically?

Sending `tools/list_changed` invalidates prompt caching on the client side, increasing cost. Instead, the tool description and `help` output always list all commands statically. Commands that require team membership return a clear error if invoked without one. The injected context on team-join tells the agent it can now use those commands — no re-registration needed.

### Why not register separate tools per feature area?

Each registered tool adds ~100-200 tokens to the agent's system prompt. With 10 features that's 1-2k tokens always present even if the agent never uses them. The single-tool approach pays ~50 tokens upfront and expands on demand.

### Why JSON subcommands instead of a free-text CLI string?

Structured input means the daemon can validate and give precise errors. It also means the agent doesn't need to guess at shell-like quoting rules.

### How does the agent know to call `help`?

The tool description says to. Additionally, when the daemon injects context (e.g., on team join), it reminds the agent about the tool and its commands explicitly — so in practice the agent rarely needs to call `help` cold.

## Implementation

See [Implementation plan and status](./implementation.md).
