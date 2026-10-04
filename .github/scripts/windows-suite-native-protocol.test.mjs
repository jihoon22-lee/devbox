import assert from "node:assert/strict";
import test from "node:test";
import { installedFixtureCommand } from "./windows-suite-native-protocol.mjs";
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
