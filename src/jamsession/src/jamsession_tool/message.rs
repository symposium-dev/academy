//! Rendering of team messages injected into a recipient agent's conversation.
//!
//! Pure and transport-free: the dispatcher renders a message here and then
//! delivers the text (via live injection or the pending-message queue).

/// The kind of team message, reflected in the `type` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    /// A message sent to one specific member via `send`.
    Direct,
    /// A message sent to all other members via `broadcast`.
    Broadcast,
}

impl MessageKind {
    fn as_str(self) -> &'static str {
        match self {
            MessageKind::Direct => "direct",
            MessageKind::Broadcast => "broadcast",
        }
    }
}

/// Render a team message as it appears to the recipient agent.
///
/// ```text
/// <team-message from="agent-1" type="broadcast">
/// I've finished the auth module.
/// </team-message>
/// ```
pub fn team_message(from: &str, kind: MessageKind, body: &str) -> String {
    format!(
        "<team-message from=\"{from}\" type=\"{}\">\n{body}\n</team-message>",
        kind.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_direct_message() {
        let msg = team_message(
            "agent-1",
            MessageKind::Direct,
            "can you export UserService?",
        );
        assert_eq!(
            msg,
            "<team-message from=\"agent-1\" type=\"direct\">\n\
             can you export UserService?\n\
             </team-message>"
        );
    }

    #[test]
    fn renders_broadcast_message() {
        let msg = team_message("agent-2", MessageKind::Broadcast, "auth module ready");
        assert!(msg.starts_with("<team-message from=\"agent-2\" type=\"broadcast\">\n"));
        assert!(msg.ends_with("\n</team-message>"));
        assert!(msg.contains("auth module ready"));
    }
}
