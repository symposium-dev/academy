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
