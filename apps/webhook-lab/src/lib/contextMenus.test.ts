import { describe, expect, it } from "vitest";
import { buildHistoryContextMenu, buildRuleContextMenu } from "./contextMenus";

function actions(items: ReturnType<typeof buildHistoryContextMenu>) {
  return items.filter((item) => item.type === "item");
}

describe("Webhook Lab context menu contracts", () => {
  it("history 메뉴의 정확한 항목과 danger 경계를 유지한다", () => {
    const items = actions(buildHistoryContextMenu(false));
    expect(items.map((item) => item.label)).toEqual([
      "마스킹 복사",
      "원본 복사",
      "헤더 복사",
      "API Playground로 변환",
      "삭제",
    ]);
    expect(items.find((item) => item.id === "delete")?.danger).toBe(true);
    expect(items.find((item) => item.id === "convert-api-playground")?.disabled).toBe(true);
  });

  it("rule 메뉴의 정확한 항목과 분리된 후속 기능을 유지한다", () => {
    const items = actions(buildRuleContextMenu(false));
    expect(items.map((item) => item.label)).toEqual([
      "편집",
      "복제",
      "예시 curl 복사",
      "삭제",
    ]);
    expect(items.find((item) => item.id === "delete")?.danger).toBe(true);
    expect(items.find((item) => item.id === "copy-example-curl")?.disabled).toBe(true);
  });

  it("진행 중에는 이미 구현된 변경 action만 비활성화한다", () => {
    expect(actions(buildHistoryContextMenu(true)).every((item) => item.disabled)).toBe(true);
    expect(actions(buildRuleContextMenu(true)).every((item) => item.disabled)).toBe(true);
  });
});
