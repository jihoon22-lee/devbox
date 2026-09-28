import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { createServer } from "node:net";
import { once } from "node:events";
import assert from "node:assert/strict";
import test from "node:test";
import { observeWindowsCdpWaits } from "./windows-cdp-waits.mjs";
const identity = { Pid: 1234, Created: "2026-09-28T01:02:03.1234567Z" };
const chain = {
  state: "observed",
  cycle: true,
  nodes: [{ type: 8, status: 3, processRole: "browser", threadRole: "sample" }],
};
const sample = {
  state: "observed",
  sampleMs: 250,
  appCpuMs: 0,
  listenerCpuMs: 0,
  listenerWorkingSetMiB: 80,
  listenerThreadCount: 1,
  debugAccess: true,
  appUi: chain,
  threads: [{ index: 0, state: 5, waitReason: 5, cpuMs: 0, chain }],
  truncated: false,
};
test("wait evidence excludes object names, IDs and unbounded native metadata", async () => {
  const native = structuredClone(sample);
  native.threads[0].chain.nodes[0].ObjectName = "private/path/token";
  native.threads[0].ThreadId = 123;
  const run = async (_file, _args, options) => {
    assert.equal(options.timeout, 15000);
    assert.equal(options.maxBuffer, 131072);
    return { stdout: JSON.stringify(native) };
  };
  assert.deepEqual(await observeWindowsCdpWaits(identity, 9222, { platform: "win32", run }), sample);
});
test("invalid ownership and oversized wait chains fail closed", async () => {
  let called = false;
  const run = async () => {
    called = true;
    return { stdout: "{}" };
  };
  assert.deepEqual(await observeWindowsCdpWaits({ ...identity, Pid: -1 }, 9222, { platform: "win32", run }), {
    state: "invalid_owner",
  });
  assert.equal(called, false);
  const native = structuredClone(sample);
  native.threads[0].chain.nodes = Array(17).fill(chain.nodes[0]);
  assert.deepEqual(
    await observeWindowsCdpWaits(identity, 9222, {
      platform: "win32",
      run: async () => ({ stdout: JSON.stringify(native) }),
    }),
    { state: "invalid_response" },
  );
});

test("Windows wait-chain inspection samples only the owned listener", {
  skip: process.platform !== "win32",
}, async () => {
  const { stdout } = await promisify(execFile)(
    "powershell.exe",
    [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      `(Get-CimInstance Win32_Process -Filter 'ProcessId=${process.pid}').CreationDate.ToUniversalTime().ToString('o')`,
    ],
    { timeout: 10000, maxBuffer: 4096, windowsHide: true },
  );
  const server = createServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  try {
    const result = await observeWindowsCdpWaits({ Pid: process.pid, Created: stdout.trim() }, server.address().port);
    assert.equal(result.state, "observed", JSON.stringify(result));
    assert.ok(result.listenerThreadCount > 0);
    assert.ok(result.threads.length > 0);
    assert.ok(
      result.threads.some((thread) => thread.chain.state === "observed"),
      JSON.stringify(result),
    );
    assert.equal(result.appUi, null);
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});
