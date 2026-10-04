import assert from "node:assert/strict";
import test from "node:test";
import * as document from "./windows-knowledge-document-recovery.mjs";
import * as search from "./windows-knowledge-search-lifecycle.mjs";
import * as activity from "./windows-knowledge-activity.mjs";
test("Knowledge native close observes the async review before one explicit decision", async () => {
  const events = [];
  let release;
  const pending = new Promise((resolve) => {
    release = resolve;
  });
  const journey = document.reviewKnowledgeQuit(
    {
      closeOwnedWindow: async () => events.push("native close"),
      waitForTarget: async (target) => {
        assert.deepEqual(target, { role: "button", name: "종료 취소" });
        events.push("readonly review");
        await pending;
      },
      click: async (target) => events.push(target.name),
    },
    "종료 취소",
  );
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(events, ["native close", "readonly review"]);
  release();
  await journey;
  assert.deepEqual(events, ["native close", "readonly review", "종료 취소"]);
});
import { visibleNoteTextExpression, noteReadyExpression } from "./windows-knowledge-flow-shared.mjs";
test("note readiness rejects a previous textbox until exact requested path and bytes are rendered", () => {
  const probe = Function("document", `return ${noteReadyExpression("Notes/B.md", "B\n")}`);
  for (const [path, content, expected] of [
    ["Notes/A.md", "B\n", false],
    ["Notes/B.md", "A\n", false],
    ["Notes/B.md", "B\n", true],
  ]) {
    const document = {
      querySelector: () => ({ textContent: path }),
      querySelectorAll: () => [{ querySelectorAll: () => content.split("\n").map((textContent) => ({ textContent })) }],
    };
    assert.equal(probe(document), expected);
  }
});
test("synthetic note observation preserves exactly the rendered trailing blank lines", () => {
  for (const lines of [["# 합성 A", ""], [""], ["a", "", ""]]) {
    const document = {
      querySelectorAll: () => [{ querySelectorAll: () => lines.map((textContent) => ({ textContent })) }],
    };
    const observed = Function("document", `return ${visibleNoteTextExpression()}`)(document);
    assert.equal(observed, lines.join("\n"));
  }
});
test("empty recovery assertions reject missing, ambiguous or unrendered editors", () => {
  const probe = Function("document", `return ${visibleNoteTextExpression()}`);
  for (const editors of [[], [{}, {}], [{ querySelectorAll: () => [] }]]) {
    assert.throws(() => probe({ querySelectorAll: () => editors }), /unavailable/);
  }
});
import { reachKnowledgeControl, preserveKnowledgeInputBaselineOnFailure } from "./windows-knowledge-input-ui.mjs";
test("failed Knowledge input preserves first evidence before restoring fixture and rethrows its original gate", async () => {
  const error = new Error("original gate");
  const events = [];
  await assert.rejects(
    preserveKnowledgeInputBaselineOnFailure(
      async () => {
        throw error;
      },
      async () => events.push("first screenshot"),
      async () => events.push("baseline restored"),
    ),
    (observed) => observed === error,
  );
  assert.deepEqual(events, ["first screenshot", "baseline restored"]);
  await assert.rejects(
    preserveKnowledgeInputBaselineOnFailure(
      async () => {
        throw error;
      },
      async () => {},
      async () => {
        throw new Error("restore failed");
      },
    ),
    (observed) => observed === error && observed.inputBaselineRestoreFailed === true,
  );
});
test("rename keyboard traversal goes backward from toolbar without entering the Tab-indent editor", async () => {
  let focus = "autosave";
  const pressed = [];
  await reachKnowledgeControl(
    {
      press: async (key) => {
        pressed.push(key);
        focus = key === "Shift+Tab" ? "rename" : "editor";
      },
    },
    async () => focus === "rename",
    "rename predicate",
    "Shift+Tab",
  );
  assert.deepEqual(pressed, ["Shift+Tab"]);
});
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
test("recovery click waits for exact restored note readiness before returning", async () => {
  const events = [];
  let ready;
  const pending = new Promise((resolve) => {
    ready = resolve;
  });
  const opening = document.openKnowledgeRecoveryNote(
    {
      waitForTarget: async (target) => events.push(["target", target.name]),
      click: async (target) => events.push(["click", target.name]),
    },
    {
      waitNote: async (path, content) => {
        events.push(["readonly exact note", path, content]);
        await pending;
      },
    },
    "Notes/synthetic-A.md",
    "",
  );
  let settled = false;
  opening.then(() => {
    settled = true;
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(settled, false);
  assert.deepEqual(events, [
    ["target", "Notes/synthetic-A.md 열어서 확인"],
    ["click", "Notes/synthetic-A.md 열어서 확인"],
    ["readonly exact note", "Notes/synthetic-A.md", ""],
  ]);
  ready();
  await opening;
  assert.equal(settled, true);
});
import { assertKnowledgeDraft } from "./windows-suite-delivery-user-flows.mjs";
test("delivery draft assertions use exact rendered document bytes and reject real extra newlines", async () => {
  const context = {
    ui: {
      text: () => {
        throw new Error("AX serialization must not supply document bytes");
      },
    },
    knowledgeFixture: { editorText: async () => "synthetic draft\n" },
  };
  await assertKnowledgeDraft(context, "synthetic draft\n");
  context.knowledgeFixture.editorText = async () => "synthetic draft\n\n";
  await assert.rejects(assertKnowledgeDraft(context, "synthetic draft\n"), assert.AssertionError);
});
