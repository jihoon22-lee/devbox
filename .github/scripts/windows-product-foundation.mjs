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
        let id = 0; const pending = new Map();
        socket.addEventListener("message", ({ data }) => {
          const response = JSON.parse(data); const entry = pending.get(response.id);
          if (entry) { pending.delete(response.id); clearTimeout(entry.timer); response.error ? entry.reject(new Error("CDP request failed")) : entry.resolve(response.result); }
        });
        return {
          close: () => socket.close(),
          async evaluate(expression) {
            const next = ++id;
            const result = await new Promise((resolve, reject) => {
              const timer = setTimeout(() => { pending.delete(next); reject(new Error("CDP request timeout")); }, 10_000);
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
  throw new Error(label);
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
    const description = await cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")');
    assert.equal(description.product.id, product.id);
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
      const artifact = await cdp.evaluate(`(async () => {
        const invoke = window.__TAURI_INTERNALS__.invoke;
        const d = await invoke("plugin:product-shell|describe");
        const header = { protocolVersion: 1, installationId: d.handshake.installationId, sessionId: d.handshake.sessionId, requestId: crypto.randomUUID(), deadlineMs: Date.now() + 5000, route: "requests" };
        const result = await invoke("plugin:api-studio|execute", { request: { header, component: "api-studio.api", method: "send_selection_to_toolbox", args: { text: JSON.stringify({ token: "synthetic-fixture-secret", ok: true }) } } });
        return { id: result.value.handoffId, redacted: result.value.redacted, owner: result.value.artifact.provenance.component };
      })()`);
      assert.equal(artifact.redacted, true);
      assert.equal(artifact.owner, "api-studio.api");
      await waitForRenderer(cdp, '!!document.querySelector(".api-feature-transforms:not([hidden]) [role=dialog]")', "internal handoff preview did not open");
      assert.equal(await cdp.evaluate('(document.querySelector(".api-feature-transforms [role=dialog]")?.textContent ?? "").includes("synthetic-fixture-secret")'), false);
      await cdp.evaluate('Array.from(document.querySelectorAll(".api-feature-transforms [role=dialog] button")).find(button => button.textContent.trim() === "적용").click()');
      await waitForRenderer(cdp, "document.querySelector('textarea[aria-label=\"스마트 워크플로 입력\"]')?.value.includes(\"[REDACTED]\") && !document.querySelector(\".api-feature-transforms [role=dialog]\")", "internal handoff was not explicitly applied");
      assert.equal(await cdp.evaluate(`(async () => {
        const invoke = window.__TAURI_INTERNALS__.invoke;
        const d = await invoke("plugin:product-shell|describe");
        const header = { protocolVersion: 1, installationId: d.handshake.installationId, sessionId: d.handshake.sessionId, requestId: crypto.randomUUID(), deadlineMs: Date.now() + 5000, route: "transforms" };
        try { await invoke("plugin:api-studio|execute", { request: { header, component: "api-studio.transforms", method: "preview_toolbox_text", args: { handoffId: ${JSON.stringify(artifact.id)} } } }); return false; } catch { return true; }
      })()`), true);
      componentProbe.internalHandoff = "redacted-preview-explicit-apply-one-time";

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
