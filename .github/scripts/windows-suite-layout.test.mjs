import assert from "node:assert/strict";
import test from "node:test";
import { correctClientSize } from "./windows-suite-layout.mjs";
test("native outer resize adds measured frame deficit in physical pixels", () => {
  assert.deepEqual(
    correctClientSize({ width: 1475, height: 975 }, { width: 1164, height: 741 }, { width: 1180, height: 780 }, 1.25),
    { width: 1495, height: 1024 },
  );
});
test("an OS-clamped exact minimum needs no larger native request", () => {
  assert.equal(
    correctClientSize({ width: 720, height: 480 }, { width: 720, height: 480 }, { width: 720, height: 480 }, 1),
    null,
  );
});
test("unsafe or unavailable client observations fail closed", () => {
  assert.throws(() =>
    correctClientSize({ width: 2500, height: 1500 }, { width: 500, height: 100 }, { width: 1180, height: 780 }, 2),
  );
  assert.throws(() =>
    correctClientSize({ width: 1180, height: 780 }, { width: NaN, height: 700 }, { width: 1180, height: 780 }, 1),
  );
});
