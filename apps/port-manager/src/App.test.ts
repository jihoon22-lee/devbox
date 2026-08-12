import { describe, expect, it } from "vitest";
import { matches } from "./App";
import type { PortRow } from "./types";

function row(overrides: Partial<PortRow> = {}): PortRow {
  return {
    proto: "tcp",
    local_addr: "127.0.0.1",
    port: 3000,
    state: "LISTENING",
    pid: 1234,
    process_name: "node.exe",
    ...overrides,
  };
}

describe("matches", () => {
  it("빈 쿼리(또는 공백만)는 항상 true", () => {
    expect(matches(row(), "")).toBe(true);
    expect(matches(row(), "   ")).toBe(true);
  });

  it("proto/state/port/local_addr/process_name 각 필드로 대소문자 구분 없이 매치한다", () => {
    expect(matches(row(), "TCP")).toBe(true);
    expect(matches(row(), "listening")).toBe(true);
    expect(matches(row(), "3000")).toBe(true);
    expect(matches(row(), "127.0.0")).toBe(true);
    expect(matches(row(), "NODE")).toBe(true);
  });

  it("pid로도 매치한다", () => {
    expect(matches(row({ pid: 4321 }), "4321")).toBe(true);
  });

  it("pid가 null이면 pid 매치 조건에서 예외 없이 건너뛴다", () => {
    expect(matches(row({ pid: null }), "1234")).toBe(false);
    // 다른 필드는 여전히 매치 가능
    expect(matches(row({ pid: null }), "tcp")).toBe(true);
  });

  it("process_name이 null이어도 예외 없이 처리한다 (옵셔널 체이닝)", () => {
    expect(matches(row({ process_name: null }), "node")).toBe(false);
    expect(matches(row({ process_name: null }), "tcp")).toBe(true);
  });

  it("어느 필드에도 없는 문자열은 false", () => {
    expect(matches(row(), "no-such-thing")).toBe(false);
  });
});
