import { describe, expect, it } from "vitest";
import { buildPaneContextMenu, buildTabContextMenu, normalizeTabName } from "./contextMenu";

function labels(items: ReturnType<typeof buildPaneContextMenu>) {
  return items.map((item) => item.type === "separator" ? "separator" : item.label);
}

describe("WSL Desktop context menu contracts", () => {
  it("탭 이름을 단일 줄·80자로 정규화하고 빈 값을 거부한다", () => {
    expect(normalizeTabName("  build\r\nlogs  ")).toBe("build logs");
    expect(normalizeTabName("x".repeat(81))).toBe("x".repeat(80));
    expect(normalizeTabName(" \r\n ")).toBeNull();
  });

  it("pane 메뉴의 정확한 항목과 #262 비범위를 유지한다", () => {
    const items = buildPaneContextMenu({ busy: false, hasSelection: true, hasCwd: true });
    expect(labels(items)).toEqual([
      "복사",
      "붙여넣기",
      "검색",
      "세로 분할",
      "가로 분할",
      "cwd 복사",
      "팬 닫기",
    ]);
    for (const id of ["copy", "paste", "search", "copy-cwd"]) {
      const item = items.find((candidate) => candidate.type === "item" && candidate.id === id);
      expect(item?.type === "item" && item.disabled).toBe(false);
    }
    const close = items.find((item) => item.type === "item" && item.id === "close");
    expect(close?.type === "item" && close.danger).toBe(true);
  });

  it("pane action은 busy 동안 모두 비활성화된다", () => {
    const items = buildPaneContextMenu({ busy: true, hasSelection: true, hasCwd: true });
    for (const id of ["copy", "paste", "search", "split-vertical", "split-horizontal", "copy-cwd", "close"]) {
      const item = items.find((candidate) => candidate.type === "item" && candidate.id === id);
      expect(item?.type === "item" && item.disabled).toBe(true);
    }
  });

  it("selection과 OSC 7 cwd가 없는 exact pane의 해당 action만 비활성화한다", () => {
    const items = buildPaneContextMenu({ busy: false, hasSelection: false, hasCwd: false });
    const disabled = (id: string) => {
      const item = items.find((candidate) => candidate.type === "item" && candidate.id === id);
      return item?.type === "item" && item.disabled;
    };
    expect(disabled("copy")).toBe(true);
    expect(disabled("copy-cwd")).toBe(true);
    expect(disabled("paste")).toBe(false);
    expect(disabled("search")).toBe(false);
  });

  it("tab 메뉴와 layout submenu의 정확한 topology를 유지한다", () => {
    const items = buildTabContextMenu(false, true);
    expect(labels(items)).toEqual(["닫기", "다른 탭 닫기", "이름 변경", "레이아웃 전환"]);
    const layout = items.find((item) => item.type === "submenu" && item.id === "layout");
    expect(layout?.type).toBe("submenu");
    expect(layout?.type === "submenu" ? labels(layout.items) : []).toEqual([
      "격자",
      "세로 분할",
      "가로 분할",
    ]);
    for (const id of ["close", "close-others"]) {
      const item = items.find((candidate) => candidate.type === "item" && candidate.id === id);
      expect(item?.type === "item" && item.danger).toBe(true);
    }
  });

  it("다른 탭이 없으면 해당 close action만 비활성화한다", () => {
    const items = buildTabContextMenu(false, false);
    const close = items.find((item) => item.type === "item" && item.id === "close");
    const closeOthers = items.find((item) => item.type === "item" && item.id === "close-others");
    expect(close?.type === "item" && close.disabled).toBe(false);
    expect(closeOthers?.type === "item" && closeOthers.disabled).toBe(true);
  });
});
