import assert from "node:assert/strict";
import test from "node:test";
import { observeWindowsCdpStacks } from "./windows-cdp-stacks.mjs";
const identity = { Pid: 1234, Created: "2026-09-28T01:02:03.1234567Z" };
const stack = { state: "observed", frames: [{ module: "user32", symbol: "SendMessageW", offset: 42 }] };
test("stack evidence includes code locations but excludes addresses, paths and memory", async () => {
  const native = { state: "observed", app: { ...stack, path: "private/path", memory: "secret" }, browser: stack };
  const result = await observeWindowsCdpStacks(identity, 9222, {
    platform: "win32",
    helper: "C:\\probe\\stacks.exe",
    run: async (_file, _args, options) => {
      assert.equal(options.timeout, 30000);
      assert.equal(options.maxBuffer, 65536);
      return { stdout: JSON.stringify(native) };
    },
  });
  assert.deepEqual(result, { state: "observed", app: stack, browser: stack });
});
test("stack projection rejects unbounded or private native output", async () => {
  for (const frames of [
    Array(33).fill(stack.frames[0]),
    [{ module: "C:\\secret.dll", symbol: "SendMessageW", offset: 0 }],
    [{ module: "user32", symbol: "private/path", offset: 0 }],
    [{ module: "user32", symbol: "x".repeat(161), offset: 0 }],
    [{ module: "user32", symbol: null, offset: -1 }],
  ]) {
    assert.deepEqual(
      await observeWindowsCdpStacks(identity, 9222, {
        platform: "win32",
        helper: "C:\\probe\\stacks.exe",
        run: async () => ({
          stdout: JSON.stringify({ state: "observed", app: { state: "observed", frames }, browser: stack }),
        }),
      }),
      { state: "invalid_response" },
    );
  }
});
test("unsupported, missing helper, invalid ownership and failed capture remain explicit", async () => {
  assert.deepEqual(await observeWindowsCdpStacks(identity, 9222, { platform: "linux" }), { state: "unsupported" });
  assert.deepEqual(await observeWindowsCdpStacks(identity, 9222, { platform: "win32", helper: "" }), {
    state: "unavailable",
  });
  assert.deepEqual(await observeWindowsCdpStacks({ Pid: -1 }, 9222, { platform: "win32", helper: "probe.exe" }), {
    state: "invalid_owner",
  });
  assert.deepEqual(
    await observeWindowsCdpStacks(identity, 9222, {
      platform: "win32",
      helper: "probe.exe",
      run: async () => {
        throw new Error("private native error");
      },
    }),
    { state: "probe_failed" },
  );
});
