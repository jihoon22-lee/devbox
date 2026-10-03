import assert from "node:assert/strict";
import { scenarios } from "./windows-knowledge-flow-shared.mjs";
export const scenarioIds = ["ACTIVITY-01", "ACTIVITY-02", "ACTIVITY-03"];
export async function run(context) {
  return scenarios(context, [
    [
      "ACTIVITY-01",
      async ({ ui, fixture, assertions, screenshots }) => {
        await fixture.navigate("activity");
        await ui.click({ role: "button", name: "설정" });
        await ui.fill({ role: "spinbutton", name: "유휴 시간 (분)" }, "1");
        await ui.click({ role: "button", name: "유휴 시간 저장" });
        await fixture.wait(async () => (await fixture.idleThreshold()) === 60000, "native threshold acknowledged");
        await fixture.waitBody("적용된 유휴 시간: 1분");
        await ui.click({ role: "button", name: "타임라인" });
        const before = await fixture.selectedDate();
        await ui.click({ role: "button", name: "이전 날짜" });
        assert.notEqual(await fixture.selectedDate(), before);
        await ui.click({ role: "button", name: "다음 날짜" });
        assert.equal(await fixture.selectedDate(), before);
        screenshots.push(await ui.screenshot("ACTIVITY-01-date-ack"));
        assertions.push(
          "Real Settings save acknowledges native one-minute threshold; Timeline previous/next changes and restores the native query day",
        );
      },
    ],
    [
      "ACTIVITY-02",
      async ({ ui, fixture, assertions, screenshots }) => {
        await fixture.navigate("activity");
        await ui.click({ role: "button", name: "일" });
        await fixture.waitBody("일간 로컬 요약");
        await fixture.createDraftViaUi();
        const old = await fixture.history();
        assert.ok(old.length > 0);
        await fixture.navigate("activity");
        await ui.click({ role: "button", name: "설정" });
        await fixture.waitFor({ role: "button", name: "다시 생성" });
        await ui.click({ role: "button", name: "다시 생성" });
        await fixture.wait(async () => {
          const rows = await fixture.history();
          return rows.some((r) => r.regeneratedFrom === old[0].handoffId && r.handoffId !== old[0].handoffId);
        }, "historical draft new linked ID");
        const rows = await fixture.history(),
          fresh = rows.find((r) => r.regeneratedFrom === old[0].handoffId);
        for (const key of ["period", "startDate", "endDate", "timezone", "filter"])
          assert.deepEqual(fresh.summary[key], old[0].summary[key]);
        assert.ok(rows.some((r) => r.handoffId === old[0].handoffId));
        await fixture.waitBody("Knowledge 초안");
        screenshots.push(await ui.screenshot("ACTIVITY-02-history-regenerate"));
        assertions.push(
          "Settings regeneration uses preserved historical range/timezone/filter and creates a new linked draft while previous history survives; delivery awaits note review",
        );
      },
    ],
    [
      "ACTIVITY-03",
      async ({ ui, fixture, assertions, screenshots }) => {
        await fixture.navigate("activity");
        assert.equal((await fixture.collectionStatus()).consent, false);
        await ui.click({ role: "button", name: "타임라인" });
        await ui.click({ role: "button", name: "추적 시작" });
        const capture = await fixture.captureOwnedIdleBoundary();
        assert.ok(capture.sessions.length > 0);
        assert.ok(
          capture.sessions.every((row) => row.end_ts <= capture.lastInputMs + 2500 && row.end_ts >= row.start_ts),
        );
        await fixture.navigate("activity");
        await ui.click({ role: "button", name: "타임라인" });
        await ui.click({ role: "button", name: "추적 중지" });
        assert.equal((await fixture.collectionStatus()).tracking, false);
        await ui.click({ role: "button", name: "설정" });
        await ui.fill({ role: "textbox", name: "제목 치환 정규식" }, capture.marker);
        await ui.click({ role: "button", name: "규칙 저장" });
        await fixture.waitBody("규칙을 저장했습니다");
        assert.ok((await fixture.privacy()).rules.redactTitlePatterns.includes(capture.marker));
        const owner = await fixture.lifecycle();
        assert.equal(owner.owner, "installedAgent");
        assert.equal(owner.consent, false);
        screenshots.push(await ui.screenshot("ACTIVITY-03-consent-idle-privacy"));
        assertions.push(
          "Owned HWND receives actual input before an idle interval; confirmed native sessions exclude all idle tail and remain nonnegative; pause and privacy persist separately from UI visibility",
        );
      },
    ],
  ]);
}
