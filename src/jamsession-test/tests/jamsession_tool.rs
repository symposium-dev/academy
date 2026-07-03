//! Integration tests for the `jamsession` MCP tool over MCP-over-ACP.
//!
//! The daemon serves the tool to the agent; the (scripted) agent invokes it via
//! `mcp::call_tool("jamsession", "jamsession", #{...})` and echoes the result
//! back with `say(...)`, which the client reads as the prompt response.

use jamsession_test::{TestDaemon, TestDaemonConfig};

/// Build an agent script that calls the tool with `command_literal` (a Rhai map
/// literal), binds the result to `result`, and says `say_expr`.
///
/// A tool response that is a JSON *string* (e.g. `help`) arrives as a Rhai
/// string; a JSON *object* (e.g. an error) arrives as a Rhai map whose fields
/// are accessed directly — so callers pick a `say_expr` to match.
fn agent_script(command_literal: &str, say_expr: &str) -> String {
    format!(
        r#"
        receive_prompt();
        let result = mcp::call_tool("jamsession", "jamsession", {command_literal});
        say({say_expr});
        "#
    )
}

async fn call_tool(command_literal: &str, say_expr: &str) -> String {
    let daemon = TestDaemon::start(TestDaemonConfig {
        serve_jamsession_tool: true,
        agent_script: agent_script(command_literal, say_expr),
        ..Default::default()
    })
    .await;

    daemon
        .execute_client(
            r#"
        let s = start_session();
        s.prompt("go")
    "#,
        )
        .await
}

#[tokio::test]
async fn list_members_after_join_shows_the_session() {
    // The agent acks its first prompt, then on its next prompt (the injected
    // join context) calls list-members and records the roster to a file.
    let cwd = tempfile::tempdir().unwrap();
    let recorded_path = cwd.path().join("members.txt");

    let agent = r#"
        receive_prompt();
        say("ack");
        loop {
            let m = receive_prompt();
            if m != "" {
                let r = mcp::call_tool("jamsession", "jamsession", #{ command: "list-members" });
                let ids = "";
                for member in r.members {
                    ids += member.id + "@" + member.status + ";";
                }
                write_file(cwd() + "/members.txt", ids);
            }
        }
    "#;

    let daemon = TestDaemon::start(TestDaemonConfig {
        serve_jamsession_tool: true,
        agent_script: agent.to_string(),
        ..Default::default()
    })
    .await;

    daemon
        .execute_client_with_cwd(
            r#"
        let s = start_session();
        s.prompt("hello");
        s.prompt("/jamsession:join-team frontend");
        ""
    "#,
            cwd.path(),
        )
        .await;

    let recorded = poll_file(&recorded_path, std::time::Duration::from_secs(2)).await;
    // The joining session is the sole, active member.
    assert!(
        recorded.contains("@active;"),
        "expected an active member, got: {recorded:?}"
    );
    assert_eq!(
        recorded.matches(';').count(),
        1,
        "expected exactly one member, got: {recorded:?}"
    );
}

/// Poll `path` until non-empty or `timeout` elapses.
async fn poll_file(path: &std::path::Path, timeout: std::time::Duration) -> String {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Ok(c) = std::fs::read_to_string(path)
            && !c.is_empty()
        {
            return c;
        }
        if tokio::time::Instant::now() >= deadline {
            return std::fs::read_to_string(path).unwrap_or_default();
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn help_command_returns_command_table() {
    // `help` returns a JSON string, so `result` is a Rhai string.
    let output = call_tool(r#"#{ command: "help" }"#, "result").await;

    assert!(
        output.contains("jamsession commands"),
        "expected help table, got: {output}"
    );
    // A few representative commands from the table.
    for cmd in ["help", "list-members", "send", "store", "retrieve"] {
        assert!(output.contains(cmd), "help missing {cmd}: {output}");
    }
    assert!(
        output.contains("/jamsession:join-team"),
        "help missing membership note: {output}"
    );
}

#[tokio::test]
async fn help_send_returns_detail() {
    let output = call_tool(r#"#{ command: "help", subcommand: "send" }"#, "result").await;

    assert!(
        output.contains("send — Send a direct message"),
        "expected send detail, got: {output}"
    );
}

#[tokio::test]
async fn unknown_command_returns_error_and_hint() {
    // Errors are JSON objects, so `result` is a Rhai map; read its fields.
    let output = call_tool(
        r#"#{ command: "frobnicate" }"#,
        r#"result.error + " | " + result.hint"#,
    )
    .await;

    assert!(
        output.contains("unknown command: frobnicate"),
        "expected unknown-command error, got: {output}"
    );
    assert!(
        output.contains("help"),
        "expected hint referencing help, got: {output}"
    );
}

#[tokio::test]
async fn team_command_without_membership_reports_not_a_member() {
    let output = call_tool(
        r#"#{ command: "send", to: "agent-2", message: "hi" }"#,
        "result.error",
    )
    .await;

    assert!(
        output.contains("not a team member"),
        "expected not-a-member error, got: {output}"
    );
}
