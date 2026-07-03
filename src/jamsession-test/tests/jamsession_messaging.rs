//! Integration tests for `send` and `broadcast` over the jamsession tool.
//!
//! The test harness runs one agent script per daemon, so the script is
//! *polymorphic*: it branches on `cwd()` (distinct per session, controlled by
//! the test) to act as either a sender or a recipient. A sender broadcasts on
//! its post-join turn; a recipient records every injected prompt to
//! `<cwd>/inbox.txt` so the test can assert what it received.

use std::time::Duration;

use jamsession_test::{TestDaemon, TestDaemonConfig};

/// One script, two roles keyed on cwd:
/// - a cwd containing "sender": broadcast once on the post-join turn;
/// - otherwise (recipient): append every injected prompt to `<cwd>/inbox.txt`.
fn polymorphic_agent() -> String {
    r#"
        let me = cwd();
        let is_sender = me.contains("sender");
        receive_prompt();
        say("ack");
        let inbox = "";
        loop {
            let m = receive_prompt();
            if m != "" {
                if is_sender {
                    mcp::call_tool("jamsession", "jamsession",
                        #{ command: "broadcast", message: "auth done" });
                } else {
                    inbox += m + "\n----\n";
                    write_file(me + "/inbox.txt", inbox);
                }
            }
        }
    "#
    .to_string()
}

/// Poll `path` until it contains `needle` or `timeout` elapses; returns contents.
async fn wait_for_contains(path: &std::path::Path, needle: &str, timeout: Duration) -> String {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let contents = std::fs::read_to_string(path).unwrap_or_default();
        if contents.contains(needle) {
            return contents;
        }
        if tokio::time::Instant::now() >= deadline {
            return contents;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// A recipient joins a team and stays live (its agent loops); a sender then
/// joins the same team and broadcasts. The recipient should receive the
/// `<team-message>` by live injection.
#[tokio::test(flavor = "multi_thread")]
async fn broadcast_reaches_live_team_peer() {
    let recipient_dir = tempfile::Builder::new()
        .prefix("recipient-")
        .tempdir()
        .unwrap();
    let sender_dir = tempfile::Builder::new()
        .prefix("sender-")
        .tempdir()
        .unwrap();
    let inbox = recipient_dir.path().join("inbox.txt");

    let daemon = TestDaemon::start(TestDaemonConfig {
        serve_jamsession_tool: true,
        agent_script: polymorphic_agent(),
        ..Default::default()
    })
    .await;

    // Recipient joins first; its client disconnects but the agent stays live
    // (idle timeout is long by default), so it can still receive injections.
    daemon
        .execute_client_with_cwd(
            r#"
        let s = start_session();
        s.prompt("hello");
        s.prompt("/jamsession:join-team frontend");
        ""
    "#,
            recipient_dir.path(),
        )
        .await;

    // Sender joins the same team; on its post-join turn it broadcasts.
    daemon
        .execute_client_with_cwd(
            r#"
        let s = start_session();
        s.prompt("hello");
        s.prompt("/jamsession:join-team frontend");
        ""
    "#,
            sender_dir.path(),
        )
        .await;

    let received = wait_for_contains(&inbox, "auth done", Duration::from_secs(3)).await;
    assert!(
        received.contains("<team-message"),
        "recipient did not receive a team message; inbox: {received:?}"
    );
    assert!(
        received.contains("type=\"broadcast\""),
        "expected a broadcast message; inbox: {received:?}"
    );
    assert!(
        received.contains("auth done"),
        "broadcast body missing; inbox: {received:?}"
    );
}

/// `send` to a non-member returns a structured `unknown agent` error.
#[tokio::test(flavor = "multi_thread")]
async fn send_to_unknown_agent_errors() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.txt");

    // Agent joins a team, then on its post-join turn sends to a bogus id and
    // records the tool's error field.
    let agent = r#"
        let me = cwd();
        receive_prompt();
        say("ack");
        loop {
            let m = receive_prompt();
            if m != "" {
                let r = mcp::call_tool("jamsession", "jamsession",
                    #{ command: "send", to: "nobody", message: "hi" });
                write_file(me + "/out.txt", r.error + "|" + r.agent);
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
            dir.path(),
        )
        .await;

    let recorded = wait_for_contains(&out, "unknown agent", Duration::from_secs(3)).await;
    assert!(
        recorded.contains("unknown agent") && recorded.contains("nobody"),
        "expected unknown-agent error, got: {recorded:?}"
    );
}

/// A message broadcast while the recipient's agent is dead is queued and then
/// delivered when the recipient's session is reloaded (agent respawns).
#[tokio::test(flavor = "multi_thread")]
async fn broadcast_is_queued_then_delivered_after_respawn() {
    let recipient_dir = tempfile::Builder::new()
        .prefix("recipient-")
        .tempdir()
        .unwrap();
    let sender_dir = tempfile::Builder::new()
        .prefix("sender-")
        .tempdir()
        .unwrap();
    let inbox = recipient_dir.path().join("inbox.txt");

    let daemon = TestDaemon::start(TestDaemonConfig {
        serve_jamsession_tool: true,
        idle_timeout: Duration::from_millis(100),
        agent_script: polymorphic_agent(),
        ..Default::default()
    })
    .await;

    // Recipient joins the team, then its agent is killed by the short idle
    // timeout once the client disconnects.
    let recipient_sid = daemon
        .execute_client_with_cwd(
            r#"
        let s = start_session();
        s.prompt("hello");
        s.prompt("/jamsession:join-team frontend");
        s.session_id()
    "#,
            recipient_dir.path(),
        )
        .await;
    // Give the idle timeout time to kill the recipient's agent.
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Sender joins and broadcasts; the recipient is a member but its agent is
    // dead, so the message is queued rather than injected live.
    daemon
        .execute_client_with_cwd(
            r#"
        let s = start_session();
        s.prompt("hello");
        s.prompt("/jamsession:join-team frontend");
        ""
    "#,
            sender_dir.path(),
        )
        .await;
    // Let the broadcast be processed and queued.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Reload the recipient session: the agent respawns and queued messages flush.
    daemon
        .execute_client_with_cwd(
            &format!(
                r#"
        let s = load_session("{recipient_sid}");
        s.prompt("resumed");
        ""
    "#
            ),
            recipient_dir.path(),
        )
        .await;

    let received = wait_for_contains(&inbox, "auth done", Duration::from_secs(3)).await;
    assert!(
        received.contains("<team-message") && received.contains("auth done"),
        "queued broadcast was not delivered after respawn; inbox: {received:?}"
    );
}
