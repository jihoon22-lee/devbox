import { describe, expect, it } from "vitest";
import {
  MIN_PANE_FRACTION,
  evenFractions,
  normalizeFractions,
  resizeAdjacent,
  toGridTemplate,
} from "./paneSizing";

describe("normalizeFractions", () => {
  it("팬 수가 맞고 합이 1이면 그대로 쓴다", () => {
    expect(normalizeFractions([0.25, 0.75], 2)).toEqual([0.25, 0.75]);
  });

  it("합이 1이 아니면 비율을 유지한 채 정규화한다", () => {
    expect(normalizeFractions([1, 3], 2)).toEqual([0.25, 0.75]);
  });

  it("길이가 다르거나 없으면 균등 분할로 되돌린다", () => {
    expect(normalizeFractions([0.5, 0.5], 3)).toEqual(evenFractions(3));
    expect(normalizeFractions(undefined, 2)).toEqual([0.5, 0.5]);
  });

  it("손상된 값이나 최소 비율 미만이면 균등 분할로 되돌린다", () => {
    expect(normalizeFractions([Number.NaN, 1], 2)).toEqual([0.5, 0.5]);
    expect(normalizeFractions([0.001, 0.999], 2)).toEqual([0.5, 0.5]);
    expect(normalizeFractions([-1, 2], 2)).toEqual([0.5, 0.5]);
  });

  it("팬이 없으면 빈 목록이다", () => {
    expect(normalizeFractions([1], 0)).toEqual([]);
  });
});

describe("resizeAdjacent", () => {
  it("이웃한 두 팬 사이에서만 크기를 옮기고 합을 보존한다", () => {
    const next = resizeAdjacent([0.5, 0.5], 0, 0.2);
    expect(next[0]).toBeCloseTo(0.7);
    expect(next[1]).toBeCloseTo(0.3);
    expect(next[0] + next[1]).toBeCloseTo(1);
  });

  it("다른 팬은 건드리지 않는다", () => {
    const next = resizeAdjacent([0.4, 0.3, 0.3], 1, 0.1);
    expect(next[0]).toBeCloseTo(0.4);
    expect(next[1] + next[2]).toBeCloseTo(0.6);
  });

  it("최소 비율 아래로는 줄이지 않는다", () => {
    const next = resizeAdjacent([0.5, 0.5], 0, -10);
    expect(next[0]).toBeGreaterThanOrEqual(MIN_PANE_FRACTION - 1e-9);
    expect(next[0] + next[1]).toBeCloseTo(1);
  });

  it("범위 밖 구분선은 아무것도 바꾸지 않는다", () => {
    expect(resizeAdjacent([0.5, 0.5], 1, 0.2)).toEqual([0.5, 0.5]);
    expect(resizeAdjacent([0.5, 0.5], -1, 0.2)).toEqual([0.5, 0.5]);
  });
});

describe("toGridTemplate", () => {
  it("비율을 fr 트랙으로 옮긴다", () => {
    expect(toGridTemplate([0.25, 0.75])).toBe("250fr 750fr");
    expect(toGridTemplate([])).toBe("1fr");
  });
});
