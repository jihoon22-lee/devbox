import assert from "node:assert/strict";
import test from "node:test";
import { observeFreshCdp } from "./cdp-fresh-observer.mjs";

const target = "ws://127.0.0.1:9222/devtools/page/owned-page";
const alive = { exitCode: null, signalCode: null };
async function withSocket(reply, run, { open = true } = {}) {
  const previous = globalThis.WebSocket;
  const calls = [];
  let closed = false;
  class FakeSocket extends EventTarget {
    constructor() {
      super();
      if (open) queueMicrotask(() => this.dispatchEvent(new Event("open")));
    }
    send(raw) {
      const request = JSON.parse(raw);
      calls.push(request);
      const result = reply(request);
      if (result !== undefined)
        queueMicrotask(() =>
          this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify({ id: request.id, result }) })),
        );
    }
    close() {
      closed = true;
      this.dispatchEvent(new Event("close"));
    }
  }
  globalThis.WebSocket = FakeSocket;
  try {
    await run(calls, () => closed);
  } finally {
    globalThis.WebSocket = previous;
  }
}

test("a fresh observer reads browser, renderer and native status without replaying reconnect", async () => {
  await withSocket(
    (request) =>
      request.method === "Browser.getVersion"
        ? { product: "private version" }
        : { result: { value: request.params.expression === "1" ? 1 : "connected" } },
    async (calls, closed) => {
      assert.deepEqual(await observeFreshCdp(target, alive, { timeoutMs: 100 }), {
        state: "observed",
        browserResponsive: true,
        rendererResponsive: true,
        nativeStatus: "connected",
      });
      assert.equal(calls.length, 3);
      assert.ok(!JSON.stringify(calls).includes("agent_reconnect"));
      assert.equal(closed(), true);
    },
  );
});

test("a responsive browser with an unresponsive renderer stays a bounded failed observation", async () => {
  await withSocket(
    (request) => (request.method === "Browser.getVersion" ? {} : undefined),
    async (calls, closed) => {
      assert.deepEqual(await observeFreshCdp(target, alive, { timeoutMs: 10 }), {
        state: "observed",
        browserResponsive: true,
        rendererResponsive: false,
        nativeStatus: "renderer_unresponsive",
      });
      assert.equal(calls.length, 2);
      assert.equal(closed(), true);
    },
  );
});

test("an exited process or a non-loopback target is never contacted", async () => {
  await withSocket(
    () => {
      throw new Error("must not send");
    },
    async (calls) => {
      assert.deepEqual(await observeFreshCdp(target, { exitCode: 0, signalCode: null }), { state: "product_exited" });
      assert.deepEqual(await observeFreshCdp("ws://external.invalid:9222/devtools/page/id", alive), {
        state: "invalid_target",
      });
      assert.equal(calls.length, 0);
    },
  );
});

test("unexpected native details are projected to a fixed code", async () => {
  await withSocket(
    (request) =>
      request.method === "Browser.getVersion"
        ? {}
        : { result: { value: request.params.expression === "1" ? 1 : "private/path/token" } },
    async () => {
      assert.equal((await observeFreshCdp(target, alive, { timeoutMs: 100 })).nativeStatus, "unexpected");
    },
  );
});

test("a socket that never opens is closed at the observer deadline", async () => {
  await withSocket(
    () => undefined,
    async (calls, closed) => {
      assert.deepEqual(await observeFreshCdp(target, alive, { timeoutMs: 10 }), { state: "open_failed" });
      assert.equal(calls.length, 0);
      assert.equal(closed(), true);
    },
    { open: false },
  );
});
