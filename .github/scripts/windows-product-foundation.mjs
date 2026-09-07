// Runs only on a disposable GitHub-hosted Windows runner. Uses synthetic
// product installations, never an installed user app or a legacy data store.
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, copyFileSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import { randomUUID } from "node:crypto";
import { windowsProcessIsElevated, inspectElevatedCdpPolicy, installElevatedCdpPolicy, restoreElevatedCdpPolicy } from "./windows-packaged-smoke.mjs";

assert.equal(process.platform, "win32");
assert.equal(process.env.GITHUB_ACTIONS, "true");
assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
const elevated = windowsProcessIsElevated();
const products = JSON.parse(readFileSync("apps/products.json", "utf8")).products;
const root = mkdtempSync(path.join(tmpdir(), "devbox-product-fixture-"));
const evidence = { source: process.env.GITHUB_SHA, environment: "github-hosted-windows", fixtureVersion: 1, products: [], result: "failed" };
mkdirSync("product-foundation-evidence", { recursive: true });
let currentProbe = null;
function progress(product, suffix, stage) {
  currentProbe = { product: product.id, suffix, stage };
  writeFileSync("product-foundation-evidence/progress.json", JSON.stringify(currentProbe, null, 2));
}
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function freePort() {
  const server = createServer(); server.listen(0, "127.0.0.1"); await once(server, "listening");
  const port = server.address().port; await new Promise((resolve) => server.close(resolve)); return port;
}

async function connect(port, child) {
  for (let i = 0; i < 120; i++) {
    if (child.exitCode !== null) throw new Error("product exited before renderer opened");
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(500) });
      const pages = await response.json(); const page = pages.find((p) => p.type === "page" && p.webSocketDebuggerUrl);
      if (page) {
        const socket = new WebSocket(page.webSocketDebuggerUrl); await once(socket, "open");
        let id = 0; const pending = new Map(); const diagnostics = [];
        socket.addEventListener("message", ({ data }) => {
          const response = JSON.parse(data); const entry = pending.get(response.id);
          if (["Runtime.exceptionThrown", "Log.entryAdded", "Page.javascriptDialogOpening", "Inspector.targetCrashed"].includes(response.method)) {
            diagnostics.push({ event: response.method, details: JSON.stringify(response.params).slice(0, 6000) });
            if (diagnostics.length > 30) diagnostics.shift();
            writeFileSync("product-foundation-evidence/renderer-events.json", JSON.stringify({ currentProbe, diagnostics }, null, 2));
          }
          if (entry) { pending.delete(response.id); clearTimeout(entry.timer); response.error ? entry.reject(new Error("CDP request failed")) : entry.resolve(response.result); }
        });
        const command = (method, params = {}) => {
          const next = ++id;
          return new Promise((resolve, reject) => {
            const timer = setTimeout(() => { pending.delete(next); reject(new Error("CDP setup timeout")); }, 10_000);
            pending.set(next, { resolve, reject, timer });
            socket.send(JSON.stringify({ id: next, method, params }));
          });
        };
        try { await command("Runtime.enable"); await command("Log.enable"); await command("Page.enable"); } catch (error) { socket.close(); throw error; }
        return {
          close: () => socket.close(),
          command,
          async evaluate(expression) {
            const next = ++id;
            const result = await new Promise((resolve, reject) => {
              const timer = setTimeout(() => { pending.delete(next); writeFileSync("product-foundation-evidence/renderer-timeout.json", JSON.stringify({ currentProbe, diagnostics, expression: expression.slice(0, 240) }, null, 2)); reject(new Error(`CDP request timeout at ${currentProbe?.stage}`)); }, 10_000);
              pending.set(next, { resolve, reject, timer });
              socket.send(JSON.stringify({ id: next, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
            });
            if (result.exceptionDetails) throw new Error(`renderer probe failed: ${String(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text).slice(0, 2000)}`);
            return result.result.value;
          },
        };
      }
    } catch { /* bounded startup polling */ }
    await delay(250);
  }
  throw new Error("renderer startup deadline exceeded");
}

async function waitForRenderer(cdp, expression, label) {
  const deadline = performance.now() + 15_000;
  while (performance.now() < deadline) {
    if (await cdp.evaluate(expression)) return;
    await delay(100);
  }
  // This runner contains only synthetic fixtures. Retain bounded UI state on
  // failure so a delivery, route, IPC error and lazy-load failure are distinct.
  const snapshot = await cdp.evaluate("({ route: document.querySelector('nav[aria-label=\"제품 화면\"] [aria-current=page]')?.textContent, dialogs: document.querySelectorAll(\"[role=dialog]\").length, text: (document.body?.innerText ?? \"\").slice(0, 12000) })");
  writeFileSync(path.join("product-foundation-evidence", `renderer-failure-${Date.now()}.json`), JSON.stringify({ label, snapshot }, null, 2));
  throw new Error(`${label}: route=${snapshot.route}, dialogs=${snapshot.dialogs}`);
}

async function start(product, suffix) {
  const directory = path.join(root, `${product.id}-${suffix}`); mkdirSync(directory);
  // Elevated WebView2 reads per-image machine policy instead of the process
  // override. Each fixture copy owns a unique value while both copies run.
  const imageName = `devbox-${product.id}-${suffix}-${randomUUID()}.exe`;
  const executable = path.join(directory, imageName);
  const built = path.resolve("target/debug", `devbox-${product.id}.exe`);
  assert.ok(existsSync(built), "packaged executable is missing"); copyFileSync(built, executable);
  const port = await freePort();
  const env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`, WEBVIEW2_USER_DATA_FOLDER: path.join(directory, "webview2") };
  const policy = elevated ? inspectElevatedCdpPolicy(imageName, port) : null;
  let cdp, child;
  try {
    if (policy) installElevatedCdpPolicy(policy);
    const started = performance.now();
    child = spawn(executable, [`--route=${product.defaultRoute}`], { env, stdio: "ignore" });
    await once(child, "spawn");
    cdp = await connect(port, child);
    progress(product, suffix, "renderer-connected");
    let ready = false, readinessError = "";
    const readinessDeadline = performance.now() + 30_000;
    while (performance.now() < readinessDeadline) {
      // Native development route selection can replace the initial document.
      // Retry only readiness; invocation and authority probes below fail once.
      try {
        ready = await cdp.evaluate(product.id === "api-studio"
          ? '!!document.querySelector(".api-feature-requests .url-input")'
          : '(document.body?.innerText ?? "").includes("기능 이전을 준비하고 있습니다")');
      } catch (error) { readinessError = error.message; }
      if (ready) break; await delay(100);
    }
    assert.ok(ready, `native route must render an accepted response: ${readinessError}`);
    const startupMs = Math.round(performance.now() - started);
    assert.equal(await cdp.evaluate('new URLSearchParams(location.search).get("route")'), product.defaultRoute);
    progress(product, suffix, "description");
    const description = await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")');
    assert.equal(description.product.id, product.id);
    progress(product, suffix, "shell-authority");
    const probe = await cdp.evaluate(`(async () => {
      const invoke = window.__TAURI_INTERNALS__.invoke;
      const d = await invoke("plugin:product-shell|describe");
      const r = { protocolVersion: 1, installationId: d.handshake.installationId, sessionId: d.handshake.sessionId, requestId: ${JSON.stringify(randomUUID())}, deadlineMs: Date.now() + 5000, route: d.product.defaultRoute };
      const result = await invoke("plugin:product-shell|route_status", { request: r });
      let replayRejected = false, ownerRejected = false;
      try { await invoke("plugin:product-shell|route_status", { request: r }); } catch { replayRejected = true; }
      try { await invoke("plugin:product-shell|route_status", { request: { ...r, requestId: crypto.randomUUID(), installationId: "other-installation" } }); } catch { ownerRejected = true; }
      return { replayRejected, ownerRejected, availability: result.availability, state: result.operation.outcome.state };
    })()`);
    assert.deepEqual(probe, { replayRejected: true, ownerRejected: true, availability: "foundation", state: "succeeded" });
    let componentProbe;
    if (product.id === "api-studio") {
      progress(product, suffix, "component-authority");
      componentProbe = await cdp.evaluate(`(async () => {
        const invoke = window.__TAURI_INTERNALS__.invoke;
        const d = await invoke("plugin:product-shell|describe");
        const header = (route) => ({ protocolVersion: 1, installationId: d.handshake.installationId, sessionId: d.handshake.sessionId, requestId: crypto.randomUUID(), deadlineMs: Date.now() + 5000, route });
        const request = { header: header("webhooks"), component: "api-studio.webhooks", method: "server_status", args: {} };
        const status = await invoke("plugin:api-studio|execute", { request });
        let replayRejected = false, ownerRejected = false, installationRejected = false;
        try { await invoke("plugin:api-studio|execute", { request }); } catch { replayRejected = true; }
        try { await invoke("plugin:api-studio|execute", { request: { ...request, header: header("webhooks"), method: "send_request" } }); } catch { ownerRejected = true; }
        try { await invoke("plugin:api-studio|execute", { request: { ...request, header: { ...header("webhooks"), installationId: "other-installation" } } }); } catch { installationRejected = true; }
        const hash = await invoke("plugin:api-studio|execute", { request: { header: header("transforms"), component: "api-studio.transforms", method: "hash", args: { data: "abc", algorithm: "sha256" } } });
        return { replayRejected, ownerRejected, installationRejected, listenerRunning: status.value.running, hash: hash.value, component: hash.operation.provenance.component, state: hash.operation.outcome.state };
      })()`);
      assert.deepEqual(componentProbe, { replayRejected: true, ownerRejected: true, installationRejected: true, listenerRunning: false, hash: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", component: "api-studio.transforms", state: "succeeded" });
      progress(product, suffix, "publish-internal-handoff");
      const artifact = await cdp.evaluate(`(async () => {
        const invoke = window.__TAURI_INTERNALS__.invoke;
        const d = await invoke("plugin:product-shell|describe");
        const header = { protocolVersion: 1, installationId: d.handshake.installationId, sessionId: d.handshake.sessionId, requestId: crypto.randomUUID(), deadlineMs: Date.now() + 5000, route: "requests" };
        const result = await invoke("plugin:api-studio|execute", { request: { header, component: "api-studio.api", method: "send_selection_to_toolbox", args: { text: JSON.stringify({ token: "synthetic-fixture-secret", ok: true }) } } });
        return { id: result.value.handoffId, redacted: result.value.redacted, owner: result.value.artifact.provenance.component };
      })()`);
      assert.equal(artifact.redacted, true);
      assert.equal(artifact.owner, "api-studio.api");
      progress(product, suffix, "wait-internal-preview");
      await waitForRenderer(cdp, '!!document.querySelector(".api-feature-transforms:not([hidden]) [role=dialog]")', "internal handoff preview did not open");
      assert.equal(await cdp.evaluate('(document.querySelector(".api-feature-transforms [role=dialog]")?.textContent ?? "").includes("synthetic-fixture-secret")'), false);
      progress(product, suffix, "apply-internal-preview");
      await cdp.evaluate('Array.from(document.querySelectorAll(".api-feature-transforms [role=dialog] button")).find(button => button.textContent.trim() === "적용").click()');
      await waitForRenderer(cdp, "document.querySelector('textarea[aria-label=\"스마트 워크플로 입력\"]')?.value.includes(\"[REDACTED]\") && !document.querySelector(\".api-feature-transforms [role=dialog]\")", "internal handoff was not explicitly applied");
      assert.equal(await cdp.evaluate(`(async () => {
        const invoke = window.__TAURI_INTERNALS__.invoke;
        const d = await invoke("plugin:product-shell|describe");
        const header = { protocolVersion: 1, installationId: d.handshake.installationId, sessionId: d.handshake.sessionId, requestId: crypto.randomUUID(), deadlineMs: Date.now() + 5000, route: "transforms" };
        const result = await invoke("plugin:api-studio|execute", { request: { header, component: "api-studio.transforms", method: "preview_toolbox_text", args: { handoffId: ${JSON.stringify(artifact.id)} } } });
        return result.operation.outcome.state === "failed";
      })()`), true);
      progress(product, suffix, "internal-handoff-complete");
      componentProbe.internalHandoff = "redacted-preview-explicit-apply-one-time";

      progress(product, suffix, "transform-export-and-knowledge-fallback");
      const outputPolicy = await cdp.evaluate(`(async () => {
        const invoke = window.__TAURI_INTERNALS__.invoke;
        const d = await invoke("plugin:product-shell|describe");
        const call = (component, method, args) => invoke("plugin:api-studio|execute", { request: {
          header: { protocolVersion: 1, installationId: d.handshake.installationId, sessionId: d.handshake.sessionId,
            requestId: crypto.randomUUID(), deadlineMs: Date.now() + 5000, route: component === "api-studio.api" ? "requests" : "transforms" },
          component, method, args
        } });
        const output = "safe-result\\npassword=synthetic-output-secret";
        const source = { kind: "tool", toolId: "json-format" };
        const hmac = await call("api-studio.transforms", "create_api_request_handoff", { source: { kind: "tool", toolId: "hmac" }, output });
        const hmacDraft = await call("api-studio.transforms", "save_knowledge_draft", { source: { kind: "tool", toolId: "hmac" }, output });
        const saved = await call("api-studio.transforms", "save_knowledge_draft", { source, output });
        if (saved.operation.outcome.state !== "succeeded") throw new Error("Knowledge native save failed");
        const id = saved.value.draft.artifact.id;
        const read = await call("api-studio.transforms", "get_knowledge_draft", { id });
        const foreign = await call("api-studio.api", "get_knowledge_draft", { id });
        const list = await call("api-studio.transforms", "list_knowledge_drafts", {});
        const sent = await call("api-studio.transforms", "create_api_request_handoff", { source, output });
        return { hmacRequestDenied: hmac.operation.outcome.state === "failed", hmacDraftDenied: hmacDraft.operation.outcome.state === "failed",
          unavailable: saved.value.delivery === "unavailable", masked: saved.value.draft.redacted && !read.value.body.includes("synthetic-output-secret"),
          durable: list.value.some(value => value.artifact.id === id) && read.value.artifact.kind === "knowledge-draft/v1",
          foreignDenied: foreign.operation.outcome.state === "failed", requestPreview: sent.operation.outcome.state === "succeeded" };
      })()`);
      assert.deepEqual(outputPolicy, { hmacRequestDenied: true, hmacDraftDenied: true, unavailable: true, masked: true, durable: true, foreignDenied: true, requestPreview: true });
      await waitForRenderer(cdp, '!!document.querySelector(".api-feature-requests:not([hidden]) [role=dialog]")', "transform request preview did not open");
      assert.equal(await cdp.evaluate('(document.querySelector(".api-feature-requests [role=dialog]")?.textContent ?? "").includes("synthetic-output-secret")'), false);
      componentProbe.outputPolicy = outputPolicy;

      progress(product, suffix, "mock-draft-preview");
      assert.equal(await cdp.evaluate(`(async () => {
        const invoke = window.__TAURI_INTERNALS__.invoke;
        const d = await invoke("plugin:product-shell|describe");
        const header = { protocolVersion: 1, installationId: d.handshake.installationId, sessionId: d.handshake.sessionId,
          requestId: crypto.randomUUID(), deadlineMs: Date.now() + 5000, route: "requests" };
        const sent = await invoke("plugin:api-studio|execute", { request: { header, component: "api-studio.api", method: "send_mock_draft",
          args: { output: "mock-fixture\\nAuthorization: Bearer synthetic-mock-secret", status: 201 } } });
        return sent.operation.outcome.state === "succeeded";
      })()`), true);
      await waitForRenderer(cdp, '!!document.querySelector(".api-feature-webhooks:not([hidden]) .mock-draft-dialog")', "Mock preview did not open");
      assert.equal(await cdp.evaluate('document.querySelector(".mock-draft-dialog").textContent.includes("synthetic-mock-secret")'), false);
      const shot = await cdp.command("Page.captureScreenshot", { format: "png" });
      writeFileSync(`product-foundation-evidence/mock-preview-${suffix}.png`, Buffer.from(shot.data, "base64"));
      await cdp.evaluate('Array.from(document.querySelectorAll(".mock-draft-dialog button")).find(button => button.textContent.trim() === "현재 규칙 초안 대신 적용").click()');
      await waitForRenderer(cdp, 'document.querySelector("#rule-body")?.value.includes("mock-fixture") && !document.querySelector(".mock-draft-dialog")', "Mock preview was not explicitly applied");
      const mockDraft = await cdp.evaluate(`(async () => {
        const invoke = window.__TAURI_INTERNALS__.invoke;
        const d = await invoke("plugin:product-shell|describe");
        const call = (method) => invoke("plugin:api-studio|execute", { request: {
          header: { protocolVersion: 1, installationId: d.handshake.installationId, sessionId: d.handshake.sessionId,
            requestId: crypto.randomUUID(), deadlineMs: Date.now() + 5000, route: "webhooks" },
          component: "api-studio.webhooks", method, args: {}
        } });
        const rules = await call("list_rules"); const status = await call("server_status");
        return { editorOnly: rules.value.length === 0, listenerStopped: !status.value.running,
          status: document.querySelector("#rule-status").value, redacted: document.querySelector("#rule-body").value.includes("[REDACTED]") };
      })()`);
      assert.deepEqual(mockDraft, { editorOnly: true, listenerStopped: true, status: "201", redacted: true });
      componentProbe.mockDraft = mockDraft;

    }
    const second = spawn(executable, [], { env, stdio: "ignore" });
    await Promise.race([once(second, "exit"), delay(10_000).then(() => { if (second.exitCode === null) { second.kill(); throw new Error("second instance did not exit"); } })]);
    assert.equal(second.exitCode, 0); assert.equal(child.exitCode, null);
    return { child, cdp, policy, handshake: description.handshake, startupMs, componentProbe };
  } catch (error) { stop({ child, cdp, policy }); throw error; }
}

function stop(instance) {
  instance?.cdp?.close();
  if (instance?.child?.pid && instance.child.exitCode === null) {
    // Kill only the process tree created by this fixture.
    spawnSync("taskkill.exe", ["/PID", String(instance.child.pid), "/T", "/F"], { stdio: "ignore" });
  }
  if (instance?.policy) restoreElevatedCdpPolicy(instance.policy);
}

try {
  for (const product of products) {
    let first, second;
    try {
      first = await start(product, "a"); second = await start(product, "b");
      assert.notEqual(first.handshake.installationId, second.handshake.installationId);
      assert.notEqual(first.handshake.sessionId, second.handshake.sessionId);
      assert.equal(first.child.exitCode, null);
      evidence.products.push({ product: product.id, nativeRoute: "pass", replay: "rejected", foreignInstallation: "rejected", sameInstallationSecondInstance: "exited", separateInstallations: "isolated", startupMs: [first.startupMs, second.startupMs], ...(first.componentProbe ? { components: [first.componentProbe, second.componentProbe] } : {}) });
    } finally { try { stop(second); } finally { stop(first); } }
  }
  evidence.result = "pass";
} finally {
  writeFileSync("product-foundation-evidence/native.json", JSON.stringify(evidence, null, 2) + "\n");
}
