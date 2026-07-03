//! The `jamsession` command enum and its dispatch.
//!
//! This module is deliberately free of any ACP/MCP transport concerns: it
//! turns a JSON request into a [`JamsessionCommand`] and produces a
//! [`serde_json::Value`] response. The transport layer (see the dispatcher)
//! is responsible only for shuttling those JSON values to and from the agent.
//!
//! At this stage only [`help`](JamsessionCommand::Help) is implemented; every
//! team command reports that the agent is not yet a team member. Later steps
//! thread team state through [`dispatch`] and light those commands up.

use serde::Deserialize;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Command enum
// ---------------------------------------------------------------------------

/// A single invocation of the `jamsession` tool.
///
/// The wire form is a flat, internally-tagged object: `command` selects the
/// variant and the remaining fields are its arguments, e.g.
/// `{"command": "send", "to": "agent-2", "message": "hi"}`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum JamsessionCommand {
    /// Show the command table, or detailed help for one command.
    Help {
        #[serde(default)]
        subcommand: Option<String>,
    },
    /// List the members of the agent's team and their status.
    ListMembers,
    /// Send a message to all other members of the team.
    Broadcast { message: String },
    /// Send a direct message to a specific team member.
    Send { to: String, message: String },
    /// Add an item to the team's shared worklist.
    PostWorklist { item: String },
    /// Remove an item from the team's shared worklist.
    RemoveWorklist { id: String },
    /// Show the team's shared worklist.
    ShowWorklist,
    /// Store a key-value pair in the team's shared store.
    Store { key: String, value: Value },
    /// Retrieve a value from the team's shared store.
    Retrieve { key: String },
}

// ---------------------------------------------------------------------------
// Command specifications (source of truth for help text)
// ---------------------------------------------------------------------------

/// Static description of one command, used to render both the general help
/// table and the per-command detail returned by `help <subcommand>`.
struct CommandSpec {
    /// The command name as it appears on the wire (kebab-case).
    name: &'static str,
    /// One-line summary of the command's fields, for the help table.
    fields: &'static str,
    /// One-line description, for the help table.
    description: &'static str,
    /// Full detail body returned by `{"command": "help", "subcommand": name}`.
    detail: &'static str,
}

/// Every command, in the order they appear in the help table.
const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "help",
        fields: "[subcommand]",
        description: "show this help, or detail on one command",
        detail: "help — Show the command table, or detailed help for one command.\n\
                 \n\
                 fields:\n\
                 \x20 subcommand  (string, optional)  command to show detail for\n\
                 \n\
                 example:\n\
                 \x20 jamsession({\"command\": \"help\", \"subcommand\": \"send\"})",
    },
    CommandSpec {
        name: "list-members",
        fields: "—",
        description: "list team members and status",
        detail: "list-members — List the members of your team and their status.\n\
                 \n\
                 fields:\n\
                 \x20 (none)\n\
                 \n\
                 example:\n\
                 \x20 jamsession({\"command\": \"list-members\"})\n\
                 \n\
                 response:\n\
                 \x20 {\"members\": [{\"id\": \"agent-1\", \"working_dir\": \"...\", \"status\": \"active\"}]}\n\
                 \n\
                 Requires team membership.",
    },
    CommandSpec {
        name: "broadcast",
        fields: "message",
        description: "send to all members",
        detail: "broadcast — Send a message to all other members of your team.\n\
                 \n\
                 fields:\n\
                 \x20 message  (string, required)  message text\n\
                 \n\
                 example:\n\
                 \x20 jamsession({\"command\": \"broadcast\", \"message\": \"auth module ready\"})\n\
                 \n\
                 response:\n\
                 \x20 {\"delivered_to\": [\"agent-2\", \"agent-3\"]}\n\
                 \n\
                 Messages are delivered asynchronously. Recipients see them on their next turn.",
    },
    CommandSpec {
        name: "send",
        fields: "to, message",
        description: "send to one member",
        detail: "send — Send a direct message to a specific team member.\n\
                 \n\
                 fields:\n\
                 \x20 to       (string, required)  agent id of recipient\n\
                 \x20 message  (string, required)  message text\n\
                 \n\
                 example:\n\
                 \x20 jamsession({\"command\": \"send\", \"to\": \"agent-2\", \"message\": \"can you export UserService?\"})\n\
                 \n\
                 response:\n\
                 \x20 {\"delivered\": true}\n\
                 \n\
                 Messages are delivered asynchronously. The recipient sees it on their next turn.",
    },
    CommandSpec {
        name: "post-worklist",
        fields: "item",
        description: "add item to shared worklist",
        detail: "post-worklist — Add an item to the team's shared worklist.\n\
                 \n\
                 fields:\n\
                 \x20 item  (string, required)  the worklist item text\n\
                 \n\
                 example:\n\
                 \x20 jamsession({\"command\": \"post-worklist\", \"item\": \"Refactor auth middleware\"})\n\
                 \n\
                 response:\n\
                 \x20 {\"id\": \"wl-3\", \"items_count\": 5}",
    },
    CommandSpec {
        name: "remove-worklist",
        fields: "id",
        description: "remove worklist item",
        detail: "remove-worklist — Remove an item from the team's shared worklist.\n\
                 \n\
                 fields:\n\
                 \x20 id  (string, required)  id of the worklist item to remove\n\
                 \n\
                 example:\n\
                 \x20 jamsession({\"command\": \"remove-worklist\", \"id\": \"wl-3\"})\n\
                 \n\
                 response:\n\
                 \x20 {\"removed\": true, \"items_count\": 4}",
    },
    CommandSpec {
        name: "show-worklist",
        fields: "—",
        description: "show shared worklist",
        detail: "show-worklist — Show the team's shared worklist.\n\
                 \n\
                 fields:\n\
                 \x20 (none)\n\
                 \n\
                 example:\n\
                 \x20 jamsession({\"command\": \"show-worklist\"})\n\
                 \n\
                 response:\n\
                 \x20 {\"items\": [{\"id\": \"wl-1\", \"item\": \"...\", \"posted_by\": \"agent-1\"}]}",
    },
    CommandSpec {
        name: "store",
        fields: "key, value",
        description: "store a key-value pair",
        detail: "store — Store a key-value pair in the team's shared store.\n\
                 \n\
                 fields:\n\
                 \x20 key    (string, required)   the key\n\
                 \x20 value  (any JSON, required)  the value to store\n\
                 \n\
                 example:\n\
                 \x20 jamsession({\"command\": \"store\", \"key\": \"api-base-url\", \"value\": \"http://localhost:3000\"})\n\
                 \n\
                 response:\n\
                 \x20 {\"stored\": true}",
    },
    CommandSpec {
        name: "retrieve",
        fields: "key",
        description: "retrieve a stored value",
        detail: "retrieve — Retrieve a value from the team's shared store.\n\
                 \n\
                 fields:\n\
                 \x20 key  (string, required)  the key to retrieve\n\
                 \n\
                 example:\n\
                 \x20 jamsession({\"command\": \"retrieve\", \"key\": \"api-base-url\"})\n\
                 \n\
                 response:\n\
                 \x20 {\"key\": \"api-base-url\", \"value\": \"http://localhost:3000\"}\n\
                 \n\
                 If the key does not exist:\n\
                 \x20 {\"error\": \"key not found\", \"key\": \"api-base-url\"}",
    },
];

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Parse a raw tool input value and dispatch it, returning a JSON response.
///
/// Unknown commands and malformed input produce a structured error object with
/// a `hint` pointing at `help`, rather than surfacing a serde error.
pub fn dispatch_json(input: Value) -> Value {
    let Some(command) = input
        .get("command")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return unknown_command_error("<missing>");
    };

    if !COMMANDS.iter().any(|spec| spec.name == command) {
        return unknown_command_error(&command);
    }

    match serde_json::from_value::<JamsessionCommand>(input) {
        Ok(cmd) => dispatch(cmd),
        Err(err) => json!({
            "error": format!("invalid arguments for command: {command}"),
            "detail": err.to_string(),
            "hint": format!("Run {{\"command\": \"help\", \"subcommand\": \"{command}\"}} for usage."),
        }),
    }
}

/// Dispatch a parsed command.
///
/// At this stage only `help` is live. All team commands require membership,
/// which does not exist yet, so they report that uniformly.
fn dispatch(cmd: JamsessionCommand) -> Value {
    match cmd {
        JamsessionCommand::Help { subcommand } => help(subcommand.as_deref()),
        JamsessionCommand::ListMembers
        | JamsessionCommand::Broadcast { .. }
        | JamsessionCommand::Send { .. }
        | JamsessionCommand::PostWorklist { .. }
        | JamsessionCommand::RemoveWorklist { .. }
        | JamsessionCommand::ShowWorklist
        | JamsessionCommand::Store { .. }
        | JamsessionCommand::Retrieve { .. } => not_a_team_member_error(),
    }
}

// ---------------------------------------------------------------------------
// Help
// ---------------------------------------------------------------------------

/// Render the response for a `help` command.
///
/// With no subcommand, returns the full command table. With a subcommand,
/// returns that command's detail (or an unknown-command error).
fn help(subcommand: Option<&str>) -> Value {
    match subcommand {
        None => Value::String(general_help()),
        Some(name) => match COMMANDS.iter().find(|spec| spec.name == name) {
            Some(spec) => Value::String(spec.detail.to_string()),
            None => unknown_command_error(name),
        },
    }
}

/// The full command table plus usage and examples.
fn general_help() -> String {
    let name_width = COMMANDS
        .iter()
        .map(|c| c.name.chars().count())
        .chain(std::iter::once("command".len()))
        .max()
        .unwrap_or(0);
    let fields_width = COMMANDS
        .iter()
        .map(|c| c.fields.chars().count())
        .chain(std::iter::once("fields".len()))
        .max()
        .unwrap_or(0);

    let mut out = String::new();
    out.push_str("jamsession commands\n\n");

    let header = format!(
        "{:<name_width$}  {:<fields_width$}  {}",
        "command", "fields", "description",
    );
    let rule = "─".repeat(header.chars().count());
    out.push_str(&header);
    out.push('\n');
    out.push_str(&rule);
    out.push('\n');

    for spec in COMMANDS {
        out.push_str(&format!(
            "{:<name_width$}  {:<fields_width$}  {}\n",
            spec.name, spec.fields, spec.description,
        ));
    }

    out.push_str(
        "\n\
         usage: jamsession({\"command\": \"<cmd>\", ...fields})\n\
         \n\
         examples:\n\
         \x20 jamsession({\"command\": \"send\", \"to\": \"agent-2\", \"message\": \"done with header\"})\n\
         \x20 jamsession({\"command\": \"store\", \"key\": \"status\", \"value\": \"blocked on API\"})\n\
         \n\
         note: team commands require membership. Ask your user to run\n\
         /jamsession:join-team if you are not yet on a team.",
    );

    out
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Error returned for an unrecognized command name.
fn unknown_command_error(command: &str) -> Value {
    json!({
        "error": format!("unknown command: {command}"),
        "hint": "Run {\"command\": \"help\"} to see available commands.",
    })
}

/// Error returned when a team command is invoked without team membership.
fn not_a_team_member_error() -> Value {
    json!({
        "error": "not a team member",
        "hint": "Ask your user to run /jamsession:join-team.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use expect_test::expect;

    #[test]
    fn general_help_lists_every_command() {
        let response = dispatch_json(json!({"command": "help"}));
        let text = response.as_str().expect("help returns a string");

        // Every command name appears in the table.
        for spec in COMMANDS {
            assert!(
                text.contains(spec.name),
                "help table missing command {}",
                spec.name
            );
        }

        expect![[r#"
            jamsession commands

            command          fields        description
            ──────────────────────────────────────────
            help             [subcommand]  show this help, or detail on one command
            list-members     —             list team members and status
            broadcast        message       send to all members
            send             to, message   send to one member
            post-worklist    item          add item to shared worklist
            remove-worklist  id            remove worklist item
            show-worklist    —             show shared worklist
            store            key, value    store a key-value pair
            retrieve         key           retrieve a stored value

            usage: jamsession({"command": "<cmd>", ...fields})

            examples:
              jamsession({"command": "send", "to": "agent-2", "message": "done with header"})
              jamsession({"command": "store", "key": "status", "value": "blocked on API"})

            note: team commands require membership. Ask your user to run
            /jamsession:join-team if you are not yet on a team."#]]
        .assert_eq(text);
    }

    #[test]
    fn help_send_returns_detail() {
        let response = dispatch_json(json!({"command": "help", "subcommand": "send"}));
        let text = response.as_str().expect("help detail returns a string");

        expect![[r#"
            send — Send a direct message to a specific team member.

            fields:
              to       (string, required)  agent id of recipient
              message  (string, required)  message text

            example:
              jamsession({"command": "send", "to": "agent-2", "message": "can you export UserService?"})

            response:
              {"delivered": true}

            Messages are delivered asynchronously. The recipient sees it on their next turn."#]]
        .assert_eq(text);
    }

    #[test]
    fn unknown_command_returns_error_and_hint() {
        let response = dispatch_json(json!({"command": "frobnicate"}));
        expect![[r#"
            {
              "error": "unknown command: frobnicate",
              "hint": "Run {\"command\": \"help\"} to see available commands."
            }"#]]
        .assert_eq(&serde_json::to_string_pretty(&response).unwrap());
    }

    #[test]
    fn help_unknown_subcommand_returns_error() {
        let response = dispatch_json(json!({"command": "help", "subcommand": "frobnicate"}));
        assert_eq!(
            response.get("error").and_then(Value::as_str),
            Some("unknown command: frobnicate")
        );
    }

    #[test]
    fn team_command_without_membership_reports_not_a_member() {
        let response = dispatch_json(json!({"command": "send", "to": "agent-2", "message": "hi"}));
        expect![[r#"
            {
              "error": "not a team member",
              "hint": "Ask your user to run /jamsession:join-team."
            }"#]]
        .assert_eq(&serde_json::to_string_pretty(&response).unwrap());
    }

    #[test]
    fn every_team_command_requires_membership() {
        for input in [
            json!({"command": "list-members"}),
            json!({"command": "broadcast", "message": "x"}),
            json!({"command": "send", "to": "a", "message": "x"}),
            json!({"command": "post-worklist", "item": "x"}),
            json!({"command": "remove-worklist", "id": "wl-1"}),
            json!({"command": "show-worklist"}),
            json!({"command": "store", "key": "k", "value": 1}),
            json!({"command": "retrieve", "key": "k"}),
        ] {
            let response = dispatch_json(input.clone());
            assert_eq!(
                response.get("error").and_then(Value::as_str),
                Some("not a team member"),
                "unexpected response for {input}"
            );
        }
    }

    #[test]
    fn malformed_known_command_reports_invalid_arguments() {
        // `send` requires `to` and `message`.
        let response = dispatch_json(json!({"command": "send", "to": "agent-2"}));
        assert_eq!(
            response.get("error").and_then(Value::as_str),
            Some("invalid arguments for command: send")
        );
        assert!(response.get("hint").is_some());
    }
}
