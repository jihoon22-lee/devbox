import assert from "node:assert/strict";
import test from "node:test";
import { installedFixtureCommand, prepareHistoricalNativeStore } from "./windows-suite-native-protocol.mjs";
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
