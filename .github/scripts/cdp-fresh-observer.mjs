// Read-only probes retain fixed metadata only; no reconnect is replayed.
function targetUrl(target) {
  try {
    const url = new URL(target);
    if (
      url.protocol !== "ws:" ||
      !["localhost", "127.0.0.1"].includes(url.hostname) ||
      !url.port ||
      url.username ||
      url.password ||
      url.search ||
      url.hash ||
      !/^\/devtools\/page\/[a-zA-Z0-9_-]+$/.test(url.pathname)
    )
      return null;
    return url;
  } catch {
    return null;
  }
}
function deadline(timeoutMs) {
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 1000)
    throw new Error("invalid observer deadline");
}
export async function inspectCdpTarget(target, { timeoutMs = 1000 } = {}) {
  deadline(timeoutMs);
  const url = targetUrl(target);
  if (!url) return { state: "invalid_target" };
  let reader;
  try {
    const response = await fetch(`http://${url.host}/json/list`, {
      signal: AbortSignal.timeout(timeoutMs),
      redirect: "error",
    });
    if (!response.ok) {
      void response.body?.cancel().catch(() => {});
      return { state: "http_error" };
    }
    reader = response.body?.getReader();
    if (!reader) return { state: "invalid_response" };
    const chunks = [];
    let size = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > 65536) return { state: "invalid_response" };
      chunks.push(value);
    }
    let pages;
    try {
      pages = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    } catch {
      return { state: "invalid_response" };
    }
    if (!Array.isArray(pages) || pages.length > 100) return { state: "invalid_response" };
    return {
      state: "responded",
      targetPresent: pages.some((page) => page?.type === "page" && page.webSocketDebuggerUrl === url.href),
      pageCount: pages.filter((page) => page?.type === "page").length,
    };
  } catch {
    return { state: "unreachable" };
  } finally {
    void reader?.cancel().catch(() => {});
  }
}
export async function observeFreshCdp(target, child, { timeoutMs = 1000 } = {}) {
  deadline(timeoutMs);
  const exited = () => child.exitCode !== null || child.signalCode != null;
  if (exited()) return { state: "product_exited" };
  if (!targetUrl(target)) return { state: "invalid_target" };
  let socket;
  let id = 0;
  const pending = new Map();
  try {
    socket = new WebSocket(target);
    const opened = await new Promise((resolve) => {
      let settled = false;
      const timer = setTimeout(() => finish(false), timeoutMs);
      const finish = (value) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        resolve(value);
      };
      socket.addEventListener("open", () => finish(true));
      const disconnected = () => {
        finish(false);
        for (const entry of pending.values()) {
          clearTimeout(entry.timer);
          entry.reject(new Error("observer disconnected"));
        }
        pending.clear();
      };
      socket.addEventListener("close", disconnected);
      socket.addEventListener("error", disconnected);
    });
    if (!opened) return { state: "open_failed", endpoint: await inspectCdpTarget(target, { timeoutMs }) };
    socket.addEventListener("message", ({ data }) => {
      try {
        const reply = JSON.parse(data);
        const entry = pending.get(reply.id);
        if (!entry) return;
        pending.delete(reply.id);
        clearTimeout(entry.timer);
        reply.error ? entry.reject(new Error("observer request failed")) : entry.resolve(reply.result);
      } catch {
        /* only protocol replies affect the bounded observer */
      }
    });
    const call = (method, params = {}) =>
      new Promise((resolve, reject) => {
        if (exited()) {
          reject(new Error("product exited"));
          return;
        }
        const next = ++id;
        const timer = setTimeout(() => {
          pending.delete(next);
          reject(new Error("observer timeout"));
        }, timeoutMs);
        pending.set(next, { resolve, reject, timer });
        try {
          socket.send(JSON.stringify({ id: next, method, params }));
        } catch {
          pending.delete(next);
          clearTimeout(timer);
          reject(new Error("observer send failed"));
        }
      });
    const evaluate = async (expression) => {
      const result = await call("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
      if (result?.exceptionDetails) throw new Error("observer evaluation failed");
      return result?.result?.value;
    };
    const browserResponsive = await call("Browser.getVersion").then(
      () => true,
      () => false,
    );
    const rendererResponsive = await evaluate("1").then(
      (value) => value === 1,
      () => false,
    );
    const nativeStatus = rendererResponsive
      ? await evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_status')").then(
          (value) =>
            ["connected", "starting", "restarting", "unavailable", "unsupported"].includes(value)
              ? value
              : "unexpected",
          () => "probe_failed",
        )
      : "renderer_unresponsive";
    return { state: "observed", browserResponsive, rendererResponsive, nativeStatus };
  } catch {
    return { state: "observer_failed" };
  } finally {
    try {
      socket?.close();
    } catch {
      /* owned diagnostic connection only */
    }
  }
}
