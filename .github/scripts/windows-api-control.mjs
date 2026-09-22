// Actual authenticated dispatcher saturation, exclusively inside the disposable
// hosted Windows acceptance app. No product fixture command or authority bypass.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createServer as createHttp2Server } from "node:http2";
import { createHash, randomUUID } from "node:crypto";
import { once } from "node:events";
import { writeFileSync, readFileSync, existsSync } from "node:fs";
import { spawn } from "node:child_process";
import path from "node:path";

const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(check, message, timeout = 8000) {
  const deadline = performance.now() + timeout;
  while (performance.now() < deadline) { if (await check()) return; await pause(25); }
  throw new Error(message);
}
const discover = id => ({ jsonrpc: "2.0", id, result: { resultType: "complete", supportedVersions: ["2026-07-28"], capabilities: { tools: {} }, ttlMs: 60000, cacheScope: "public", _meta: { "io.modelcontextprotocol/serverInfo": { name: "owned fixture", version: "1" } } } });
const template = url => ({ method: "GET", url, headers: [], params: [], cookies: [], multipart: [], body_kind: "none", body: "", auth: null, timeout_ms: 30000 });
function varint(value) { const bytes = []; do { const next = value & 127; value >>>= 7; bytes.push(next | (value ? 128 : 0)); } while (value); return Buffer.from(bytes); }
function field(number, value) { const bytes = Buffer.isBuffer(value) ? value : Buffer.from(value); return Buffer.concat([varint(number * 8 + 2), varint(bytes.length), bytes]); }
const protobuf = (...fields) => Buffer.concat(fields);
function frame(bytes) { const prefix = Buffer.alloc(5); prefix.writeUInt32BE(bytes.length, 1); return Buffer.concat([prefix, bytes]); }
const descriptor = protobuf(field(1, "control.proto"), field(2, "fixture"), field(4, field(1, "Empty")), field(6, protobuf(field(1, "Control"), field(2, protobuf(field(1, "Hold"), field(2, ".fixture.Empty"), field(3, ".fixture.Empty"))))), field(12, "proto3"));

export async function exerciseControlAdmission({ ui, root, call, success, evidence }) {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true"); assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  const loads = new Map(), held = new Map(), sockets = new Set(), h2sessions = new Set();
  const server = createServer(async (request, response) => {
    const url = new URL(request.url, "http://fixture");
    response.on("error", () => {});
    if (url.pathname === "/load") {
      const bucket = loads.get(url.searchParams.get("id"));
      if (!bucket) { response.writeHead(404).end(); return; }
      bucket.push(response); return;
    }
    if (url.pathname === "/sse") {
      response.writeHead(200, { "Content-Type": "text/event-stream" }); response.write(": connected\n\n");
      held.set("sse", response); response.on("close", () => held.delete("sse")); return;
    }
    if (url.pathname === "/http" || url.pathname === "/oauth" || url.pathname.startsWith("/.well-known/")) {
      const key = url.pathname === "/http" ? "http" : "oauth";
      held.set(key, response); response.on("close", () => held.delete(key)); return;
    }
    if (url.pathname === "/mcp") {
      const chunks = []; for await (const chunk of request) chunks.push(chunk);
      const message = JSON.parse(Buffer.concat(chunks));
      if (message.method === "server/discover") { response.setHeader("Content-Type", "application/json"); response.end(JSON.stringify(discover(message.id))); }
      else { held.set("mcp", response); response.on("close", () => held.delete("mcp")); }
      return;
    }
    response.writeHead(404).end();
  });
  server.on("connection", socket => { sockets.add(socket); socket.on("close", () => sockets.delete(socket)); });
  server.on("upgrade", (request, socket) => {
    const accept = createHash("sha1").update(request.headers["sec-websocket-key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest("base64");
    socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`);
    held.set("websocket", socket); socket.on("close", () => held.delete("websocket"));
    socket.on("data", bytes => { if ((bytes[0] & 15) === 8) socket.end(Buffer.from([0x88, 0])); });
  });
  const h2 = createHttp2Server();
  h2.on("session", session => { h2sessions.add(session); session.on("error", () => {}); session.on("close", () => h2sessions.delete(session)); });
  h2.on("stream", (stream, headers) => {
    stream.on("error", () => {});
    if (headers[":path"] === "/fixture.Control/Hold") {
      held.set("grpc", stream); stream.on("close", () => held.delete("grpc")); stream.resume(); return;
    }
    let bytes = Buffer.alloc(0);
    stream.respond({ ":status": 200, "content-type": "application/grpc" }, { waitForTrailers: true });
    stream.on("wantTrailers", () => stream.sendTrailers({ "grpc-status": "0" }));
    stream.on("end", () => stream.end());
    stream.on("data", chunk => {
      bytes = Buffer.concat([bytes, chunk]);
      while (bytes.length >= 5 && bytes.length >= 5 + bytes.readUInt32BE(1)) {
        const length = bytes.readUInt32BE(1), message = bytes.subarray(5, 5 + length);
        bytes = bytes.subarray(5 + length);
        const result = message.includes(Buffer.from([0x3a, 0]))
          ? field(6, field(1, field(1, "fixture.Control"))) : field(4, field(1, descriptor));
        stream.write(frame(protobuf(field(2, message), result)));
      }
    });
  });
  server.listen(0, "127.0.0.1"); h2.listen(0, "127.0.0.1");
  await Promise.all([once(server, "listening"), once(h2, "listening")]);
  const endpoint = `http://127.0.0.1:${server.address().port}`;
  const pending = async (method, args) => {
    const key = randomUUID();
    await ui.cdp.evaluate(`(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const d=await invoke("plugin:product-shell|describe");window.__controlPending??={};const header={protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+5000,route:"requests"};window.__controlPending[${JSON.stringify(key)}]={done:false};void invoke("plugin:api-studio|execute",{request:{header,component:"api-studio.api",method:${JSON.stringify(method)},args:${JSON.stringify(args)}}}).then(value=>{window.__controlPending[${JSON.stringify(key)}]={done:true,value};},error=>{window.__controlPending[${JSON.stringify(key)}]={done:true,error};});})()`);
    return key;
  };
  const completed = async key => { await until(() => ui.cdp.evaluate(`window.__controlPending[${JSON.stringify(key)}].done`), "protocol request did not retire"); return ui.cdp.evaluate(`window.__controlPending[${JSON.stringify(key)}]`); };
  async function saturated(occupied, action) {
    const id = randomUUID(), responses = []; loads.set(id, responses);
    const keys = await Promise.all(Array.from({ length: 64 - occupied }, () => pending("fetch_openapi_source", { url: `${endpoint}/load?id=${id}` })));
    try {
      await until(() => responses.length === keys.length, "normal dispatcher slots did not fill");
      const overflow = await pending("fetch_openapi_source", { url: `${endpoint}/overflow` });
      const blocked = await completed(overflow);
      assert.equal(blocked.error?.code, "overloaded", "normal operation unexpectedly entered the full dispatcher");
      await action();
      // Prove the workload was still holding normal slots when control finished.
      const stillHeld = await ui.cdp.evaluate(`[${keys.map(key => `window.__controlPending[${JSON.stringify(key)}].done`).join(",")}].every(done=>!done)`);
      assert.equal(stillHeld, true, "load expired before control completion");
    } finally {
      for (const response of responses) response.end("openapi: 3.0.3\n");
      loads.delete(id); await Promise.all(keys.map(completed));
    }
  }
  const recorded = [];
  evidence.controlAdmission = { normalSlots: 64, released: recorded, result: "running" };
  async function control(method, args, occupied, released, pendingKey) {
    evidence.controlAdmission.step = method;
    await saturated(occupied, async () => {
      await success("api-studio.api", method, args);
      await until(released, `${method} did not release its owned resource`);
      if (pendingKey) { const result = await completed(pendingKey); assert.ok(!result.error); assert.notEqual(result.value.operation.outcome.state, "succeeded"); }
    });
    recorded.push(method);
  }
  try {
    let key = await pending("send_request", { req: template(`${endpoint}/http`), environment: [], requestId: "control-http" });
    await until(() => held.has("http"), "HTTP did not start");
    await control("cancel_request", { requestId: "control-http" }, 1, () => !held.has("http"), key);
    for (const method of ["cancel_mcp_http", "disconnect_mcp_http"]) {
      const connection = await success("api-studio.api", "connect_mcp_http", { profile: { endpoint: `${endpoint}/mcp`, era: "modern", headers: [], timeoutMs: 30000 }, environment: [] });
      key = await pending("invoke_mcp_http", { connectionId: connection.connectionId, requestId: "control-mcp", method: "tools/list", params: {} });
      await until(() => held.has("mcp"), "MCP invocation did not start");
      await control(method, { connectionId: connection.connectionId, ...(method.startsWith("cancel") ? { requestId: "control-mcp" } : {}) }, 1, () => !held.has("mcp"), key);
      if (method.startsWith("cancel")) await success("api-studio.api", "disconnect_mcp_http", { connectionId: connection.connectionId });
    }
    const sse = await success("api-studio.api", "start_sse_stream", { req: template(`${endpoint}/sse`), environment: [], options: { connectTimeoutMs: 10000, idleTimeoutMs: 30000, totalTimeoutMs: 60000, reconnect: false } });
    await until(() => held.has("sse"), "SSE did not start");
    await control("stop_sse_stream", { sessionId: sse }, 0, () => !held.has("sse"));
    for (const method of ["close_websocket", "disconnect_websocket"]) {
      const sessionId = await success("api-studio.api", "start_websocket", { req: template(endpoint.replace("http:", "ws:") + "/ws"), environment: [] });
      await until(() => held.has("websocket"), "WebSocket did not connect");
      await control(method, { sessionId, ...(method === "close_websocket" ? { close: { code: 1000, reason: "owned fixture" } } : {}) }, 0, () => !held.has("websocket"));
    }
    for (const method of ["cancel_grpc", "disconnect_grpc"]) {
      evidence.controlAdmission.step = "grpc-connect";
      const connection = await success("api-studio.api", "connect_grpc", { profile: { endpoint: `http://127.0.0.1:${h2.address().port}`, source: { kind: "reflection" }, tls: { rootMode: "native" }, connectTimeoutMs: 10000, rpcTimeoutMs: 30000 } });
      key = await pending("invoke_grpc", { connectionId: connection.connectionId, requestId: "control-grpc", method: "fixture.Control.Hold", messages: ["{}"] });
      await until(() => held.has("grpc"), "gRPC invocation did not start");
      await control(method, { connectionId: connection.connectionId, ...(method.startsWith("cancel") ? { requestId: "control-grpc" } : {}) }, 1, () => !held.has("grpc"), key);
      if (method.startsWith("cancel")) await success("api-studio.api", "disconnect_grpc", { connectionId: connection.connectionId });
    }
    // Select Node through the real native dialog; no renderer-created path ID.
    evidence.controlAdmission.step = "stdio-native-selection";
    const selecting = call("api-studio.api", "pick_mcp_stdio_executable").then(value => ({ value }), error => ({ error }));
    const picker = spawn("powershell.exe", ["-NoProfile", "-NonInteractive", "-File", path.resolve(".github/scripts/windows-api-control-picker.ps1"), "-OwnerProcessId", String(ui.child.pid), "-SelectedFile", process.execPath], { stdio: "inherit", windowsHide: true });
    const [pickerCode] = await once(picker, "exit"); assert.equal(pickerCode, 0);
    const selected = await selecting; assert.ok(!selected.error);
    const selectionResponse = selected.value; assert.equal(selectionResponse.operation.outcome.state, "succeeded");
    const executableSelectionId = selectionResponse.value.selectionId;
    for (const method of ["cancel_mcp_stdio", "disconnect_mcp_stdio"]) {
      const pidFile = path.join(root, `control-${randomUUID()}.json`), fixture = path.join(root, `control-${randomUUID()}.mjs`);
      writeFileSync(fixture, `import{createInterface}from'node:readline';import{writeFileSync}from'node:fs';writeFileSync(${JSON.stringify(pidFile)},JSON.stringify({pid:process.pid}));createInterface({input:process.stdin}).on('line',line=>{const m=JSON.parse(line);if(m.method==='server/discover')process.stdout.write(JSON.stringify((${discover.toString()})(m.id))+'\\n');else if(m.id)writeFileSync(${JSON.stringify(pidFile + ".invoked")},"ready");});`, { flag: "wx" });
      const connection = await success("api-studio.api", "connect_mcp_stdio", { profile: { executableSelectionId, cwdSelectionId: null, era: "modern", args: [fixture], environment: [], timeoutMs: 30000 }, environment: [] });
      key = await pending("invoke_mcp_stdio", { connectionId: connection.connectionId, requestId: "control-stdio", method: "tools/list", params: {} });
      await until(() => existsSync(pidFile + ".invoked"), "stdio invocation did not start");
      const { pid } = JSON.parse(readFileSync(pidFile));
      await control(method, { connectionId: connection.connectionId, ...(method.startsWith("cancel") ? { requestId: "control-stdio" } : {}) }, 1, () => { try { process.kill(pid, 0); return false; } catch (error) { if (error.code === "ESRCH") return true; throw error; } }, key);
    }
    key = await pending("authorize_mcp_http", { requestId: "control-oauth", endpoint: `${endpoint}/oauth`, issuer: null, clientId: "owned-fixture", scopes: [] });
    await until(() => held.has("oauth"), "OAuth discovery did not start");
    await control("cancel_mcp_oauth", { requestId: "control-oauth" }, 1, () => !held.has("oauth"), key);
    evidence.controlAdmission = { normalSlots: 64, released: recorded, result: "pass" };
  } finally {
    for (const socket of sockets) socket.destroy();
    for (const session of h2sessions) session.destroy();
    await Promise.all([new Promise(resolve => server.close(resolve)), new Promise(resolve => h2.close(resolve))]);
  }
}
