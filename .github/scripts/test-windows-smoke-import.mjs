import assert from "node:assert/strict";

const before = [process.listenerCount("SIGINT"), process.listenerCount("SIGTERM")];
const helpers = await import("./windows-packaged-smoke.mjs");
for (const name of ["windowsProcessIsElevated", "inspectElevatedCdpPolicy", "installElevatedCdpPolicy", "restoreElevatedCdpPolicy"]) {
  assert.equal(typeof helpers[name], "function");
}
assert.deepEqual([process.listenerCount("SIGINT"), process.listenerCount("SIGTERM")], before);
assert.equal(process.exitCode, undefined);
console.log("Packaged smoke helpers import without running acceptance or changing signal handlers: PASS");
