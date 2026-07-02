use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Duration, NaiveTime, Utc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::db::{Store, TraceDirection, TraceQuery};

#[derive(Debug, Clone, Default)]
pub struct DebugFilters {
    pub session_id: Option<String>,
    pub since: Option<DateTime<Utc>>,
}

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
        "/" | "/index.html" => {
            write_response(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                VIEWER_HTML,
            )
            .await?;
        }
        "/api/traces" => {
            let query = trace_query_from_params(parse_query(query), filters)?;
            let traces = store.traces(query).await?;
            let body = serde_json::to_string(&serde_json::json!({ "traces": traces }))?;
            write_response(&mut stream, "200 OK", "application/json", &body).await?;
        }
        _ => {
            write_response(&mut stream, "404 Not Found", "text/plain", "not found").await?;
        }
    }

    Ok(())
}

async fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> crate::error::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    Ok(())
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

static VIEWER_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>jamsession trace</title>
<style>
:root { color-scheme: dark; font-family: ui-sans-serif, system-ui, sans-serif; background: #111318; color: #d8dee9; }
body { margin: 0; }
header { display: flex; gap: 12px; align-items: center; padding: 12px 16px; border-bottom: 1px solid #2b303b; background: #171a21; position: sticky; top: 0; z-index: 1; flex-wrap: wrap; }
h1 { font-size: 16px; margin: 0 12px 0 0; font-weight: 650; }
input, select { background: #0f1117; color: #d8dee9; border: 1px solid #3a4050; border-radius: 4px; padding: 6px 8px; }
button { background: #335c9f; color: white; border: 0; border-radius: 4px; padding: 7px 10px; min-width: 34px; }
label { display: inline-flex; gap: 6px; align-items: center; color: #b6c2d2; font-size: 13px; }
main { padding: 16px; }
#diagram { width: 100%; min-height: 360px; }
.trace-layout { display: grid; grid-template-columns: minmax(0, 1fr) minmax(300px, 34vw); gap: 16px; align-items: start; }
#graph-panel { min-width: 0; border-bottom: 1px solid #2b303b; }
#detail { position: sticky; top: 72px; max-height: calc(100vh - 92px); overflow: auto; border-left: 1px solid #2b303b; padding-left: 16px; }
#detail h2 { margin: 0 0 10px; font-size: var(--detail-heading-font, 14px); font-weight: 650; color: #d8dee9; }
#detail-meta { display: grid; grid-template-columns: var(--detail-label-col, 84px) 1fr; gap: var(--detail-gap-y, 6px) var(--detail-gap-x, 10px); margin-bottom: 12px; color: #b6c2d2; font-size: var(--detail-font, 12px); }
#detail-meta div:nth-child(odd) { color: #8f98aa; text-transform: uppercase; font-size: var(--detail-label-font, 11px); }
#detail-payload { display: block; margin: 0; max-height: none; color: #d8dee9; background: #0f1117; border: 1px solid #2b303b; border-radius: 4px; padding: 10px; }
.request { color: #80bfff; }
.response { color: #ffd479; }
.notification { color: #8ce99a; }
.event { color: #adb5bd; }
pre { display: none; margin: 8px 0 0; white-space: pre-wrap; overflow-wrap: anywhere; color: #b6c2d2; font-size: var(--payload-font, 12px); }
.lane { stroke: #2b303b; stroke-width: 1; }
.arrow { stroke-width: 2; fill: none; }
.event-dot { stroke-width: 2; fill: #171a21; }
.trace-mark { cursor: pointer; }
.trace-mark.correlated { opacity: .55; }
.trace-mark.selected { opacity: 1; }
.trace-mark.selected .label { font-weight: 700; }
.trace-hit { stroke: transparent; fill: none; pointer-events: stroke; }
.label { fill: #d8dee9; font-size: var(--svg-label-font, 12px); }
.quote-label { font-style: italic; }
.meta { fill: #8f98aa; font-size: var(--svg-meta-font, 11px); }
@media (max-width: 900px) {
  .trace-layout { grid-template-columns: 1fr; }
  #detail { position: static; max-height: none; border-left: 0; padding-left: 0; border-top: 1px solid #2b303b; padding-top: 12px; }
}
</style>
</head>
<body>
<header>
<h1>jamsession trace</h1>
<input id="session" placeholder="session">
<input id="method" placeholder="method">
<select id="dir">
<option value="">any direction</option>
<option>client_to_daemon</option>
<option>daemon_to_agent</option>
<option>agent_to_daemon</option>
<option>daemon_to_client</option>
<option>internal</option>
</select>
<label><input id="live" type="checkbox" checked> Live</label>
<label><input id="compact" type="checkbox" checked> Compact</label>
<button id="zoom-out" title="Zoom out" aria-label="Zoom out">-</button>
<button id="zoom-reset" title="Reset zoom" aria-label="Reset zoom">100%</button>
<button id="zoom-in" title="Zoom in" aria-label="Zoom in">+</button>
<button id="refresh">Refresh</button>
</header>
<main>
<div class="trace-layout">
<section id="graph-panel">
  <svg id="diagram" viewBox="0 0 760 240" preserveAspectRatio="xMidYMin meet"></svg>
</section>
<aside id="detail">
  <h2>Selected message</h2>
  <div id="detail-meta"></div>
  <pre id="detail-payload">Select a row to inspect the complete payload.</pre>
</aside>
</div>
</main>
<script>
let lastId = 0;
let zoom = 0.78;
let selectedTraceId = null;
let selectedRequestId = null;
const traces = [];
const diagram = document.querySelector("#diagram");
const detailMeta = document.querySelector("#detail-meta");
const detailPayload = document.querySelector("#detail-payload");
const lanes = { "acp-client": 110, "daemon": 380, "agent": 650 };
const palette = ["#80bfff", "#ffd479", "#8ce99a", "#ff8787", "#b197fc", "#66d9e8"];
function clamp(n, min, max) {
  return Math.min(max, Math.max(min, n));
}
function compactEnabled() {
  return document.querySelector("#compact").checked;
}
function rowStep() {
  return (compactEnabled() ? 22 : 32) * zoom;
}
function applyZoom(next) {
  zoom = clamp(next, 0.45, 1.6);
  document.documentElement.style.setProperty("--zoom", zoom);
  document.documentElement.style.setProperty("--payload-font", `${12 * zoom}px`);
  document.documentElement.style.setProperty("--detail-heading-font", `${14 * zoom}px`);
  document.documentElement.style.setProperty("--detail-font", `${12 * zoom}px`);
  document.documentElement.style.setProperty("--detail-label-font", `${11 * zoom}px`);
  document.documentElement.style.setProperty("--detail-label-col", `${84 * zoom}px`);
  document.documentElement.style.setProperty("--detail-gap-y", `${6 * zoom}px`);
  document.documentElement.style.setProperty("--detail-gap-x", `${10 * zoom}px`);
  document.documentElement.style.setProperty("--svg-label-font", `${12 * zoom}px`);
  document.documentElement.style.setProperty("--svg-meta-font", `${11 * zoom}px`);
  document.querySelector("#zoom-reset").textContent = `${Math.round(zoom * 100)}%`;
  drawSvg();
}
function qs() {
  const p = new URLSearchParams();
  if (lastId) p.set("after_id", lastId);
  for (const id of ["session", "method", "dir"]) {
    const v = document.querySelector("#" + id).value.trim();
    if (v) p.set(id, v);
  }
  return p.toString();
}
function colorFor(trace) {
  const key = trace.request_id ?? trace.method ?? trace.id;
  let hash = 0;
  for (const ch of String(key)) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  return palette[hash % palette.length];
}
function endpoint(trace, source) {
  if (trace.dir === "client_to_daemon") return source ? "acp-client" : "daemon";
  if (trace.dir === "daemon_to_agent") return source ? "daemon" : "agent";
  if (trace.dir === "agent_to_daemon") return source ? "agent" : "daemon";
  if (trace.dir === "daemon_to_client") return source ? "daemon" : "acp-client";
  return trace.role ?? "daemon";
}
function truncate(text, max) {
  if (!text) return "";
  return text.length > max ? text.slice(0, max - 1) + "..." : text;
}
function shortMethod(method) {
  return method ? method.replace(/^session\//, "") : "";
}
function textFrom(value) {
  if (value == null) return "";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (Array.isArray(value)) return value.map(textFrom).filter(Boolean).join(" ");
  if (typeof value !== "object") return "";
  for (const key of ["text", "message", "error"]) {
    if (typeof value[key] === "string") return value[key];
  }
  for (const key of ["update", "content", "prompt", "result", "delta"]) {
    const nested = textFrom(value[key]);
    if (nested) return nested;
  }
  return "";
}
function summarizeTrace(trace) {
  const payload = trace.payload ?? {};
  let summary = "";
  if (trace.method === "session/update") {
    const update = payload.update ?? payload;
    const kind = typeof update.sessionUpdate === "string" ? update.sessionUpdate.replace(/^agent_/, "") : "";
    summary = textFrom(update.content) || textFrom(update);
    return truncate([kind, summary].filter(Boolean).join(": "), 58);
  }
  if (trace.method === "session/prompt") {
    summary = textFrom(payload.prompt) || textFrom(payload);
    return truncate(summary ? `prompt: ${summary}` : "", 58);
  }
  summary = textFrom(payload);
  return truncate(summary, 58);
}
function updatePayload(trace) {
  const payload = trace.payload ?? {};
  return payload.update ?? payload;
}
function isMessageChunk(trace) {
  if (trace.method !== "session/update") return false;
  const update = updatePayload(trace);
  return typeof update.sessionUpdate === "string" && update.sessionUpdate.endsWith("message_chunk");
}
function messageChunkText(trace) {
  if (!isMessageChunk(trace)) return "";
  return textFrom(updatePayload(trace).content);
}
function promptText(trace) {
  if (trace.method !== "session/prompt") return "";
  const payload = trace.payload ?? {};
  return textFrom(payload.prompt) || textFrom(payload);
}
function diagramLabel(trace) {
  const chunk = messageChunkText(trace);
  if (chunk) return `"${truncate(chunk, 56)}"`;
  const prompt = promptText(trace);
  if (prompt) return `"${truncate(prompt, 56)}"`;
  const summary = summarizeTrace(trace);
  const method = shortMethod(trace.method) || trace.kind;
  return summary ? `${method} - ${summary}` : method;
}
function diagramLabelClass(trace) {
  return messageChunkText(trace) || promptText(trace) ? "label quote-label" : "label";
}
function drawSvg() {
  const step = rowStep();
  const height = Math.max(180, 52 + traces.length * step);
  diagram.setAttribute("viewBox", `0 0 760 ${height}`);
  diagram.textContent = "";
  const defs = document.createElementNS("http://www.w3.org/2000/svg", "defs");
  defs.innerHTML = '<marker id="arrowhead" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M 0 0 L 8 4 L 0 8 z" fill="#8f98aa"></path></marker>';
  diagram.appendChild(defs);
  for (const [name, x] of Object.entries(lanes)) {
    const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
    line.setAttribute("class", "lane");
    line.setAttribute("x1", x);
    line.setAttribute("x2", x);
    line.setAttribute("y1", 34);
    line.setAttribute("y2", height - 16);
    diagram.appendChild(line);
    const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
    text.setAttribute("class", "meta");
    text.setAttribute("x", x);
    text.setAttribute("y", 22);
    text.setAttribute("text-anchor", "middle");
    text.textContent = name;
    diagram.appendChild(text);
  }
  for (const [index, trace] of traces.entries()) {
    const y = 42 + index * step;
    const color = colorFor(trace);
    const group = document.createElementNS("http://www.w3.org/2000/svg", "g");
    const isSelected = selectedTraceId === String(trace.id);
    const isCorrelated = selectedRequestId && trace.request_id === selectedRequestId && !isSelected;
    group.setAttribute("class", `trace-mark ${isSelected ? "selected" : ""} ${isCorrelated ? "correlated" : ""}`);
    group.dataset.traceId = String(trace.id);
    group.addEventListener("click", () => selectTrace(trace));
    if (trace.kind === "event" || trace.dir === "internal") {
      const x = lanes[endpoint(trace, true)] ?? lanes.daemon;
      const hit = document.createElementNS("http://www.w3.org/2000/svg", "circle");
      hit.setAttribute("class", "trace-hit");
      hit.setAttribute("cx", x);
      hit.setAttribute("cy", y);
      hit.setAttribute("r", 12 * zoom);
      hit.setAttribute("stroke-width", 12 * zoom);
      group.appendChild(hit);
      const dot = document.createElementNS("http://www.w3.org/2000/svg", "circle");
      dot.setAttribute("class", "event-dot");
      dot.setAttribute("cx", x);
      dot.setAttribute("cy", y);
      dot.setAttribute("r", 5 * zoom);
      dot.setAttribute("stroke-width", 2 * zoom);
      dot.setAttribute("stroke", color);
      group.appendChild(dot);
      group.appendChild(svgText(x + 10, y + 4, diagramLabel(trace), diagramLabelClass(trace), "start"));
    } else {
      const x1 = lanes[endpoint(trace, true)] ?? lanes.daemon;
      const x2 = lanes[endpoint(trace, false)] ?? lanes.daemon;
      const hit = document.createElementNS("http://www.w3.org/2000/svg", "line");
      hit.setAttribute("class", "trace-hit");
      hit.setAttribute("x1", x1);
      hit.setAttribute("x2", x2);
      hit.setAttribute("y1", y);
      hit.setAttribute("y2", y);
      hit.setAttribute("stroke-width", 12 * zoom);
      group.appendChild(hit);
      const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
      line.setAttribute("class", "arrow");
      line.setAttribute("x1", x1);
      line.setAttribute("x2", x2);
      line.setAttribute("y1", y);
      line.setAttribute("y2", y);
      line.setAttribute("stroke", color);
      line.setAttribute("stroke-width", 2 * zoom);
      line.setAttribute("marker-end", "url(#arrowhead)");
      group.appendChild(line);
      group.appendChild(svgText((x1 + x2) / 2, y - 6, diagramLabel(trace), diagramLabelClass(trace), "middle"));
    }
    group.appendChild(svgText(18, y + 4, String(trace.id), "meta", "start"));
    diagram.appendChild(group);
  }
}
function svgText(x, y, text, cls, anchor) {
  const label = document.createElementNS("http://www.w3.org/2000/svg", "text");
  label.setAttribute("class", cls);
  label.setAttribute("x", x);
  label.setAttribute("y", y);
  label.setAttribute("text-anchor", anchor);
  label.textContent = text;
  return label;
}
function showDetail(trace) {
  const values = [
    ["id", trace.id],
    ["time", new Date(trace.ts).toLocaleString()],
    ["session", trace.session_id ?? ""],
    ["direction", trace.dir],
    ["role", trace.role ?? ""],
    ["kind", trace.kind],
    ["method", trace.method ?? ""],
    ["request", trace.request_id ?? ""],
    ["summary", summarizeTrace(trace)],
  ];
  detailMeta.textContent = "";
  for (const [label, value] of values) {
    const k = document.createElement("div");
    const v = document.createElement("div");
    k.textContent = label;
    v.textContent = value;
    detailMeta.append(k, v);
  }
  detailPayload.textContent = JSON.stringify(trace.payload, null, 2);
}
function selectTrace(trace) {
  selectedTraceId = String(trace.id);
  selectedRequestId = trace.request_id ?? null;
  showDetail(trace);
  drawSvg();
}
function render(trace) {
  lastId = Math.max(lastId, trace.id);
  traces.push(trace);
}
async function load(reset = false) {
  if (reset) {
    lastId = 0;
    traces.length = 0;
    selectedTraceId = null;
    selectedRequestId = null;
    detailMeta.textContent = "";
    detailPayload.textContent = "Select a message in the graph to inspect the complete payload.";
  }
  const r = await fetch("/api/traces?" + qs());
  const data = await r.json();
  data.traces.forEach(render);
  if (reset || data.traces.length > 0) drawSvg();
}
document.querySelector("#refresh").onclick = () => load(true);
document.querySelector("#compact").onchange = () => applyZoom(zoom);
document.querySelector("#zoom-out").onclick = () => applyZoom(zoom - 0.1);
document.querySelector("#zoom-reset").onclick = () => applyZoom(compactEnabled() ? 0.78 : 1);
document.querySelector("#zoom-in").onclick = () => applyZoom(zoom + 0.1);
window.addEventListener("keydown", event => {
  if (!(event.metaKey || event.ctrlKey)) return;
  if (event.key === "+" || event.key === "=") {
    event.preventDefault();
    applyZoom(zoom + 0.1);
  } else if (event.key === "-") {
    event.preventDefault();
    applyZoom(zoom - 0.1);
  } else if (event.key === "0") {
    event.preventDefault();
    applyZoom(compactEnabled() ? 0.78 : 1);
  }
});
setInterval(() => {
  if (document.querySelector("#live").checked) load(false);
}, 200);
applyZoom(zoom);
load(true);
</script>
</body>
</html>
"##;

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
