//! Parsing and rendering for the daemon's `/jamsession:*` slash commands.
//!
//! These commands are driven by the *human*, not the agent. They arrive as an
//! ordinary `session/prompt` whose leading text starts with `/jamsession:`; the
//! daemon intercepts them, updates team state, and replies to the user without
//! ever involving the agent. This module holds the transport-free parsing and
//! message rendering so it can be unit-tested directly; the dispatcher supplies
//! the effects (DB writes, notifications, context injection).

/// The prefix that marks a daemon-handled slash command.
pub const PREFIX: &str = "/jamsession:";

/// A parsed `/jamsession:*` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommand {
    /// `/jamsession:join-team TEAM` — join (creating if new) the named team.
    JoinTeam { team: String },
    /// `/jamsession:leave-team` — leave the current team.
    LeaveTeam,
    /// `/jamsession:teams` — list all active teams and their members.
    Teams,
    /// A `/jamsession:` command that could not be parsed (unknown verb, or
    /// `join-team` with no team name). Carries a help message for the user.
    Invalid { message: String },
}

/// Attempt to parse `text` as a `/jamsession:*` command.
///
/// Returns `None` if `text` is not a jamsession slash command (the daemon
/// should forward it to the agent as a normal prompt). Returns `Some` — possibly
/// [`SlashCommand::Invalid`] — when the text is addressed to jamsession.
pub fn parse(text: &str) -> Option<SlashCommand> {
    let rest = text.trim().strip_prefix(PREFIX)?;

    let mut parts = rest.split_whitespace();
    let verb = parts.next().unwrap_or("");
    let arg = parts.next();

    let command = match verb {
        "join-team" => match arg {
            Some(team) => SlashCommand::JoinTeam {
                team: team.to_string(),
            },
            None => SlashCommand::Invalid {
                message: missing_team_message(),
            },
        },
        "leave-team" => SlashCommand::LeaveTeam,
        "teams" => SlashCommand::Teams,
        other => SlashCommand::Invalid {
            message: unknown_verb_message(other),
        },
    };

    Some(command)
}

/// Help text shown when `join-team` is invoked without a team name.
///
/// `available` is the list of currently-active teams (may be empty).
pub fn missing_team_message_with_teams(available: &[String]) -> String {
    let mut msg = String::from(
        "Usage: /jamsession:join-team $TEAM\n\nProvide a team name to join or create.",
    );
    if !available.is_empty() {
        msg.push_str("\n\nActive teams: ");
        msg.push_str(&available.join(", "));
    }
    msg
}

fn missing_team_message() -> String {
    missing_team_message_with_teams(&[])
}

fn unknown_verb_message(verb: &str) -> String {
    format!(
        "Unknown jamsession command: {verb}\n\n\
         Available commands:\n\
         \x20 /jamsession:join-team $TEAM   join (or create) a team\n\
         \x20 /jamsession:leave-team        leave your current team\n\
         \x20 /jamsession:teams             list active teams and members"
    )
}

/// The context injected into the agent's conversation when it joins a team.
///
/// `team` is the joined team; `members` is the full member roster (session ids
/// or display ids), and `me` identifies which member this agent is.
pub fn join_context(team: &str, me: &str, members: &[String]) -> String {
    let roster = members
        .iter()
        .map(|m| {
            if m == me {
                format!("{m} (you)")
            } else {
                m.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "<context>\n\
         You are now a member of the jamsession team \"{team}\".\n\
         Team members: {roster}.\n\
         \n\
         You can now use team commands via the jamsession tool.\n\
         Use {{\"command\": \"help\"}} for the full command table, or\n\
         {{\"command\": \"help\", \"subcommand\": \"send\"}} for details on a specific command.\n\
         </context>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_jamsession_text_is_not_a_command() {
        assert_eq!(parse("hello world"), None);
        assert_eq!(parse("/other:thing"), None);
        assert_eq!(parse("please run /jamsession:teams"), None);
    }

    #[test]
    fn parses_join_team_with_name() {
        assert_eq!(
            parse("/jamsession:join-team frontend"),
            Some(SlashCommand::JoinTeam {
                team: "frontend".to_string()
            })
        );
        // Leading/trailing whitespace is tolerated.
        assert_eq!(
            parse("  /jamsession:join-team   backend  "),
            Some(SlashCommand::JoinTeam {
                team: "backend".to_string()
            })
        );
    }

    #[test]
    fn join_team_without_name_is_invalid_with_help() {
        match parse("/jamsession:join-team") {
            Some(SlashCommand::Invalid { message }) => {
                assert!(message.contains("/jamsession:join-team $TEAM"), "{message}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn parses_leave_team_and_teams() {
        assert_eq!(
            parse("/jamsession:leave-team"),
            Some(SlashCommand::LeaveTeam)
        );
        assert_eq!(parse("/jamsession:teams"), Some(SlashCommand::Teams));
    }

    #[test]
    fn unknown_verb_is_invalid_with_help() {
        match parse("/jamsession:frobnicate") {
            Some(SlashCommand::Invalid { message }) => {
                assert!(
                    message.contains("Unknown jamsession command: frobnicate"),
                    "{message}"
                );
                assert!(message.contains("/jamsession:join-team"), "{message}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn join_context_marks_the_joining_agent() {
        let context = join_context(
            "frontend",
            "agent-1",
            &["agent-1".to_string(), "agent-2".to_string()],
        );
        assert!(context.contains("team \"frontend\""), "{context}");
        assert!(context.contains("agent-1 (you)"), "{context}");
        assert!(context.contains("agent-2"), "{context}");
        assert!(!context.contains("agent-2 (you)"), "{context}");
    }
}
