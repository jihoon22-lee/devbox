import assert from "node:assert/strict";
import test from "node:test";
import { Cdp } from "./windows-packaged-smoke.mjs";

test("protocol errors identify the failed command and code without exposing parameters or raw messages", async () => {
  const previous = globalThis.WebSocket;
  class FixtureSocket extends EventTarget {
    constructor() {
      super();
      queueMicrotask(() => this.dispatchEvent(new Event("open")));
    }
    send(payload) {
      const { id, method } = JSON.parse(payload);
      const message =
        method === "Page.captureScreenshot"
          ? { id, error: { code: -32000, message: "private fixture value" } }
          : { id, result: {} };
      queueMicrotask(() => this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify(message) })));
    }
    close() {}
  }
  globalThis.WebSocket = FixtureSocket;
  const cdp = new Cdp("ws://owned-fixture");
  try {
    await cdp.connect();
    await assert.rejects(cdp.send("Page.captureScreenshot", { secret: "private argument" }), (error) => {
      assert.equal(error.message, "CDP command failed: Page.captureScreenshot (-32000; unclassified)");
      assert.equal(error.message.includes("private"), false);
      return true;
    });
  } finally {
    cdp.close();
    globalThis.WebSocket = previous;
  }
});
