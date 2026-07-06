//! The MCP-facing `jamsession` tool.
//!
//! This is the thin transport shim between an agent's MCP tool call and the
//! daemon's central dispatcher. It owns no command logic of its own: it ships
//! the raw JSON input to the dispatcher (tagged with the calling agent's id)
//! and returns whatever JSON the dispatcher produces.
//!
//! The tool is served over MCP-over-ACP. See [`crate::jamsession_tool`] and the
//! dispatcher's agent-pipe wiring for how it is attached to a session.

use agent_client_protocol::Error as AcpError;
use agent_client_protocol::mcp_server::{McpConnectionTo, McpTool};
use agent_client_protocol::role::Role;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

/// A request from an agent's tool call, sent to the dispatcher for handling.
///
/// The dispatcher looks up the calling agent's team (via `agent_id`), runs the
/// command, and returns the JSON response over `respond`.
pub struct JamsessionToolCall {
    /// The dispatcher-assigned id of the agent that invoked the tool.
    pub agent_id: u64,
    /// The raw JSON input the agent passed to the tool.
    pub input: serde_json::Value,
    /// Channel on which the dispatcher returns the JSON response.
    pub respond: oneshot::Sender<serde_json::Value>,
}

/// Sink for tool calls. The dispatcher owns the receiving half.
pub type ToolCallSender = tokio::sync::mpsc::UnboundedSender<JamsessionToolCall>;

/// The whole tool input is a single flat JSON object (`{"command": ..., ...}`),
/// so the input type is just an opaque JSON value.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct ToolInput(pub serde_json::Value);

/// The tool output is likewise an arbitrary JSON value (a string for `help`, an
/// object for command results and errors). Its schema is deliberately not an
/// object, so the MCP layer returns it as unstructured text that the agent
/// parses back to the appropriate value.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct ToolOutput(pub serde_json::Value);

/// The `jamsession` MCP tool, bound to one agent connection.
///
/// Implements [`McpTool`] directly (rather than via `tool_fn`) so `call_tool`
/// runs inline on the MCP handler and needs no separate responder task.
pub struct JamsessionTool {
    agent_id: u64,
    tool_calls: ToolCallSender,
}

impl JamsessionTool {
    /// The tool name, as registered with the MCP server and seen by the agent.
    pub const NAME: &'static str = "jamsession";

    /// The tool description; intentionally terse, and lists the commands so the
    /// agent sees the menu at zero call cost. See the jamsession-tool RFD.
    pub const DESCRIPTION: &'static str = "Interface to the jamsession daemon. \
        Commands: help, list-members, broadcast, send, post-worklist, \
        remove-worklist, show-worklist, store, retrieve. \
        Use {\"command\":\"help\"} for usage or \
        {\"command\":\"help\",\"subcommand\":\"send\"} for details on a command.";

    /// Create a tool bound to `agent_id`, forwarding calls to `tool_calls`.
    pub fn new(agent_id: u64, tool_calls: ToolCallSender) -> Self {
        Self {
            agent_id,
            tool_calls,
        }
    }
}

impl<R: Role> McpTool<R> for JamsessionTool {
    type Input = ToolInput;
    type Output = ToolOutput;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        Self::DESCRIPTION.to_string()
    }

    async fn call_tool(
        &self,
        input: ToolInput,
        _context: McpConnectionTo<R>,
    ) -> Result<ToolOutput, AcpError> {
        let (respond, response_rx) = oneshot::channel();
        self.tool_calls
            .send(JamsessionToolCall {
                agent_id: self.agent_id,
                input: input.0,
                respond,
            })
            .map_err(|_| {
                AcpError::internal_error().data("jamsession dispatcher is not accepting tool calls")
            })?;

        let response = response_rx.await.map_err(|_| {
            AcpError::internal_error().data("jamsession dispatcher dropped the tool call")
        })?;

        Ok(ToolOutput(response))
    }
}
