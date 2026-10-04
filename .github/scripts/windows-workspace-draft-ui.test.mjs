import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, rm } from "node:fs/promises";
import path from "node:path";
import { tmpdir } from "node:os";
import {
  run,
  scenarioIds,
  assertWorkspaceEditorText,
  waitForWorkspaceEditorText,
} from "./windows-workspace-draft-ui.mjs";
test("missing owned native fixture cannot fabricate packaged UI PASS", async () => {
  const result = await run({
    sourceSha: "a".repeat(40),
    fixtureSha: "b".repeat(40),
    artifactDigests: { workspace: "c".repeat(64) },
  });
  assert.deepEqual(
    result.map((item) => item.id),
    scenarioIds,
  );
  assert.ok(result.every((item) => item.status === "NOT_RUN" && item.failureCode));
});
test("nested draft failure keeps its first error and screenshot despite cleanup failure", async (t) => {
  const temporaryRoot = await mkdtemp(path.join(tmpdir(), "devbox-draft-test-"));
  t.after(() => rm(temporaryRoot, { recursive: true, force: true }));
  const methods = [
    "prepare",
    "selectWindows",
    "crashAndReopen",
    "context",
    "recovery",
    "failRecoveryWriter",
    "reopenAfterClose",
    "observeInput",
  ];
  const fixture = Object.fromEntries(methods.map((name) => [name, async () => {}]));
  fixture.prepare = async () => {
    throw new Error("first draft failure");
  };
  fixture.cleanup = async () => {
    throw new Error("later cleanup failure");
  };
  const results = await run({
    fixtureRoot: temporaryRoot,
    workspaceFixture: fixture,
    ui: { screenshot: async () => "/owned/first.png" },
  });
  assert.equal(results[0].status, "FAIL");
  assert.equal(results[0].error.message, "first draft failure");
  assert.deepEqual(results[0].screenshotPaths, ["/owned/first.png"]);
  assert.equal(results[0].cleanupError.message, "later cleanup failure");
});

test("empty CodeMirror display line is exact despite AX presentation newline", async () => {
  const expressions = [];
  await assertWorkspaceEditorText(
    {
      evaluate: async (expression) => {
        expressions.push(expression);
        return [[""]];
      },
    },
    "",
  );
  assert.match(expressions[0], /cm-line/);
  assert.doesNotMatch(expressions[0], /trim|innerText/);
});
test("draft text observation retains whitespace and all trailing blank lines", async () => {
  await assertWorkspaceEditorText({ evaluate: async () => [["한글 ", ""]] }, "한글 \n");
  await assert.rejects(assertWorkspaceEditorText({ evaluate: async () => [[" "]] }, ""));
  await assert.rejects(assertWorkspaceEditorText({ evaluate: async () => [["", ""]] }, ""));
  await assert.rejects(assertWorkspaceEditorText({ evaluate: async () => [] }, ""));
});

test("async recovery waits exact live draft before final assertion without another input", async () => {
  let reads = 0;
  const cdp = { evaluate: async () => (++reads === 1 ? [] : [[""]]) };
  await waitForWorkspaceEditorText(
    cdp,
    async (check) => {
      await assert.rejects(check());
      assert.equal(await check(), true);
    },
    "",
  );
  assert.equal(reads, 3);
});

test("crash recovery returns to the owned project and Files before reading its journal dialog", async (t) => {
  const fixtureRoot = await mkdtemp(path.join(tmpdir(), "devbox-draft-resume-"));
  t.after(() => rm(fixtureRoot, { recursive: true, force: true }));
  let root,
    selected = true,
    route = "files";
  const boundary = "owned recovery dialog reached";
  let closeChoiceReady = false;
  const fixture = {
    prepare: async (value) => {
      root = value;
    },
    context: async () => null,
    recovery: async () => ({ entries: [{ path: path.join(root, "한글.txt"), content: "" }] }),
    wait: async (probe) => assert.equal(await probe(), true),
    observeInput: async () => {},
    crashAndReopen: async () => {
      selected = false;
      route = "overview";
    },
    selectWindows: async () => {
      selected = true;
    },
    waitForText: async (target) => {
      if (target.name !== "복구 (1)") return;
      assert.ok(selected && route === "files", "recovery read before restoring its owning view");
      throw new Error(boundary);
    },
    failRecoveryWriter: async () => {},
    reopenAfterClose: async () => {},
    cleanup: async () => {},
  };
  const records = await run({
    fixtureRoot,
    workspaceFixture: fixture,
    cdp: { evaluate: async () => [[""]] },
    ui: {
      click: async (target) => {
        if (target.name === "파일") route = "files";
        if (target.name === "종료 취소") assert.ok(closeChoiceReady, "native close review is still loading");
      },
      waitForTarget: async (target) => {
        if (target.name === "종료 취소") closeChoiceReady = true;
      },
      fill: async () => {},
      closeOwnedWindow: async () => {},
      screenshot: async () => "/owned/recovery.png",
    },
  });
  assert.equal(records[0].error.message, boundary);
});
