use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Duration, NaiveTime, Utc};
use rust_embed::RustEmbed;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::db::{Store, TraceDirection, TraceQuery};

#[derive(Debug, Clone, Default)]
pub struct DebugFilters {
    pub session_id: Option<String>,
    pub since: Option<DateTime<Utc>>,
}

#[derive(RustEmbed)]
#[folder = "debug-ui/"]
struct DebugAssets;

pub async fn run_debug_server(
    db_path: &Path,
    port: u16,
    filters: DebugFilters,
) -> crate::error::Result<()> {
    let store = Store::open(db_path).await?;
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    eprintln!("jamsession debug listening on http://127.0.0.1:{port}");

    loop {
        let (stream, _) = listener.accept().await?;
        let store = store.clone();
        let filters = filters.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, store, filters).await {
                tracing::debug!(error = %e, "debug connection failed");
            }
        });
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    store: Store,
    filters: DebugFilters,
) -> crate::error::Result<()> {
    let mut buf = vec![0; 8192];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buf[..n]);
    let Some(request_line) = request.lines().next() else {
        return Ok(());
    };
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();

    if method != "GET" {
        write_response(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain",
            "GET only",
        )
        .await?;
        return Ok(());
    }

    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    match path {
        "/" => write_asset_response(&mut stream, "index.html").await?,
        "/index.html" => write_asset_response(&mut stream, "index.html").await?,
        "/api/traces" => {
            let query = trace_query_from_params(parse_query(query), filters)?;
            let traces = store.traces(query).await?;
            let body = serde_json::to_string(&serde_json::json!({ "traces": traces }))?;
            write_response(&mut stream, "200 OK", "application/json", &body).await?;
        }
        _ => match path.strip_prefix('/') {
            Some(asset_path) if !asset_path.is_empty() => {
                if !write_optional_asset_response(&mut stream, asset_path).await? {
                    write_response(&mut stream, "404 Not Found", "text/plain", "not found").await?;
                }
            }
            _ => write_response(&mut stream, "404 Not Found", "text/plain", "not found").await?,
        },
    }

    Ok(())
}

async fn write_asset_response(stream: &mut TcpStream, path: &str) -> crate::error::Result<()> {
    if !write_optional_asset_response(stream, path).await? {
        write_response(stream, "404 Not Found", "text/plain", "not found").await?;
    }
    Ok(())
}

async fn write_optional_asset_response(
    stream: &mut TcpStream,
    path: &str,
) -> crate::error::Result<bool> {
    let Some(asset) = DebugAssets::get(path) else {
        return Ok(false);
    };
    write_response_bytes(stream, "200 OK", content_type(path), asset.data.as_ref()).await?;
    Ok(true)
}

async fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> crate::error::Result<()> {
    write_response_bytes(stream, status, content_type, body.as_bytes()).await
}

async fn write_response_bytes(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> crate::error::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(body).await?;
    Ok(())
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

fn trace_query_from_params(
    params: HashMap<String, String>,
    filters: DebugFilters,
) -> crate::error::Result<TraceQuery> {
    let after_id = params.get("after_id").and_then(|v| v.parse().ok());
    let session_id = params.get("session").cloned().or(filters.session_id);
    let method = params.get("method").filter(|v| !v.is_empty()).cloned();
    let dir = params
        .get("dir")
        .filter(|v| !v.is_empty())
        .map(|v| TraceDirection::parse(v))
        .transpose()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let limit = params.get("limit").and_then(|v| v.parse().ok());
    let since = match params.get("since") {
        Some(since) if !since.is_empty() => Some(parse_since(since)?),
        _ => filters.since,
    };

    Ok(TraceQuery {
        after_id,
        session_id,
        since,
        method,
        dir,
        limit,
    })
}

pub fn parse_since(value: &str) -> crate::error::Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc))
}

pub fn midnight_today_utc() -> DateTime<Utc> {
    Utc::now().date_naive().and_time(NaiveTime::MIN).and_utc()
}

pub fn parse_ago(value: &str) -> crate::error::Result<DateTime<Utc>> {
    let (amount, unit) = value.split_at(value.len().saturating_sub(1));
    let amount: i64 = amount.parse().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid --ago duration {value:?}: {e}"),
        )
    })?;
    let duration = match unit {
        "m" => Duration::minutes(amount),
        "h" => Duration::hours(amount),
        "d" => Duration::days(amount),
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "expected --ago duration ending in m, h, or d",
            )
            .into());
        }
    };
    Ok(Utc::now() - duration)
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            Some((percent_decode(key)?, percent_decode(value)?))
        })
        .collect()
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{NewTrace, TraceKind};

    #[test]
    fn parses_trace_query_params() {
        let query = trace_query_from_params(
            parse_query("after_id=4&session=sess-1&method=session%2Fprompt&dir=daemon_to_agent"),
            DebugFilters::default(),
        )
        .unwrap();

        assert_eq!(query.after_id, Some(4));
        assert_eq!(query.session_id.as_deref(), Some("sess-1"));
        assert_eq!(query.method.as_deref(), Some("session/prompt"));
        assert_eq!(query.dir, Some(TraceDirection::DaemonToAgent));
    }

    #[test]
    fn parses_absolute_since() {
        let since = parse_since("2026-06-30T10:00:00Z").unwrap();
        assert_eq!(since.to_rfc3339(), "2026-06-30T10:00:00+00:00");
    }

    #[test]
    fn embeds_debug_ui_assets() {
        let index = DebugAssets::get("index.html").expect("embedded index.html");
        let body = std::str::from_utf8(index.data.as_ref()).unwrap();

        assert!(body.contains(r#"<link rel="stylesheet" href="/styles.css">"#));
        assert!(body.contains(r#"<script src="/app.js"></script>"#));
        assert_eq!(content_type("styles.css"), "text/css; charset=utf-8");
        assert_eq!(content_type("app.js"), "text/javascript; charset=utf-8");
    }

    #[tokio::test]
    async fn api_traces_returns_filtered_rows() {
        let dir = tempfile::TempDir::new().unwrap();
        let db_path = dir.path().join("jamsession.db");
        let store = Store::open(&db_path).await.unwrap();
        store
            .record_trace(NewTrace {
                session_id: Some("sess-1".to_string()),
                dir: TraceDirection::ClientToDaemon,
                role: Some("acp-client".to_string()),
                kind: TraceKind::Request,
                method: Some("session/prompt".to_string()),
                request_id: Some("1".to_string()),
                payload: serde_json::json!({ "text": "hello" }),
            })
            .await
            .unwrap();
        store
            .record_trace(NewTrace {
                session_id: Some("sess-2".to_string()),
                dir: TraceDirection::Internal,
                role: Some("daemon".to_string()),
                kind: TraceKind::Event,
                method: Some("session_created".to_string()),
                request_id: None,
                payload: serde_json::json!({}),
            })
            .await
            .unwrap();

        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_store = store.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, server_store, DebugFilters::default())
                .await
                .unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /api/traces?session=sess-1 HTTP/1.1\r\nhost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut body = String::new();
        client.read_to_string(&mut body).await.unwrap();
        server.await.unwrap();

        let (_, json) = body.split_once("\r\n\r\n").unwrap();
        let response: serde_json::Value = serde_json::from_str(json).unwrap();
        let traces = response["traces"].as_array().unwrap();
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0]["session_id"], "sess-1");
        assert_eq!(traces[0]["method"], "session/prompt");
    }
}
