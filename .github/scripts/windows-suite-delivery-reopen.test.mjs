import assert from "node:assert/strict";
import test from "node:test";
import { observeDeliveryReopen } from "./windows-suite-delivery-user-flows.mjs";
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
