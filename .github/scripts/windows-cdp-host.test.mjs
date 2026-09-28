import assert from "node:assert/strict";
import test from "node:test";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { createServer } from "node:net";
import { once } from "node:events";
import { observeWindowsCdpHost } from "./windows-cdp-host.mjs";

const owner = { Pid: 1234, Created: "2026-09-28T01:02:03.1234567Z", Path: "private/path" };
test("Windows observes the owned test process and its own ephemeral listener", {
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
    const result = await observeWindowsCdpHost({ Pid: process.pid, Created: stdout.trim() }, server.address().port);
    assert.equal(result.state, "observed", JSON.stringify(result));
    assert.equal(result.listener, "owned");
    assert.equal(result.webviewProcesses, 0);
    assert.ok(result.totalMemoryMiB > 0);
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});
const observed = {
  state: "observed",
  ownerWindowPresent: true,
  ownerResponding: false,
  webviewProcesses: 4,
  listener: "owned",
  freeMemoryMiB: 1024,
  totalMemoryMiB: 8192,
};
test("the host probe uses the exact owner creation time and emits only fixed bounded metadata", async () => {
  let calls = 0;
  const run = async (file, args, options) => {
    calls++;
    assert.equal(file, "powershell.exe");
    assert.ok(args.at(-1).includes(owner.Created));
    assert.ok(!args.at(-1).includes(owner.Path));
    assert.equal(options.timeout, 10000);
    assert.equal(options.maxBuffer, 16384);
    return { stdout: JSON.stringify({ ...observed, commandLine: "private secret" }) };
  };
  assert.deepEqual(await observeWindowsCdpHost(owner, 9222, { platform: "win32", run }), observed);
  assert.equal(calls, 1);
});
test("malformed ownership and non-Windows hosts cannot launch the native probe", async () => {
  const run = async () => {
    throw new Error("must not execute");
  };
  assert.deepEqual(
    await observeWindowsCdpHost({ ...owner, Created: "'; injected" }, 9222, { platform: "win32", run }),
    { state: "invalid_owner" },
  );
  assert.deepEqual(await observeWindowsCdpHost(owner, 70000, { platform: "win32", run }), { state: "invalid_owner" });
  assert.deepEqual(await observeWindowsCdpHost(owner, 9222, { platform: "linux", run }), { state: "unsupported" });
});
test("identity changes, invalid native output and probe errors never expose raw details", async () => {
  for (const [value, expected] of [
    [{ state: "identity_changed", Path: "private" }, { state: "identity_changed" }],
    [{ ...observed, listener: "private" }, { state: "invalid_response" }],
    [{ ...observed, webviewProcesses: 1000000 }, { state: "invalid_response" }],
    [{ ...observed, ownerResponding: "private" }, { state: "invalid_response" }],
  ]) {
    assert.deepEqual(
      await observeWindowsCdpHost(owner, 9222, {
        platform: "win32",
        run: async () => ({ stdout: JSON.stringify(value) }),
      }),
      expected,
    );
  }
  assert.deepEqual(
    await observeWindowsCdpHost(owner, 9222, {
      platform: "win32",
      run: async () => {
        throw new Error("private stderr");
      },
    }),
    { state: "probe_failed" },
  );
});
