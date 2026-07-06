//! Integration tests for the worklist and key-value store commands over the
//! jamsession tool.
//!
//! A single agent joins a team, then on its post-join turn runs a scripted
//! sequence of tool calls and writes the outcome to a file the test reads.

use std::time::Duration;

use jamsession_test::{TestDaemon, TestDaemonConfig};

/// Run `body` (Rhai) on the agent's post-join turn, writing its string result to
/// `<cwd>/out.txt`. `body` may call `mcp::call_tool(...)` and must evaluate to a
/// string.
fn scripted_agent(body: &str) -> String {
    format!(
        r#"
        let me = cwd();
        receive_prompt();
        say("ack");
        loop {{
            let m = receive_prompt();
            if m != "" {{
                let out = {{ {body} }};
                write_file(me + "/out.txt", out);
            }}
        }}
    "#
    )
}

/// Join `frontend`, run `body`, and return what the agent wrote to out.txt.
async fn run_after_join(body: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.txt");

    let daemon = TestDaemon::start(TestDaemonConfig {
        serve_jamsession_tool: true,
        agent_script: scripted_agent(body),
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

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(c) = std::fs::read_to_string(&out)
            && !c.is_empty()
        {
            return c;
        }
        if tokio::time::Instant::now() >= deadline {
            return std::fs::read_to_string(&out).unwrap_or_default();
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn worklist_post_show_remove_roundtrip() {
    // Post two items, show, remove the first, then show again — record the
    // key facts as a single string.
    let body = r#"
        let a = mcp::call_tool("jamsession","jamsession", #{ command:"post-worklist", item:"fixtures" });
        let b = mcp::call_tool("jamsession","jamsession", #{ command:"post-worklist", item:"api" });
        let shown = mcp::call_tool("jamsession","jamsession", #{ command:"show-worklist" });
        let rm = mcp::call_tool("jamsession","jamsession", #{ command:"remove-worklist", id: a.id });
        let after = mcp::call_tool("jamsession","jamsession", #{ command:"show-worklist" });
        "posted=" + a.id + "," + a.items_count.to_string() +
        " shown=" + shown.items.len().to_string() +
        " removed=" + rm.removed.to_string() + "," + rm.items_count.to_string() +
        " after=" + after.items.len().to_string() + "," + after.items[0].item
    "#;

    let out = run_after_join(body).await;
    assert_eq!(
        out, "posted=wl-1,1 shown=2 removed=true,1 after=1,api",
        "unexpected worklist roundtrip: {out}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn remove_unknown_worklist_id_errors() {
    let body = r#"
        let r = mcp::call_tool("jamsession","jamsession", #{ command:"remove-worklist", id:"wl-999" });
        "error=" + r.error
    "#;
    let out = run_after_join(body).await;
    assert!(out.contains("unknown worklist id"), "got: {out}");
}

#[tokio::test(flavor = "multi_thread")]
async fn store_and_retrieve_roundtrip() {
    // Store a string and an object, retrieve both, retrieve a missing key.
    let body = r#"
        mcp::call_tool("jamsession","jamsession", #{ command:"store", key:"url", value:"http://a" });
        mcp::call_tool("jamsession","jamsession", #{ command:"store", key:"cfg", value: #{ port: 3000 } });
        let s1 = mcp::call_tool("jamsession","jamsession", #{ command:"retrieve", key:"url" });
        let s2 = mcp::call_tool("jamsession","jamsession", #{ command:"retrieve", key:"cfg" });
        let miss = mcp::call_tool("jamsession","jamsession", #{ command:"retrieve", key:"nope" });
        "url=" + s1.value + " port=" + s2.value.port.to_string() + " miss=" + miss.error
    "#;
    let out = run_after_join(body).await;
    assert_eq!(
        out, "url=http://a port=3000 miss=key not found",
        "unexpected store roundtrip: {out}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn store_overwrite_replaces_value() {
    let body = r#"
        mcp::call_tool("jamsession","jamsession", #{ command:"store", key:"k", value:"first" });
        mcp::call_tool("jamsession","jamsession", #{ command:"store", key:"k", value:"second" });
        let r = mcp::call_tool("jamsession","jamsession", #{ command:"retrieve", key:"k" });
        "value=" + r.value
    "#;
    let out = run_after_join(body).await;
    assert_eq!(out, "value=second", "expected overwrite; got: {out}");
}
