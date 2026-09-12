import { describe, expect, it } from "vitest";
import { isSnapshotActionable, isSnapshotExpired } from "./snapshotState";

const captured = 1_000_000;
const snapshot = { capturedAtMs: captured, staleAfterMs: 30_000 };

describe("isSnapshotExpired", () => {
  it("TTL 안이면 만료가 아니다", () => {
    expect(isSnapshotExpired(snapshot, captured)).toBe(false);
    expect(isSnapshotExpired(snapshot, captured + 30_000)).toBe(false);
  });

  it("TTL을 넘기면 만료다", () => {
    expect(isSnapshotExpired(snapshot, captured + 30_001)).toBe(true);
  });

  it("미래 시각·비정상 TTL·비유한 값은 만료로 본다", () => {
    expect(isSnapshotExpired(snapshot, captured - 1)).toBe(true);
    expect(isSnapshotExpired({ capturedAtMs: captured, staleAfterMs: 0 }, captured)).toBe(true);
    expect(isSnapshotExpired({ capturedAtMs: captured, staleAfterMs: -1 }, captured)).toBe(true);
    expect(isSnapshotExpired({ capturedAtMs: Number.NaN, staleAfterMs: 30_000 }, captured)).toBe(true);
    expect(isSnapshotExpired({ capturedAtMs: captured, staleAfterMs: Number.POSITIVE_INFINITY }, captured)).toBe(true);
  });
});

describe("isSnapshotActionable", () => {
  it("최신 snapshot에서 허용한다", () => {
    expect(isSnapshotActionable("fresh", true, false)).toBe(true);
  });

  it("refresh가 진행 중이어도 TTL 안이면 허용한다", () => {
    expect(isSnapshotActionable("refreshing", true, false)).toBe(true);
  });

  it("refresh가 TTL을 넘겨 오래 걸리면 거부한다", () => {
    expect(isSnapshotActionable("refreshing", true, true)).toBe(false);
  });

  it("수집 실패·만료·snapshot 부재에서는 거부한다", () => {
    expect(isSnapshotActionable("error", true, false)).toBe(false);
    expect(isSnapshotActionable("stale", true, false)).toBe(false);
    expect(isSnapshotActionable("loading", true, false)).toBe(false);
    expect(isSnapshotActionable("fresh", false, false)).toBe(false);
    expect(isSnapshotActionable("fresh", true, true)).toBe(false);
  });
});
