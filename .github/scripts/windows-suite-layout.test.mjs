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

test("size failure persists screenshot and screen/native metrics before rejecting", async () => {
  const { persistMeasuredLayout } = await import("./windows-suite-layout.mjs");
  const events = [];
  const observation = {
    size: { name: "default", width: 1180, height: 780 },
    requestedNativeSize: { width: 1196, height: 819 },
    nativeResizeRequests: [
      { width: 1180, height: 780 },
      { width: 1196, height: 819 },
    ],
    windowMetrics: {
      width: 1024,
      height: 720,
      outerWidth: 1040,
      outerHeight: 759,
      screen: { width: 1024, height: 768, availHeight: 728 },
      pixelRatio: 1,
    },
    nativeWindow: { selectedWindow: { bounds: { width: 1040, height: 759 } } },
    observed: {
      viewport: { width: 1024, height: 720 },
      main: { width: 800 },
      notice: null,
      editor: null,
      horizontalOverflow: false,
      clippedPrimaryControls: [],
    },
  };
  await assert.rejects(
    persistMeasuredLayout(observation, {
      screenshot: async () => {
        events.push("screenshot");
        return "failure.png";
      },
      persist: async (saved) => {
        events.push("persist");
        assert.equal(saved.screenshotPath, "failure.png");
        assert.deepEqual(saved.windowMetrics, observation.windowMetrics);
      },
    }),
    /Actual client dimensions/,
  );
  assert.deepEqual(events, ["screenshot", "persist"]);
});
