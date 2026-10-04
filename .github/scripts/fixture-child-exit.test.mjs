import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import test from "node:test";
import { waitForFixtureChildExit } from "./fixture-child-exit.mjs";
function child() {
  const value = new EventEmitter();
  value.exitCode = null;
  value.signalCode = null;
  value.kills = 0;
  value.kill = () => {
    value.kills++;
    return false;
  };
  return value;
}
test("deadline rejects even when owned kill fails and no exit event arrives", async () => {
  const value = child();
  const released = [];
  value.stdout = { destroy: () => released.push("stdout") };
  value.stderr = { destroy: () => released.push("stderr") };
  value.unref = () => released.push("owned handle");
  await assert.rejects(waitForFixtureChildExit(value, 10, 10), { code: "ETIMEDOUT" });
  assert.equal(value.kills, 1);
  assert.deepEqual(released, ["stdout", "stderr", "owned handle"]);
  assert.equal(value.listenerCount("exit"), 0);
  assert.equal(value.listenerCount("error"), 0);
});
test("actual exit codes and original process errors are preserved", async () => {
  const value = child();
  const exit = waitForFixtureChildExit(value, 1000);
  value.emit("exit", 7, null);
  assert.deepEqual(await exit, [7, null]);
  const failed = child();
  const error = new Error("original process error");
  const pending = waitForFixtureChildExit(failed, 1000);
  failed.emit("error", error);
  await assert.rejects(pending, (observed) => observed === error);
  assert.equal(failed.kills, 0);
});
