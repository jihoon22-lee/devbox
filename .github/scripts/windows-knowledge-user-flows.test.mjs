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
    { wait: async (observe) => assert.equal(await observe(), true), quitReviewOpen: async () => false },
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

test("Activity draft review uses the actual dialog and cancels before route navigation", async () => {
  const events = [];
  const ui = {
    waitForTarget: async (target) => events.push(["observe", target.role, target.name]),
    click: async (target) => events.push(["click", target.name]),
  };
  const fixture = { waitBody: async (text) => events.push(["closed", text]) };
  await activity.reviewActivityDraft(ui, fixture);
  assert.deepEqual(events, [
    ["observe", "dialog", "Life Log 초안 미리보기"],
    ["click", "취소"],
    ["closed", "Knowledge 초안 미리보기를 취소했습니다"],
  ]);
});

test("Activity regeneration refreshes its history section before waiting for an actionable row", async () => {
  const events = [];
  await activity.refreshActivityHistory({
    waitForTarget: async (target) => events.push(["ready", target]),
    click: async (target) => events.push(["click", target]),
  });
  const target = { role: "button", name: "새로 고침", scope: { role: "region", name: "Knowledge 초안 handoff 기록" } };
  assert.deepEqual(events, [
    ["ready", target],
    ["click", target],
    ["ready", { role: "button", name: "다시 생성" }],
  ]);
});

test("opening an existing Knowledge note resolves the current native root without prepareNotes", async () => {
  const { readCurrentKnowledgeNote } = await import("./windows-knowledge-flow-shared.mjs");
  const events = [];
  let root = "/owned/imported-vault";
  const observeRoot = async () => {
    events.push("native root");
    return root;
  };
  const read = async (file, encoding) => {
    events.push([file, encoding]);
    return "existing note";
  };
  assert.equal(await readCurrentKnowledgeNote(observeRoot, "Notes/legacy.md", read), "existing note");
  root = "/owned/reconnected-vault";
  await readCurrentKnowledgeNote(observeRoot, "Notes/legacy.md", read);
  assert.deepEqual(events, [
    "native root",
    ["/owned/imported-vault/Notes/legacy.md", "utf8"],
    "native root",
    ["/owned/reconnected-vault/Notes/legacy.md", "utf8"],
  ]);
});

test("Activity stop waits for both native tracking and consent acknowledgements after one click", async () => {
  const events = [];
  const observations = [
    { tracking: true, consent: true },
    { tracking: false, consent: true },
    { tracking: false, consent: false },
  ];
  await activity.stopActivityTracking(
    {
      click: async (target) => {
        assert.deepEqual(target, { role: "button", name: "추적 중지" });
        events.push("click");
      },
    },
    {
      collectionStatus: async () => {
        events.push("observe");
        return observations.shift();
      },
      wait: async (check) => {
        assert.equal(await check(), false);
        assert.equal(await check(), false);
        assert.equal(await check(), true);
      },
    },
  );
  assert.deepEqual(events, ["click", "observe", "observe", "observe"]);
});

test("Activity stop propagates bounded acknowledgement timeout without re-clicking", async () => {
  let clicks = 0;
  const timeout = new Error("native acknowledgement timed out");
  await assert.rejects(
    activity.stopActivityTracking(
      { click: async () => clicks++ },
      {
        collectionStatus: async () => ({ tracking: false, consent: true }),
        wait: async (check) => {
          assert.equal(await check(), false);
          assert.equal(await check(), false);
          throw timeout;
        },
      },
    ),
    (error) => error === timeout,
  );
  assert.equal(clicks, 1);
});

test("Knowledge cancel waits for modal removal after native ACK without repeating the click", async () => {
  let open = true;
  let clicks = 0;
  let observe;
  let release;
  const waiting = new Promise((resolve) => {
    release = resolve;
  });
  const journey = document.reviewKnowledgeQuit(
    {
      closeOwnedWindow: async () => {},
      waitForTarget: async () => {},
      click: async () => {
        clicks++;
      },
    },
    "종료 취소",
    {
      quitReviewOpen: async () => open,
      wait: async (probe) => {
        observe = probe;
        assert.equal(await probe(), false);
        await waiting;
        assert.equal(await probe(), true);
      },
    },
  );
  let finished = false;
  void journey.then(() => {
    finished = true;
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(finished, false);
  assert.equal(clicks, 1);
  assert.equal(await observe(), false);
  open = false;
  release();
  await journey;
  assert.equal(clicks, 1);
});
test("Knowledge cancel observation timeout remains a failure without retrying the decision", async () => {
  let clicks = 0;
  await assert.rejects(
    document.reviewKnowledgeQuit(
      {
        closeOwnedWindow: async () => {},
        waitForTarget: async () => {},
        click: async () => {
          clicks++;
        },
      },
      "종료 취소",
      {
        quitReviewOpen: async () => true,
        wait: async (observe) => {
          assert.equal(await observe(), false);
          throw new Error("cancel ACK timeout");
        },
      },
    ),
    /cancel ACK timeout/,
  );
  assert.equal(clicks, 1);
});
