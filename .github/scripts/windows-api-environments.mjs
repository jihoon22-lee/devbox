import assert from "node:assert/strict";
import { requireApiContext, button, textbox, scenario, until, expectText } from "./windows-api-user-flow-actions.mjs";

import { observeEnvironmentIme } from "./windows-api-input-observations.mjs";

export const SCENARIO_IDS = Object.freeze(["ENV-01", "ENV-02"]);

export async function run(context) {
  requireApiContext(context);
  const results = [];
  results.push(
    await scenario(context, "ENV-01", async (record) => {
      await context.ui.click(button("요청"));
      const sealed = await context.nativeCall("plugin:api-studio|api", "seal_secret", {
        value: "synthetic-env-secret",
      });
      const seed = {
        version: 1,
        environments: [
          { id: "env-sparse", name: "Synthetic sparse", variables: [{ key: "var2", value: sealed, secret: true }] },
        ],
      };
      await context.seedDocument("environments", seed);
      await context.restart();
      record(
        "L4 fixture preparation seeds one sparse var2 with native DPAPI synthetic ciphertext into the owned document namespace",
      );
      await context.ui.waitForTarget(button("Synthetic sparse"));
      await context.ui.click(button("Synthetic sparse"));
      await context.ui.click(button("+ 변수"));
      await until(
        async () => (await context.document("environments")).value.environments[0].variables.length === 2,
        "Variable addition was not committed",
      );
      let stored = (await context.document("environments")).value;
      assert.deepEqual(
        stored.environments[0].variables.find((item) => item.key === "var2"),
        seed.environments[0].variables[0],
      );
      record("Actual Add creates a free name while retaining existing var2 sealed bytes and secret flag");
      await context.ui.fill(textbox("환경 변수 1 이름"), "var1");
      await context.ui.press("Tab");
      await expectText(context, "같은 이름의 변수가 있습니다.");
      assert.deepEqual((await context.document("environments")).value, stored);
      await observeEnvironmentIme(context);
      await context.ui.fill(textbox("환경 변수 1 이름"), "accessToken");
      await context.ui.press("Tab");
      await until(
        async () => (await context.document("environments")).value.environments[0].variables[0].key === "accessToken",
        "Rename was not saved",
      );
      stored = (await context.document("environments")).value;
      assert.equal(stored.environments[0].variables[0].value, sealed);
      record("Actual duplicate rename rejects persistence, then valid rename preserves the exact sealed blob");
      await context.ui.clickWithDialog(button("환경 변수 var1 삭제"), false);
      assert.deepEqual((await context.document("environments")).value, stored);
      await context.ui.clickWithDialog(button("환경 변수 var1 삭제"), true);
      await until(
        async () => (await context.document("environments")).value.environments[0].variables.length === 1,
        "Confirmed deletion was not saved",
      );
      record("Actual delete cancel preserves metadata; confirmed deletion removes only the named row");
      await context.restart();
      await context.ui.waitForTarget(button("Synthetic sparse"));
      await context.ui.click(button("Synthetic sparse"));
      assert.equal((await context.document("environments")).value.environments[0].variables[0].value, sealed);
      await expectText(context, "••••••••");
      record("Ordinary native window close/restart reopens the renamed secret in the same installed namespace");
    }),
  );
  if (results.at(-1).status !== "PASS") return results;
  results.push(
    await scenario(context, "ENV-02", async (record) => {
      await context.ui.fill(textbox("환경 이름"), "Synthetic sparse");
      const before = (await context.document("environments")).value;
      await context.ui.click(button("추가"));
      await until(
        async () =>
          (await context.document("environments")).value.environments.length === before.environments.length + 1,
        "Environment creation was not committed",
      );
      const stored = await context.document("environments");
      const created = stored.value.environments.find((item) => !before.environments.some((old) => old.id === item.id));
      assert.ok(created && created.name === "Synthetic sparse");
      const selected = await context.cdp.evaluate(
        "[...document.querySelectorAll('.env-item')].find(item=>item.querySelector('.env-name')?.getAttribute('aria-pressed')==='true')?.querySelector('.env-name').textContent",
      );
      assert.equal(selected, created.name);
      const rows = await context.cdp.evaluate("document.querySelectorAll('.env-var-row').length");
      assert.equal(rows, 0, "Created ID must select its empty environment instead of existing same-name variables");
      record(
        "Real creation selects the new empty environment ID even when its name duplicates the existing environment",
      );
      await context.seedDocument("environments", stored.value);
      record(
        "L4 failure fixture advances the native document revision without changing committed data or filesystem permissions",
      );
      await context.ui.click(button("+ 변수"));
      await expectText(context, "앱을 다시 열어 저장 상태를 확인하세요");
      assert.deepEqual((await context.document("environments")).value, stored.value);
      const locked = await context.cdp.evaluate("document.querySelector('[aria-label=\"새 변수 이름\"]').disabled");
      assert.equal(locked, true);
      await context.ui.click(button("프로토콜"));
      await expectText(context, "앱을 다시 열어 저장 상태를 확인하세요");
      await context.ui.click(button("기록 및 콘솔"));
      await expectText(context, "앱을 다시 열어 저장 상태를 확인하세요");
      record(
        "Real stale-revision save rejection retains committed native data, locks further writes and remains visible across Protocols/History",
      );
      await context.restart();
      await context.ui.click(button("요청"));
      await until(
        async () => await context.cdp.evaluate("!document.querySelector('[aria-label=\"환경 이름\"]').disabled"),
        "Restart did not recover native revision",
      );
      assert.deepEqual((await context.document("environments")).value, stored.value);
      record(
        "Ordinary restart reloads committed native revision and recovers editable controls without force-writing stale drafts",
      );
    }),
  );
  return results;
}
