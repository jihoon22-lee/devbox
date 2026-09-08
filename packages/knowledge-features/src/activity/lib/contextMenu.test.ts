import { describe, expect, it } from "vitest";
import { buildDateContextMenu, parseDateKey } from "./contextMenu";

describe("Life Log date context menu contract", () => {
  it("정확한 날짜 메뉴와 #305 export 경계를 유지한다", () => {
    const items = buildDateContextMenu(false);
    expect(items.map((item) => item.type === "separator" ? "separator" : item.label)).toEqual([
      "날짜 복사",
      "Markdown 내보내기",
      "JSON 내보내기",
      "CSV 내보내기",
    ]);
    expect(items[0]).toMatchObject({ id: "copy-date", disabled: false });
    expect(items[1]).toMatchObject({ id: "export-markdown", disabled: false });
    expect(items[2]).toMatchObject({ id: "export-json", disabled: false });
    expect(items[3]).toMatchObject({ id: "export-csv", disabled: false });
  });

  it("복사 action은 busy 동안만 비활성화한다", () => {
    expect(buildDateContextMenu(true)[0]).toMatchObject({ id: "copy-date", disabled: true });
  });

  it("실제 존재하는 local calendar date만 파싱한다", () => {
    const leapDay = parseDateKey("2024-02-29");
    expect(leapDay && [leapDay.getFullYear(), leapDay.getMonth(), leapDay.getDate()]).toEqual([
      2024,
      1,
      29,
    ]);
    expect(parseDateKey("2023-02-29")).toBeNull();
    expect(parseDateKey("2024-13-01")).toBeNull();
    expect(parseDateKey("2024-1-01")).toBeNull();
    expect(parseDateKey("0000-01-01")).toBeNull();
    expect(parseDateKey("credential-raw")).toBeNull();
  });
});
