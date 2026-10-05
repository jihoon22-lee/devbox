import assert from "node:assert/strict";
import test from "node:test";
import { finalizeDeliveryContexts, observeDeliveryReopen } from "./windows-suite-delivery-user-flows.mjs";

test("delivery cleanup retains original failure rows and attempts both owned closes", async () => {
  const rows = [
    { id: "INSTALL-03", status: "PASS", assertions: ["cancel preserved draft"], failureCode: null },
    { id: "DELIVERY-02", status: "FAIL", assertions: ["original journey failure"], failureCode: "delivery-ui-failed" },
  ];
  const closed = [];
  const context = (name) => ({
    close: async () => {
      closed.push(name);
      throw new Error("private cleanup payload");
    },
  });
  await finalizeDeliveryContexts(context("center"), context("knowledge"), rows);
  assert.deepEqual(closed, ["center", "knowledge"]);
  assert.equal(rows.length, 2);
  assert.equal(rows[0].status, "PASS");
  assert.equal(rows[1].failureCode, "delivery-ui-failed");
  assert.deepEqual(rows[1].assertions, [
    "original journey failure",
    "Owned control-center cleanup failed",
    "Owned knowledge cleanup failed",
  ]);
  assert.equal(JSON.stringify(rows).includes("private cleanup payload"), false);
});

test("cleanup failure makes successful delivery or diagnostic evidence fail without duplicate rows", async () => {
  for (const id of ["DELIVERY-02", "CHECKPOINT-DIAGNOSTIC"]) {
    const rows = [{ id, status: "PASS", assertions: ["completed journey"], failureCode: null }];
    await finalizeDeliveryContexts(
      {
        close: async () => {
          throw new Error("cleanup failed");
        },
      },
      null,
      rows,
    );
    assert.equal(rows.length, 1);
    assert.equal(rows[0].status, "FAIL");
    assert.equal(rows[0].failureCode, "delivery-cleanup-failed");
    assert.deepEqual(rows[0].assertions, ["completed journey", "Owned control-center cleanup failed"]);
  }
});

test("successful delivery cleanup keeps evidence unchanged", async () => {
  const rows = [{ id: "DELIVERY-02", status: "PASS", assertions: ["complete"], failureCode: null }];
  const before = structuredClone(rows);
  await finalizeDeliveryContexts({ close: async () => {} }, null, rows);
  assert.deepEqual(rows, before);
});
test("reopened Center requires exact product image; helper alone never satisfies observation", async () => {
  const center = { executable: "C:\\owned\\devbox-control-center.exe" };
  let observed = 0;
  await observeDeliveryReopen(
    center,
    async (check, label, deadline) => {
      assert.equal(label, "helper reopened Center");
      assert.equal(deadline, 90000);
      assert.equal(await check(), false);
      assert.equal(await check(), true);
    },
    () => (++observed === 1 ? [{ Path: "C:\\owned\\devbox-suite-bootstrap.exe" }] : [{ Path: center.executable }]),
    () => assert.fail("success must not inspect failure"),
  );
});
test("snapshot reopen timeout preserves original and inspects helper before caller cleanup", async () => {
  const original = Object.freeze(new Error("reopen timeout"));
  let captures = 0;
  await assert.rejects(
    observeDeliveryReopen(
      {},
      async () => {
        throw original;
      },
      () => [],
      async (center, error, id) => {
        assert.equal(error, original);
        assert.match(id, /^[a-f0-9-]{36}$/);
        captures++;
        throw new Error("observation unavailable");
      },
    ),
    (error) => error === original,
  );
  assert.equal(captures, 1);
});
