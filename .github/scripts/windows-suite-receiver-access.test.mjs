import assert from "node:assert/strict";
import test from "node:test";
import { withUnavailableReceiver } from "./windows-suite-receiver-access.mjs";

test("receiver access is restored after successful and failed transfer probes", async () => {
  for (const fails of [false, true]) {
    const events = [];
    const run = withUnavailableReceiver(
      async (action) => events.push(action),
      async () => {
        events.push("probe");
        if (fails) throw new Error("original transfer failure");
      },
    );
    if (fails) await assert.rejects(run, /original transfer failure/);
    else await run;
    assert.deepEqual(events, ["Deny", "probe", "Restore"]);
  }
});
test("failed preparation never probes or attempts nonexistent restoration", async () => {
  const events = [];
  await assert.rejects(
    withUnavailableReceiver(
      async (action) => {
        events.push(action);
        throw new Error("access preparation failed");
      },
      async () => events.push("probe"),
    ),
    /access preparation failed/,
  );
  assert.deepEqual(events, ["Deny"]);
});
test("restoration failure cannot pass", async () => {
  await assert.rejects(
    withUnavailableReceiver(
      async (action) => {
        if (action === "Restore") throw new Error("restoration failed");
      },
      async () => {},
    ),
    /restoration failed/,
  );
});
test("restoration failure preserves the original transfer error", async () => {
  await assert.rejects(
    withUnavailableReceiver(
      async (action) => {
        if (action === "Restore") throw new Error("restoration failed");
      },
      async () => {
        throw new Error("original transfer failure");
      },
    ),
    /original transfer failure/,
  );
});
