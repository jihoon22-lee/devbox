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

assert.equal(process.platform, "win32");
assert.equal(process.env.GITHUB_ACTIONS, "true");
assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
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
            if (result.exceptionDetails) throw new Error("renderer probe failed");
            return result.result.value;
          },
        };
      }
    } catch { /* bounded startup polling */ }
    await delay(250);
  }
  throw new Error("renderer startup deadline exceeded");
}

async function start(product, suffix) {
  const directory = path.join(root, `${product.id}-${suffix}`); mkdirSync(directory);
  const executable = path.join(directory, `devbox-${product.id}.exe`);
  const built = path.resolve("target/debug", `devbox-${product.id}.exe`);
  assert.ok(existsSync(built), "packaged executable is missing"); copyFileSync(built, executable);
  const port = await freePort(); const started = performance.now();
  const env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-address=127.0.0.1 --remote-debugging-port=${port}` };
  const child = spawn(executable, [], { env, stdio: "ignore" });
  let cdp;
  try {
    cdp = await connect(port, child);
    let ready = false;
    for (let i = 0; i < 100; i++) {
      ready = await cdp.evaluate('document.body.innerText.includes("기능 이전을 준비하고 있습니다")');
      if (ready) break; await delay(100);
    }
    assert.ok(ready, "native route must render an accepted response");
    const startupMs = Math.round(performance.now() - started);
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
      return { replayRejected, ownerRejected, availability: result.availability };
    })()`);
    assert.deepEqual(probe, { replayRejected: true, ownerRejected: true, availability: "foundation" });
    const second = spawn(executable, [], { env, stdio: "ignore" });
    await Promise.race([once(second, "exit"), delay(10_000).then(() => { if (second.exitCode === null) { second.kill(); throw new Error("second instance did not exit"); } })]);
    assert.equal(second.exitCode, 0); assert.equal(child.exitCode, null);
    return { child, cdp, handshake: description.handshake, startupMs };
  } catch (error) { cdp?.close(); child.kill(); throw error; }
}

function stop(instance) {
  instance?.cdp.close();
  if (instance?.child.pid && instance.child.exitCode === null) {
    // Kill only the process tree created by this fixture.
    spawnSync("taskkill.exe", ["/PID", String(instance.child.pid), "/T", "/F"], { stdio: "ignore" });
  }
}

try {
  for (const product of products) {
    let first, second;
    try {
      first = await start(product, "a"); second = await start(product, "b");
      assert.notEqual(first.handshake.installationId, second.handshake.installationId);
      assert.notEqual(first.handshake.sessionId, second.handshake.sessionId);
      assert.equal(first.child.exitCode, null);
      evidence.products.push({ product: product.id, nativeRoute: "pass", replay: "rejected", foreignInstallation: "rejected", sameInstallationSecondInstance: "exited", separateInstallations: "isolated", startupMs: [first.startupMs, second.startupMs] });
    } finally { stop(second); stop(first); }
  }
  evidence.result = "pass";
} finally {
  writeFileSync("product-foundation-evidence/native.json", JSON.stringify(evidence, null, 2) + "\n");
}
