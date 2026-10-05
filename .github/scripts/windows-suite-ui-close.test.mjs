import test from "node:test";
import assert from "node:assert/strict";
import { observeNormalClose, requestNormalClose } from "./windows-suite-ui-context.mjs";
test("native close requests once, confirms review, and waits for child exit before cleanup", async () => {
  const child = { exitCode: null };
  const events = [];
  await requestNormalClose(
    {
      child,
      product: "workspace",
      cdp: { evaluate: async () => events.includes("request") },
      ui: {
        click: async (target) => {
          assert.equal(target.name, "종료");
          events.push("confirm");
        },
      },
    },
    async () => events.push("request"),
    async (check) => {
      assert.equal(await check(), false);
      assert.deepEqual(events, ["request", "confirm"]);
      assert.equal(await check(), false);
      child.exitCode = 0;
      assert.equal(await check(), true);
      events.push("exit");
    },
  );
  events.push("cleanup");
  assert.deepEqual(events, ["request", "confirm", "exit", "cleanup"]);
});
test("reviewed close cannot continue to cleanup while the child remains alive", async () => {
  let cleanup = false;
  await assert.rejects(async () => {
    await requestNormalClose(
      {
        child: { exitCode: null },
        product: "workspace",
        cdp: { evaluate: async () => true },
        ui: { click: async () => {} },
      },
      async () => {},
      async (check) => {
        assert.equal(await check(), false);
        throw new Error("owned close deadline");
      },
    );
    cleanup = true;
  }, /owned close deadline/);
  assert.equal(cleanup, false);
});
test("owned native close waits child exit after renderer disconnect without another renderer call", async () => {
  const child = { exitCode: null };
  let calls = 0;
  await observeNormalClose(
    {
      child,
      product: "workspace",
      cdp: {
        evaluate: async () => {
          calls++;
          throw new Error("CDP disconnected");
        },
      },
      ui: {},
    },
    async (check) => {
      assert.equal(await check(), false);
      child.exitCode = 0;
      assert.equal(await check(), true);
    },
  );
  assert.equal(calls, 1);
});
test("renderer disconnect is not proof that the owned process exited", async () => {
  const child = { exitCode: null };
  await assert.rejects(
    observeNormalClose(
      {
        child,
        product: "workspace",
        cdp: {
          evaluate: async () => {
            throw new Error("CDP disconnected");
          },
        },
        ui: {},
      },
      async (check) => {
        assert.equal(await check(), false);
        assert.equal(await check(), false);
        throw new Error("owned close deadline");
      },
    ),
    /owned close deadline/,
  );
});
test("Workspace close still requires the explicit owned quit review", async () => {
  const child = { exitCode: null };
  let accepted = 0;
  await observeNormalClose(
    {
      child,
      product: "workspace",
      cdp: { evaluate: async () => true },
      ui: {
        click: async (target) => {
          assert.deepEqual(target, {
            role: "button",
            name: "종료",
            scope: { role: "dialog", name: "Workspace 종료 검토" },
          });
          accepted++;
          child.exitCode = 0;
        },
      },
    },
    async (check) => {
      assert.equal(await check(), true);
    },
  );
  assert.equal(accepted, 1);
});

test("explicit owned handoff close review still waits for native process exit", async () => {
  const child = { exitCode: null };
  let reviews = 0;
  await observeNormalClose(
    {
      child,
      product: "workspace",
      cdp: { evaluate: async () => true },
      ui: {
        click: async () => {
          throw new Error("Unscoped clean-close button used");
        },
      },
      reviewWorkspaceClose: async () => {
        reviews++;
      },
    },
    async (check) => {
      assert.equal(await check(), false);
      assert.equal(reviews, 1);
      assert.equal(await check(), false);
      assert.equal(reviews, 1);
      child.exitCode = 0;
      assert.equal(await check(), true);
    },
  );
});
