import assert from "node:assert/strict";
import test from "node:test";
import { runKnowledgeChecks } from "./knowledge-diagnostic-sequence.mjs";
test("candidate fails immediately on the original required gate without continuation", async () => {
  const original = new Error("performance failed");
  const events = [];
  await assert.rejects(
    runKnowledgeChecks({
      diagnosticOnly: false,
      isUsable: () => true,
      onFirstFailure: async () => events.push("preserve"),
      checks: [
        {
          name: "performance",
          run: async () => {
            events.push("performance");
            throw original;
          },
        },
        { name: "input", run: async () => events.push("input") },
        { name: "documents", run: async () => events.push("documents") },
        { name: "activity", run: async () => events.push("activity") },
      ],
    }),
    (error) => error === original,
  );
  assert.deepEqual(events, ["performance"]);
});
test("diagnosis preserves first failure before independent scenarios and retains original gate error", async () => {
  const original = new Error("performance failed");
  const events = [];
  const observed = await runKnowledgeChecks({
    diagnosticOnly: true,
    isUsable: () => true,
    onFirstFailure: async (error) => {
      assert.equal(error, original);
      events.push("preserve");
    },
    checks: [
      {
        name: "performance",
        run: async () => {
          events.push("performance");
          throw original;
        },
      },
      {
        name: "input",
        run: async () => {
          events.push("input");
          throw new Error("zoom disabled");
        },
      },
      { name: "documents", run: async () => events.push("documents") },
      { name: "activity", run: async () => events.push("activity") },
    ],
  });
  assert.equal(observed.firstError, original);
  assert.deepEqual(events, ["performance", "preserve", "input", "documents", "activity"]);
  assert.deepEqual(observed.failed, ["performance", "input"]);
});
test("diagnosis stops when owned context is no longer usable without marking skipped checks complete", async () => {
  const original = new Error("input lost window");
  let usable = true;
  const events = [];
  const observed = await runKnowledgeChecks({
    diagnosticOnly: true,
    isUsable: () => usable,
    onFirstFailure: async () => events.push("preserve"),
    checks: [
      {
        name: "input",
        run: async () => {
          usable = false;
          throw original;
        },
      },
      { name: "documents", run: async () => events.push("documents") },
    ],
  });
  assert.equal(observed.firstError, original);
  assert.deepEqual(events, ["preserve"]);
  assert.deepEqual(observed.completed, []);
});
