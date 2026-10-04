import test from "node:test";
import assert from "node:assert/strict";
import { observeNormalClose } from "./windows-suite-ui-context.mjs";
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
