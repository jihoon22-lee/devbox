import assert from "node:assert/strict";
import test from "node:test";
import * as document from "./windows-knowledge-document-recovery.mjs";
import * as search from "./windows-knowledge-search-lifecycle.mjs";
import * as activity from "./windows-knowledge-activity.mjs";
test("search removal uses native normalized path as its exact accessible name", async () => {
  const stored = "D:/fixture/knowledge-search-watch-owned";
  assert.deepEqual(
    await search.searchRootRemovalTarget([{ path: stored }], "D:\\fixture\\knowledge-search-watch-owned"),
    {
      role: "button",
      name: `${stored} 루트 제거`,
    },
  );
  await assert.rejects(
    search.searchRootRemovalTarget([{ path: stored }], "D:\\fixture\\unowned"),
    /Owned search root missing/,
  );
});
test("activity navigation observes readiness before one settings input", async () => {
  const events = [];
  let release;
  const pending = new Promise((resolve) => {
    release = resolve;
  });
  const ui = {
    click: async (target) => {
      events.push(target.name);
    },
    waitForTarget: async (target) => {
      assert.deepEqual(target, { role: "button", name: "설정" });
      events.push("readonly readiness");
      await pending;
    },
  };
  const journey = (async () => {
    await activity.navigateKnowledgeRoute(ui, "activity", "활동");
    await ui.click({ role: "button", name: "설정" });
  })();
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(events, ["활동", "readonly readiness"]);
  release();
  await journey;
  assert.deepEqual(events, ["활동", "readonly readiness", "설정"]);
});

test("other Knowledge navigation keeps one input without an added readiness target", async () => {
  for (const route of ["notes", "search", "daily"]) {
    const events = [];
    await activity.navigateKnowledgeRoute(
      {
        click: async (target) => {
          events.push(target);
        },
        waitForTarget: async () => {
          assert.fail("Unexpected readiness wait");
        },
      },
      route,
      route,
    );
    assert.deepEqual(events, [{ role: "button", name: route }]);
  }
});
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
