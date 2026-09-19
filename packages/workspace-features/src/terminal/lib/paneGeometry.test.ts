import { describe, expect, it } from "vitest";
import { nextPaneIndex, paneColumns } from "./paneGeometry";

describe("paneColumns", () => {
  it("레이아웃별 열 수는 PaneCanvas의 grid 계산과 같다", () => {
    expect(paneColumns("cols", 3)).toBe(3);
    expect(paneColumns("rows", 3)).toBe(1);
    expect(paneColumns("grid", 4)).toBe(2);
    expect(paneColumns("grid", 5)).toBe(3);
    expect(paneColumns("grid", 0)).toBe(1);
  });
});

describe("nextPaneIndex", () => {
  it("2×2 격자에서 방향마다 실제 이웃으로 이동한다", () => {
    expect(nextPaneIndex("grid", 4, 0, "right")).toBe(1);
    expect(nextPaneIndex("grid", 4, 0, "down")).toBe(2);
    expect(nextPaneIndex("grid", 4, 3, "left")).toBe(2);
    expect(nextPaneIndex("grid", 4, 3, "up")).toBe(1);
  });

  it("격자 가장자리에서는 순환하지 않는다", () => {
    expect(nextPaneIndex("grid", 4, 1, "right")).toBeNull();
    expect(nextPaneIndex("grid", 4, 0, "left")).toBeNull();
    expect(nextPaneIndex("grid", 4, 0, "up")).toBeNull();
    expect(nextPaneIndex("grid", 4, 2, "down")).toBeNull();
  });

  it("마지막 줄이 비어 있는 칸으로는 내려가지 않는다", () => {
    // 3개 팬은 2열 × 2행이고 오른쪽 아래 칸이 비어 있다.
    expect(nextPaneIndex("grid", 3, 1, "down")).toBeNull();
    expect(nextPaneIndex("grid", 3, 0, "down")).toBe(2);
  });

  it("세로 분할은 좌우로만, 가로 분할은 위아래로만 움직인다", () => {
    expect(nextPaneIndex("cols", 3, 0, "right")).toBe(1);
    expect(nextPaneIndex("cols", 3, 0, "down")).toBeNull();
    expect(nextPaneIndex("rows", 3, 0, "down")).toBe(1);
    expect(nextPaneIndex("rows", 3, 0, "right")).toBeNull();
  });

  it("범위를 벗어난 입력은 이동하지 않는다", () => {
    expect(nextPaneIndex("grid", 0, 0, "right")).toBeNull();
    expect(nextPaneIndex("grid", 4, -1, "right")).toBeNull();
    expect(nextPaneIndex("grid", 4, 4, "right")).toBeNull();
  });
});
