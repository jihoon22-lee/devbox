import assert from "node:assert/strict";
import test from "node:test";
import { cleanupOwnedFixture, withOwnedCleanup } from "./owned-fixture-cleanup.mjs";

function session(identity = {}) {
  const events = [];
  return {
    events,
    identity,
    child: {
      exitCode: null,
      signalCode: null,
      stderr: { destroy: () => events.push("pipe") },
      unref: () => events.push("unref"),
    },
    cdp: {},
  };
}
test("verified stop failure still releases CDP, pipes and live child handle", async () => {
  const item = session();
  const failure = new Error("owned stop failed");
  await assert.rejects(
    cleanupOwnedFixture(
      item,
      async () => {
        item.events.push("stop");
        throw failure;
      },
      () => item.events.push("release"),
    ),
    (error) => error === failure,
  );
  assert.deepEqual(item.events, ["stop", "release", "pipe", "unref"]);
});
test("identity discovery failure preserves process without stopping arbitrary PID", async () => {
  const item = session(null);
  await assert.rejects(
    cleanupOwnedFixture(
      item,
      () => assert.fail("unverified process must not be stopped"),
      () => item.events.push("release"),
    ),
    /identity unavailable/,
  );
  assert.deepEqual(item.events, ["release", "pipe", "unref"]);
});
test("normal owned stop verifies exit and does not detach an exited process", async () => {
  const item = session();
  await cleanupOwnedFixture(
    item,
    async () => {
      item.child.exitCode = 0;
      item.events.push("stop");
    },
    () => item.events.push("release"),
  );
  assert.deepEqual(item.events, ["stop", "release"]);
});
test("work failure survives a second cleanup failure including primitive and frozen errors", async () => {
  for (const failure of [null, "original", Object.freeze(new Error("original"))]) {
    await assert.rejects(
      withOwnedCleanup(
        async () => {
          throw failure;
        },
        async () => {
          throw new Error("cleanup");
        },
      ),
      (error) => error === failure,
    );
  }
});
test("cleanup failure after successful work remains failure", async () => {
  const failure = new Error("cleanup");
  await assert.rejects(
    withOwnedCleanup(
      async () => "success",
      async () => {
        throw failure;
      },
    ),
    (error) => error === failure,
  );
});

test("Agent normal-close failure preserves the original error and detaches unverified live child", async () => {
  const { closeOwnedProduct } = await import("./windows-suite-agent-user-flows.mjs");
  const failure = Object.freeze(new Error("original close failed"));
  const item = session(null);
  item.product = "workspace";
  item.cdp = { close: () => item.events.push("socket") };
  item.ui = {
    closeOwnedWindow: async () => {
      throw failure;
    },
  };
  await assert.rejects(closeOwnedProduct(item), (error) => error === failure);
  assert.deepEqual(item.events, ["pipe", "socket", "pipe", "unref"]);
  assert.equal(item.child.exitCode, null);
});
