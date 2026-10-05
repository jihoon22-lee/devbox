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

test("performance evidence captures the current measured renderer and persists its PNG", async () => {
  const { mkdtemp, readFile, rm } = await import("node:fs/promises");
  const { tmpdir } = await import("node:os");
  const path = await import("node:path");
  const { capturePerformanceScreenshot } = await import("./windows-suite-layout.mjs");
  const directory = await mkdtemp(path.join(tmpdir(), "devbox-performance-capture-"));
  const bytes = Buffer.from("89504e470d0a1a0a00000000", "hex");
  const calls = [];
  try {
    const paths = await capturePerformanceScreenshot(
      {
        command: async (...args) => {
          calls.push(args);
          return { data: bytes.toString("base64") };
        },
      },
      "workspace",
      directory,
    );
    assert.deepEqual(calls, [["Page.captureScreenshot", { format: "png" }]]);
    assert.equal(paths.length, 1);
    assert.deepEqual(await readFile(paths[0]), bytes);
    await assert.rejects(
      capturePerformanceScreenshot(
        {
          command: async () => {
            throw new Error("capture failed");
          },
        },
        "workspace",
        directory,
      ),
      /capture failed/,
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("successful layout status includes the explicit null required by the release validator", async () => {
  const { layoutEvidenceStatus } = await import("./windows-suite-layout.mjs");
  const { summarizeEvidence } = await import("./suite-user-flow-evidence.mjs");
  const digest = { image: "a".repeat(64) },
    source = "b".repeat(40);
  const row = { id: "PERF-01", evidenceKind: "packaged-ui" };
  const result = {
    ...row,
    ...layoutEvidenceStatus([], "missing"),
    sourceSha: source,
    fixtureSha: source,
    artifactDigests: digest,
    assertions: ["measured"],
    screenshotPaths: ["performance.png"],
  };
  assert.equal(
    summarizeEvidence([row], [result], { expectedSource: source, expectedFixture: source, expectedDigests: digest })
      .ready,
    true,
  );
  assert.deepEqual(layoutEvidenceStatus(["knowledge"], "missing"), {
    status: "NOT_RUN",
    failureCode: "missing",
    missing: ["knowledge"],
  });
});
