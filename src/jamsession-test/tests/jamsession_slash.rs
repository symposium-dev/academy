//! Integration tests for the daemon's `/jamsession:*` slash commands.
//!
//! Slash commands are handled entirely by the daemon: the client's prompt
//! returns the daemon's reply, and the agent never sees the command. On a
//! successful join the daemon also injects a `<context>` message into the live
//! agent, which the scripted agent surfaces via `receive_prompt()`.

use std::time::Duration;

use jamsession_test::{TestDaemon, TestDaemonConfig};

/// A simple agent that acks its first prompt; used by tests that only assert on
/// the daemon's slash-command replies (which never reach the agent).
fn ack_agent() -> String {
    r#"
        let p = receive_prompt();
        say("ack: " + p);
    "#
    .to_string()
}

/// An agent that records every prompt it receives after the first into a file
/// in its cwd, so the test can assert what the agent did (and did not) see.
/// The path is absolute (`cwd()`), which the test controls via the client cwd.
fn recording_agent() -> String {
    r#"
        let p = receive_prompt();
        say("ack: " + p);
        loop {
            let m = receive_prompt();
            if m != "" {
                write_file(cwd() + "/received.txt", m);
            }
        }
    "#
    .to_string()
}

#[tokio::test]
async fn join_team_replies_and_lists() {
    let daemon = TestDaemon::start(TestDaemonConfig {
        serve_jamsession_tool: true,
        agent_script: ack_agent(),
        ..Default::default()
    })
    .await;

    // Join a team via slash command; the reply comes from the daemon.
    let join_reply = daemon
        .execute_client(
            r#"
        let s = start_session();
        s.prompt("/jamsession:join-team frontend")
    "#,
        )
        .await;

    assert!(
        join_reply.contains("Joined team \"frontend\""),
        "expected join confirmation, got: {join_reply}"
    );
}

#[tokio::test]
async fn teams_lists_active_teams() {
    let daemon = TestDaemon::start(TestDaemonConfig {
        serve_jamsession_tool: true,
        agent_script: ack_agent(),
        ..Default::default()
    })
    .await;

    // One client joins a team, then asks for the team list in the same session.
    let listing = daemon
        .execute_client(
            r#"
        let s = start_session();
        s.prompt("/jamsession:join-team backend");
        s.prompt("/jamsession:teams")
    "#,
        )
        .await;

    assert!(listing.contains("Active teams:"), "got: {listing}");
    assert!(listing.contains("backend"), "got: {listing}");
}

#[tokio::test]
async fn leave_team_without_membership_is_reported() {
    let daemon = TestDaemon::start(TestDaemonConfig {
        serve_jamsession_tool: true,
        agent_script: ack_agent(),
        ..Default::default()
    })
    .await;

    let reply = daemon
        .execute_client(
            r#"
        let s = start_session();
        s.prompt("/jamsession:leave-team")
    "#,
        )
        .await;

    assert!(reply.contains("not on a team"), "got: {reply}");
}

#[tokio::test]
async fn invalid_command_returns_help() {
    let daemon = TestDaemon::start(TestDaemonConfig {
        serve_jamsession_tool: true,
        agent_script: ack_agent(),
        ..Default::default()
    })
    .await;

    let reply = daemon
        .execute_client(
            r#"
        let s = start_session();
        s.prompt("/jamsession:frobnicate")
    "#,
        )
        .await;

    assert!(reply.contains("Unknown jamsession command"), "got: {reply}");
}

#[tokio::test]
async fn join_injects_context_into_agent() {
    let cwd = tempfile::tempdir().unwrap();
    let received_path = cwd.path().join("received.txt");

    let daemon = TestDaemon::start(TestDaemonConfig {
        serve_jamsession_tool: true,
        agent_script: recording_agent(),
        ..Default::default()
    })
    .await;

    // A real prompt first (so the agent's first receive_prompt is consumed),
    // then the slash join which should inject context into the agent.
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

    // Injection is asynchronous; poll briefly for the agent to record it.
    let recorded = read_when_present(&received_path, Duration::from_secs(2)).await;

    assert!(
        recorded.contains("member of the jamsession team \"frontend\""),
        "agent did not receive injected context; recorded: {recorded:?}"
    );
    assert!(
        !recorded.contains("/jamsession:join-team"),
        "agent should not have seen the slash command itself; recorded: {recorded:?}"
    );
}

/// Regression guard for the reply/response ordering race: the slash reply text
/// must always reach the client before the terminating response, even on a
/// multi-threaded runtime under repetition. Before the fix, the AgentMessageChunk
/// and the PromptResponse raced on separate queues and the reply could be lost.
#[tokio::test(flavor = "multi_thread")]
async fn join_reply_is_never_lost_to_response_race() {
    for _ in 0..20 {
        let daemon = TestDaemon::start(TestDaemonConfig {
            serve_jamsession_tool: true,
            agent_script: ack_agent(),
            ..Default::default()
        })
        .await;

        let reply = daemon
            .execute_client(
                r#"
            let s = start_session();
            s.prompt("/jamsession:join-team frontend")
        "#,
            )
            .await;

        assert!(
            reply.contains("Joined team \"frontend\""),
            "reply lost to race; got: {reply:?}"
        );
    }
}

/// Poll `path` until it exists and is non-empty, or `timeout` elapses.
async fn read_when_present(path: &std::path::Path, timeout: Duration) -> String {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Ok(contents) = std::fs::read_to_string(path)
            && !contents.is_empty()
        {
            return contents;
        }
        if tokio::time::Instant::now() >= deadline {
            return std::fs::read_to_string(path).unwrap_or_default();
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
