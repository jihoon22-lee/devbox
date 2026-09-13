// Shared bounded WebView diagnostic transport. No product/process is launched here.
import {once} from "node:events";
import {createServer} from "node:net";
import {writeFileSync} from "node:fs";
import path from "node:path";
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const currentProbe={stage:"owned-workspace-cdp"};
export async function freePort() {
  const server = createServer(); server.listen(0, "127.0.0.1"); await once(server, "listening");
  const port = server.address().port; await new Promise((resolve) => server.close(resolve)); return port;
}

export async function connect(port, child, deadline = performance.now() + 30_000, terminalId = null) {
  while (performance.now() < deadline) {
    if (child.exitCode !== null) throw new Error("product exited before renderer opened");
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(500) });
      const pages = await response.json(); const page = pages.find((p) => {
        if (p.type !== "page" || !p.webSocketDebuggerUrl) return false;
        try { const url = new URL(p.url); return (url.hostname === "tauri.localhost" || (url.protocol === "tauri:" && url.hostname === "localhost")) && ["/", "/index.html"].includes(url.pathname) && (terminalId ? url.searchParams.get("surface")==="terminal" && url.searchParams.get("id")===terminalId : url.searchParams.get("surface")!=="terminal"); } catch { return false; }
      });
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
          async evaluate(expression, {timeoutMs=10_000} = {}) {
            if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 660_000) throw new Error("Invalid fixture CDP deadline");
            new Function(expression); // Check fixture JavaScript before sending it.
            const next = ++id;
            const result = await new Promise((resolve, reject) => {
              const timer = setTimeout(() => { pending.delete(next); writeFileSync("product-foundation-evidence/renderer-timeout.json", JSON.stringify({ currentProbe, diagnostics, expression: expression.slice(0, 240) }, null, 2)); reject(new Error(`CDP request timeout at ${currentProbe?.stage}`)); }, timeoutMs);
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

export async function waitForRenderer(cdp, expression, label) {
  const deadline = performance.now() + 60_000;
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
