import assert from "node:assert/strict";
import test from "node:test";
import * as document from "./windows-knowledge-document-recovery.mjs";
import * as search from "./windows-knowledge-search-lifecycle.mjs";
import * as activity from "./windows-knowledge-activity.mjs";
for (const module of [document, search, activity])
  test(`${module.scenarioIds.join(",")} cannot fabricate Windows PASS without an owned fixture`, async () => {
    const results = await module.run({
      sourceSha: "a".repeat(40),
      fixtureSha: "b".repeat(40),
      artifactDigests: { knowledge: "c".repeat(64) },
    });
    assert.deepEqual(
      results.map((r) => r.id),
      module.scenarioIds,
    );
    assert.ok(results.every((r) => r.status === "NOT_RUN" && r.failureCode));
  });

test("search navigation waits for readonly accessible field readiness before returning", async () => {
  const events = [];
  let ready;
  const pending = new Promise((resolve) => {
    ready = resolve;
  });
  const target = { role: "textbox", name: "검색 루트 경로" };
  const navigation = search.navigateSearch(
    {
      knowledgeFixture: {
        navigate: async (route) => {
          events.push(route);
        },
      },
      ui: {
        waitForTarget: async (field) => {
          assert.deepEqual(field, target);
          events.push("AX observation");
          await pending;
        },
      },
    },
    target,
  );
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(events, ["search", "AX observation"]);
  let completed = false;
  navigation.then(() => {
    completed = true;
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(completed, false);
  ready();
  await navigation;
  assert.equal(completed, true);
});
