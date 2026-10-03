import assert from "node:assert/strict";
import {
  requireApiContext,
  button,
  textbox,
  select,
  scenario,
  until,
  expectText,
  echoFixture,
} from "./windows-api-user-flow-actions.mjs";

import { observePipelineKeyboardModal, finishApiInputObservation } from "./windows-api-input-observations.mjs";

const byId = (items) => [...items].sort((left, right) => left.id.localeCompare(right.id));
const settled = (context) =>
  until(
    async () =>
      await context.cdp.evaluate(
        "Boolean(Array.from(document.querySelectorAll('.smart-workflow button')).find(item=>item.textContent.startsWith('현재 도구 즐겨찾기')&&!item.disabled))&&!Array.from(document.querySelectorAll('.smart-workflow-pipeline p[role=status]')).some(item=>item.textContent==='저장 중…')",
      ),
    "Workflow persistence did not settle",
  );
export const SCENARIO_IDS = Object.freeze(["TRANSFORM-01"]);

export async function run(context) {
  requireApiContext(context);
  const echo = await echoFixture();
  try {
    return [
      await scenario(context, "TRANSFORM-01", async (record) => {
        await context.ui.click(button("요청"));
        await context.ui.fill(textbox("요청 URL"), `${echo.url}/handoff-no-auto-send`);
        await context.ui.click(button("변환"));
        await settled(context);
        const pipelines = Array.from({ length: 20 }, (_, index) => ({
          id: `pipeline-${index + 1}`,
          inputType: "json",
          steps: [{ transformerId: "json-format" }],
          updatedAt: index + 1,
        }));
        await context.seedDocument("workflows", { schemaVersion: 1, recentTools: [], favoriteTools: [], pipelines });
        await context.restart();
        await context.ui.click(button("변환"));
        await settled(context);
        record("L4 fixture preparation seeds exactly 20 metadata-only pipelines in the owned native document");
        await context.ui.click(button("pipeline-7: JSON 포매터"));
        const index = await context.cdp.evaluate(
          "Array.from(document.querySelector('[aria-label=\"변환 단계 추가\"]').options).findIndex(option=>option.value==='json-format')",
        );
        assert.ok(index >= 0);
        await select(context, "변환 단계 추가", index);
        await context.ui.click(button("단계 추가"));
        await context.ui.click(button("파이프라인 저장"));
        await until(
          async () =>
            (await context.document("workflows")).value.pipelines.find((item) => item.id === "pipeline-7").steps
              .length === 2,
          "Edited pipeline not committed",
        );
        let stored = (await context.document("workflows")).value;
        assert.equal(stored.pipelines.length, 20);
        assert.deepEqual(
          byId(stored.pipelines.filter((item) => item.id !== "pipeline-7")),
          byId(pipelines.filter((item) => item.id !== "pipeline-7")),
        );
        await expectText(context, "저장 완료");
        record(
          "Actual edit at capacity retains pipeline-7 identity and all other 19 pipelines; confirmed native commit precedes saved status",
        );
        await context.restart();
        await context.ui.click(button("변환"));
        await settled(context);
        await context.ui.click(button("pipeline-7: JSON 포매터 → JSON 포매터"));
        await context.ui.click(button("단계 1 JSON 포매터 제거"));
        const focused = await context.cdp.evaluate("document.activeElement?.getAttribute('aria-label')");
        assert.equal(focused, "단계 1 JSON 포매터 제거");
        await context.ui.click(button("pipeline-7 파이프라인 삭제"));
        await context.ui.confirmDialog(false);
        assert.equal((await context.document("workflows")).value.pipelines.length, 20);
        assert.deepEqual((await context.document("workflows")).value.pipelines, stored.pipelines);
        await context.ui.click(button("pipeline-7 파이프라인 삭제"));
        await context.ui.confirmDialog(true);
        await until(
          async () => (await context.document("workflows")).value.pipelines.length === 19,
          "Confirmed deletion not committed",
        );
        await context.ui.click(button("새 파이프라인"));
        await select(context, "파이프라인 입력 형식", 1);
        const nextIndex = await context.cdp.evaluate(
          "Array.from(document.querySelector('[aria-label=\"변환 단계 추가\"]').options).findIndex(option=>option.value==='json-format')",
        );
        await select(context, "변환 단계 추가", nextIndex);
        await context.ui.click(button("단계 추가"));
        await context.ui.fill(textbox("스마트 워크플로 입력"), '{"synthetic-private-input":true}');
        await context.ui.click(button("파이프라인 저장"));
        await until(
          async () => (await context.document("workflows")).value.pipelines.length === 20,
          "New pipeline did not reuse freed capacity",
        );
        stored = (await context.document("workflows")).value;
        assert.ok(!JSON.stringify(stored).includes("synthetic-private-input"));
        record(
          "Real restart restores edited steps; stage deletion preserves keyboard focus; delete cancel/confirm and explicit new save recover capacity without persisting input",
        );
        await context.ui.click(button("파이프라인 실행"));
        await until(
          async () =>
            await context.cdp.evaluate(
              "document.querySelector('[aria-label=\"파이프라인 결과\"]')?.textContent.includes('synthetic-private-input')",
            ),
          "Successful pipeline output missing",
        );
        await observePipelineKeyboardModal(context);
        await expectText(context, "Requests 요청 미리보기");
        await context.ui.click(button("취소"));
        assert.equal(await context.cdp.evaluate("document.querySelectorAll('.api-handoff-dialog').length"), 0);
        await context.ui.click(button("Requests로 보내기", { role: "region", name: "타입 지정 파이프라인" }));
        await context.ui.click(button("API Playground로 전달"));
        await context.ui.click(button("요청"));
        await expectText(context, "Toolbox 텍스트 요청 미리보기");
        const priorUrl = await context.cdp.evaluate("document.querySelector('[aria-label=\"요청 URL\"]').value");
        await context.ui.click(button("취소"));
        assert.equal(await context.cdp.evaluate("document.querySelector('[aria-label=\"요청 URL\"]').value"), priorUrl);
        assert.equal(echo.hits.length, 0);
        await context.ui.click(button("변환"));
        await context.ui.click(button("Requests로 보내기", { role: "region", name: "타입 지정 파이프라인" }));
        await context.ui.click(button("API Playground로 전달"));
        await context.ui.click(button("요청"));
        await expectText(context, "Toolbox 텍스트 요청 미리보기");
        await context.ui.click(button("적용"));
        await context.ui.click(button("BODY"));
        assert.ok(
          await context.cdp.evaluate(
            "document.querySelector('.body-input')?.value.includes('synthetic-private-input')",
          ),
        );
        assert.equal(echo.hits.length, 0);
        record(
          "Actual successful output requires sender and recipient previews; both cancel paths preserve draft, explicit recipient apply inserts body, and the owned echo receives zero automatic requests",
        );
        await context.ui.click(button("변환"));
        await context.ui.fill(textbox("스마트 워크플로 입력"), "invalid-json");
        await context.ui.click(button("파이프라인 실행"));
        assert.equal(
          await context.cdp.evaluate("document.querySelector('.smart-workflow-pipeline .api-handoff-button')!==null"),
          false,
        );
        record("Actual failed pipeline removes output handoff action rather than exporting stale successful output");
        await settled(context);
        const committed = await context.document("workflows");
        await context.seedDocument("workflows", committed.value);
        record(
          "L4 failure fixture advances only owned native document revision, retaining the last committed metadata",
        );
        await context.ui.click(button("단계 추가"));
        await context.ui.click(button("파이프라인 저장"));
        await expectText(context, "저장 실패 — 앱을 다시 열어 저장 상태를 확인하세요");
        assert.deepEqual((await context.document("workflows")).value, committed.value);
        await context.restart();
        await context.ui.click(button("변환"));
        assert.deepEqual((await context.document("workflows")).value.pipelines, committed.value.pipelines);
        record(
          "Actual stale-revision save failure preserves the last committed metadata through native window close/restart",
        );
        await finishApiInputObservation(context);
      }),
    ];
  } finally {
    await echo.close();
  }
}
