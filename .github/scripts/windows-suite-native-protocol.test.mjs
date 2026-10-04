import assert from "node:assert/strict";
import test from "node:test";
import {
  installedFixtureCommand,
  installedFixtureOwnerRecording,
  prepareHistoricalNativeStore,
  historicalHealthUnavailable,
} from "./windows-suite-native-protocol.mjs";
test("historical health unavailable requires exact pinned source and typed owner provenance", () => {
  const problem = {
    code: "unavailable",
    provenance: {
      component: "control-center.commands",
      product: "control-center",
      revision: 13,
      requestId: "owned-request",
    },
  };
  assert.equal(historicalHealthUnavailable("1c97b41ee10ca0df7c062338bfe85659af025a89", problem), true);
  for (const changed of [
    { ...problem, code: "rejected" },
    { ...problem, provenance: { ...problem.provenance, product: "workspace" } },
    { ...problem, provenance: { ...problem.provenance, revision: 14 } },
  ]) {
    assert.equal(historicalHealthUnavailable("1c97b41ee10ca0df7c062338bfe85659af025a89", changed), false);
  }
  assert.equal(historicalHealthUnavailable("current", problem), false);
});
test("pinned historical empty-store preparation uses one visible setup input and never infers health", async () => {
  for (const [product, name] of [
    ["workspace", "빈 Workspace 시작"],
    ["knowledge", "새 저장소로 시작"],
  ]) {
    const events = [];
    const receipt = await prepareHistoricalNativeStore("1c97b41ee10ca0df7c062338bfe85659af025a89", product, false, {
      waitForTarget: async (target) => events.push(["observe", target]),
      click: async (target) => events.push(["input", target]),
    });
    assert.deepEqual(events, [
      ["observe", { role: "button", name }],
      ["input", { role: "button", name }],
    ]);
    assert.equal(receipt.evidenceKind, "historical-fixture-preparation");
    assert.equal(receipt.promotionEvidence, false);
    assert.equal("nativeStoreReady" in receipt, false);
  }
});
test("current, unknown, already prepared and other products get no historical setup input", async () => {
  const ui = {
    waitForTarget: async () => assert.fail("Unexpected preparation"),
    click: async () => assert.fail("Unexpected input"),
  };
  for (const [source, product, ready] of [
    ["c91d0326ef145e9f4519843b27a98449acee0933", "workspace", false],
    ["unknown", "knowledge", false],
    ["1c97b41ee10ca0df7c062338bfe85659af025a89", "workspace", true],
    ["1c97b41ee10ca0df7c062338bfe85659af025a89", "api-studio", false],
  ]) {
    assert.equal(await prepareHistoricalNativeStore(source, product, ready, ui), null);
  }
});
test("only pinned v0.8.1 uses its historical execute command for delivery fixture preparation", () => {
  const delivery = "plugin:control-center|delivery";
  assert.equal(
    installedFixtureCommand(delivery, "1c97b41ee10ca0df7c062338bfe85659af025a89"),
    "plugin:control-center|execute",
  );
  for (const source of [
    "e499ac7127269bf67863bf0fdc42eaf53236b9f3",
    "c91d0326ef145e9f4519843b27a98449acee0933",
    "unknown",
  ])
    assert.equal(installedFixtureCommand(delivery, source), delivery);
  assert.equal(
    installedFixtureCommand("plugin:suite|connection", "1c97b41ee10ca0df7c062338bfe85659af025a89"),
    "plugin:suite|connection",
  );
});

test("published historical import records owner readiness before separate health activation", () => {
  const pinned = "1c97b41ee10ca0df7c062338bfe85659af025a89";
  assert.deepEqual(installedFixtureOwnerRecording(pinned, "import"), {
    method: "record_migration_owner",
    route: "migration",
  });
  for (const [source, mode] of [
    [pinned, "health"],
    ["current-candidate", "import"],
    ["current-candidate", "health"],
  ])
    assert.deepEqual(installedFixtureOwnerRecording(source, mode), {
      method: "record_suite_health",
      route: "recovery",
    });
});
