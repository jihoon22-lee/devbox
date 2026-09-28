import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { connect } from "./workspace-cdp-fixture.mjs";

test("a closed CDP socket rejects pending and future calls without a misleading timeout", async () => {
  const previous = { fetch: globalThis.fetch, WebSocket: globalThis.WebSocket, cwd: process.cwd() };
  const directory = mkdtempSync(path.join(tmpdir(), "devbox-cdp-test-"));
  mkdirSync(path.join(directory, "product-foundation-evidence"));
  let socket;
  class FakeSocket extends EventTarget {
    readyState = 1;
    constructor() {
      super();
      socket = this;
      queueMicrotask(() => this.dispatchEvent(new Event("open")));
    }
    send(raw) {
      const request = JSON.parse(raw);
      if (request.method === "Runtime.evaluate") return;
      queueMicrotask(() =>
        this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify({ id: request.id, result: {} }) })),
      );
    }
    close() {
      this.readyState = 3;
      const event = new Event("close");
      event.code = 1006;
      this.dispatchEvent(event);
    }
  }
  try {
    process.chdir(directory);
    globalThis.WebSocket = FakeSocket;
    globalThis.fetch = async () => ({
      json: async () => [
        { type: "page", url: "http://tauri.localhost/", webSocketDebuggerUrl: "ws://127.0.0.1/fixture" },
      ],
    });
    const cdp = await connect(1, { exitCode: null });
    const pending = cdp.evaluate("1", { timeoutMs: 100 });
    socket.close();
    await assert.rejects(pending, /CDP disconnected/);
    await assert.rejects(cdp.evaluate("1", { timeoutMs: 100 }), /CDP disconnected/);
    assert.deepEqual(cdp.connectionState(), { state: "closed", closeCode: 1006 });
  } finally {
    globalThis.fetch = previous.fetch;
    globalThis.WebSocket = previous.WebSocket;
    process.chdir(previous.cwd);
    rmSync(directory, { recursive: true, force: true });
  }
});

test("startup failure retains a closed stage and attempt count without remote error text", async () => {
  const previous = globalThis.fetch;
  globalThis.fetch = async () => {
    throw new Error("private host/token response");
  };
  try {
    await assert.rejects(connect(1, { exitCode: null }, performance.now() + 20), (error) => {
      assert.equal(error.message, "renderer startup deadline exceeded");
      assert.deepEqual(error.cdpStartup, { stage: "discovery", attempts: 1 });
      assert.ok(!JSON.stringify(error).includes("private"));
      return true;
    });
  } finally {
    globalThis.fetch = previous;
  }
});
