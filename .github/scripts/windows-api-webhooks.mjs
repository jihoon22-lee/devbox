import assert from "node:assert/strict";
import path from "node:path";
import { readFile, writeFile, realpath, lstat } from "node:fs/promises";
import { freePort } from "./workspace-cdp-fixture.mjs";
import {
  requireApiContext,
  button,
  textbox,
  scenario,
  until,
  expectText,
  bodyText,
} from "./windows-api-user-flow-actions.mjs";

export const SCENARIO_IDS = Object.freeze(["WEB-01", "WEB-02"]);

export async function run(context) {
  requireApiContext(context);
  const results = [];
  const port = await freePort();
  let started = false;
  const post = async (name) =>
    fetch(`http://127.0.0.1:${port}/${name}`, {
      method: "POST",
      body: "synthetic webhook payload",
      signal: AbortSignal.timeout(5000),
    });
  try {
    results.push(
      await scenario(context, "WEB-01", async (record) => {
        await context.ui.click(button("웹훅 및 모의 서버"));
        await context.ui.fill({ role: "spinbutton", name: "포트" }, String(port));
        await context.ui.click(button("시작"));
        await expectText(context, `127.0.0.1:${port}`);
        started = true;
        await context.ui.fill(textbox("path"), "/synthetic-unsaved-rule");
        const sent = Date.now();
        await post("r15-first");
        await expectText(context, "/r15-first");
        assert.ok(Date.now() - sent < 5000, "Visible webhook did not refresh within polling budget");
        await context.ui.click({ role: "generic", name: "POST /r15-first 요청" });
        await context.ui.click(button("요청"));
        await post("r15-hidden");
        await context.ui.click(button("웹훅 및 모의 서버"));
        await expectText(context, "/r15-hidden");
        assert.equal(
          await context.cdp.evaluate("document.querySelector('#rule-path').value"),
          "/synthetic-unsaved-rule",
        );
        assert.equal(
          await context.cdp.evaluate("document.querySelector('.request-row.selected')?.getAttribute('aria-label')"),
          "POST /r15-first 요청",
        );
        const status = await context.nativeCall("plugin:api-studio|webhooks", "server_status", {}, "webhooks");
        assert.equal(status.running, true);
        record(
          "Actual UI starts its owned listener; external POST appears within the interval without a mutation refresh",
        );
        record(
          "Requests navigation leaves native listener running; reentry reads second external arrival while preserving selected history and unsaved rule draft",
        );
      }),
    );
    if (results.at(-1).status !== "PASS") return results;
    results.push(
      await scenario(context, "WEB-02", async (record) => {
        const history = await context.nativeCall("plugin:api-studio|webhooks", "list_history", {}, "webhooks");
        await context.nativeCall(
          "plugin:api-studio|webhooks",
          "save_fixture",
          { historyId: history[0].id },
          "webhooks",
        );
        await context.ui.click(button("새로 고침"));
        await expectText(context, "저장된 fixture (1)");
        record("L4 preparation captures one synthetic native history record into the owned masked fixture store");
        const file = path.join(context.namespace, "webhooks", "fixtures.json");
        assert.ok(!(await lstat(file)).isSymbolicLink());
        assert.equal((await realpath(file)).toLowerCase(), path.resolve(file).toLowerCase());
        const original = await readFile(file);
        try {
          await writeFile(file, '{"schemaVersion":999,"fixtures":[]}');
          record(
            "L4 read-failure fixture temporarily supplies unsupported schema in only the verified disposable fixtures file",
          );
          await context.ui.click(button("새로 고침"));
          await expectText(context, "목록을 갱신하지 못했습니다");
          await expectText(context, "저장된 fixture (1)");
          await expectText(context, "/r15-first");
          record(
            "Actual failed read preserves the last successful fixture and history lists and exposes a fixed refresh error",
          );
        } finally {
          await writeFile(file, original);
        }
        await context.ui.click(button("새로 고침"));
        await until(
          async () => !(await bodyText(context)).includes("목록을 갱신하지 못했습니다"),
          "Manual refresh did not clear recovered read error",
        );
        assert.equal(
          await context.cdp.evaluate("document.querySelector('#rule-path').value"),
          "/synthetic-unsaved-rule",
        );
        record(
          "Actual manual refresh recovers after restored fixture bytes without restarting listener or losing draft",
        );
      }),
    );
    return results;
  } finally {
    if (started) {
      await context.ui.click(button("웹훅 및 모의 서버"));
      await context.ui.click(button("중지"));
      const status = await context.nativeCall("plugin:api-studio|webhooks", "server_status", {}, "webhooks");
      assert.equal(status.running, false, "Owned webhook listener did not stop");
    }
  }
}
