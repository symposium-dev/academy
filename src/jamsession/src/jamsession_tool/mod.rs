//! The `jamsession` MCP tool: a single CLI-style tool exposing daemon
//! capabilities to agents as JSON subcommands.
//!
//! The command *logic* lives here, decoupled from the ACP/MCP transport, so it
//! can be unit-tested without any plumbing. See [`command`] for the command
//! enum and its dispatch.

pub mod command;
pub mod slash;
pub mod tool;

pub use command::{MemberInfo, TeamContext, dispatch_json};
pub use slash::SlashCommand;
pub use tool::{JamsessionTool, JamsessionToolCall, ToolCallSender};
