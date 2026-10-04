import { test } from "node:test";
import assert from "node:assert/strict";
import { waitForOwnedHealthConnections, projectHealthRows } from "./windows-suite-health-readiness.mjs";
const products = ["control-center", "workspace", "api-studio", "knowledge"];
const connected = (id = 0) => ({
  product: products[id],
  connected: true,
  connectionState: "connected",
  generation: "a".repeat(64),
  issue: null,
});
test("waits for native startup before the one visible health action", async () => {
  let tick = 0,
    clicks = 0;
  const contexts = [0, 1, 2, 3];
  const observe = async (check) => {
    assert.equal(await check(), false);
    assert.equal(clicks, 0);
    tick++;
    assert.equal(await check(), true);
  };
  await waitForOwnedHealthConnections(contexts, observe, async (id) =>
    tick || id
      ? connected(id)
      : { product: products[id], connected: false, connectionState: "preparing", generation: null },
  );
  clicks++;
  assert.equal(clicks, 1);
});
test("off, failed and foreign generations never authorize a health action", async () => {
  for (const bad of [
    { ...connected(), connectionState: "off" },
    { ...connected(), connectionState: "failed", issue: "suite_connection_failed" },
    { ...connected(), generation: "b".repeat(64) },
  ]) {
    await assert.rejects(
      waitForOwnedHealthConnections(
        [0, 1, 2, 3],
        async (check) => {
          await check();
        },
        async (id) => (id ? connected(id) : bad),
      ),
    );
  }
});

test("health failure rows retain only fixed product/status and version metadata", () => {
  const rows = projectHealthRows([
    ["Devbox Workspace", "0.9.0", "응답·저장소 선택 확인됨"],
    ["private title", "private path", "private content"],
  ]);
  assert.deepEqual(rows, [
    { product: "workspace", version: "0.9.0", status: "응답·저장소 선택 확인됨" },
    { product: null, version: null, status: null },
  ]);
});

test("duplicate products cannot substitute for the four owned identities", async () => {
  await assert.rejects(
    waitForOwnedHealthConnections(
      [0, 1, 2, 3],
      async (check) => {
        await check();
      },
      async () => connected(),
    ),
  );
});
