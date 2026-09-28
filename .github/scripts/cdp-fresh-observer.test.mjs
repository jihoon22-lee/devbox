import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import { once } from "node:events";
import { observeFreshCdp, inspectCdpTarget } from "./cdp-fresh-observer.mjs";

const target = "ws://127.0.0.1:9222/devtools/page/owned-page";
const alive = { exitCode: null, signalCode: null };
test("discovery aborts a real loopback response that stops mid-body", async () => {
  const server = createServer((_request, response) => {
    response.writeHead(200, { "Content-Type": "application/json" });
    response.write("[");
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  try {
    const started = performance.now();
    assert.deepEqual(
      await inspectCdpTarget(`ws://127.0.0.1:${server.address().port}/devtools/page/id`, { timeoutMs: 50 }),
      { state: "unreachable", reason: "timeout" },
    );
    assert.ok(performance.now() - started < 2000);
  } finally {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});
async function withFetch(response, run) {
  const previous = globalThis.fetch;
  const requests = [];
  globalThis.fetch = async (...args) => {
    requests.push(args);
    if (response instanceof Error) throw response;
    return response();
  };
  try {
    await run(requests);
  } finally {
    globalThis.fetch = previous;
  }
}

test("target discovery distinguishes the original page from a replaced page without exposing metadata", async () => {
  for (const present of [true, false]) {
    await withFetch(
      () =>
        Response.json([
          { type: "page", webSocketDebuggerUrl: present ? target : `${target}-replacement`, title: "private" },
        ]),
      async (requests) => {
        assert.deepEqual(await inspectCdpTarget(target), { state: "responded", targetPresent: present, pageCount: 1 });
        assert.equal(requests[0][0], "http://127.0.0.1:9222/json/list");
        assert.equal(requests[0][1].redirect, "error");
      },
    );
  }
});

test("discovery bounds malformed, oversized, failed and external endpoint responses", async () => {
  for (const [response, state] of [
    [() => new Response("x".repeat(65537)), "invalid_response"],
    [() => Response.json({ token: "private" }), "invalid_response"],
    [() => new Response("private", { status: 503 }), "http_error"],
    [new Error("private connection details"), "unreachable"],
  ]) {
    await withFetch(response, async () =>
      assert.deepEqual(
        await inspectCdpTarget(target),
        state === "unreachable" ? { state, reason: "other" } : { state },
      ),
    );
  }
  await withFetch(
    () => Response.json([]),
    async (requests) => {
      assert.deepEqual(await inspectCdpTarget("ws://external.invalid:9222/devtools/page/id"), {
        state: "invalid_target",
      });
      assert.equal(requests.length, 0);
    },
  );
});
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

test("a socket that never opens is closed and its target discovery is recorded", async () => {
  await withFetch(
    () => Response.json([]),
    async () =>
      withSocket(
        () => undefined,
        async (calls, closed) => {
          assert.deepEqual(await observeFreshCdp(target, alive, { timeoutMs: 10 }), {
            state: "open_failed",
            endpoint: { state: "responded", targetPresent: false, pageCount: 0 },
          });
          assert.equal(calls.length, 0);
          assert.equal(closed(), true);
        },
        { open: false },
      ),
  );
});

test("discovery distinguishes refusal from timeout without retaining native error text", async () => {
  for (const [code, reason] of [
    ["ECONNREFUSED", "refused"],
    ["ECONNRESET", "reset"],
    ["UND_ERR_CONNECT_TIMEOUT", "timeout"],
  ]) {
    const cause = Object.assign(new Error("private/address"), { code });
    await withFetch(new TypeError("private endpoint", { cause }), async () => {
      assert.deepEqual(await inspectCdpTarget(target), { state: "unreachable", reason });
    });
  }
});
