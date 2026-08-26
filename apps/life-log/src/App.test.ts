import { cleanup, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { buildExportInput, DataSourceRow, monthRange, toDateStr, weekRange } from "./App";
import type { SourceStatus } from "./api";

afterEach(cleanup);

// 참고: 2024-01-01은 실제로 월요일이다(고정된 역사적 사실이므로 타임존과 무관하게 검증 가능).

describe("weekRange", () => {
  it("일요일이 주어지면 그 주(직전 월요일 ~ 다음 월요일 전)를 반환한다", () => {
    const sunday = new Date(2023, 11, 31); // 2023-12-31, Sunday
    expect(sunday.getDay()).toBe(0);

    const { start, end } = weekRange(sunday);
    const startDate = new Date(start);
    const endDate = new Date(end);

    expect([startDate.getFullYear(), startDate.getMonth(), startDate.getDate()]).toEqual([2023, 11, 25]); // Mon 2023-12-25
    expect([endDate.getFullYear(), endDate.getMonth(), endDate.getDate()]).toEqual([2024, 0, 1]); // 다음 Mon 2024-01-01
  });

  it("월요일이 주어지면 그 날 자체가 주의 시작이다", () => {
    const monday = new Date(2024, 0, 1); // 2024-01-01, Monday
    expect(monday.getDay()).toBe(1);

    const { start, end } = weekRange(monday);
    const startDate = new Date(start);
    const endDate = new Date(end);

    expect([startDate.getFullYear(), startDate.getMonth(), startDate.getDate()]).toEqual([2024, 0, 1]);
    expect([endDate.getFullYear(), endDate.getMonth(), endDate.getDate()]).toEqual([2024, 0, 8]);
  });

  it("주중 아무 요일이나 같은 주의 월요일로 정규화된다", () => {
    const wednesday = new Date(2024, 0, 3); // Wednesday
    const { start } = weekRange(wednesday);
    const startDate = new Date(start);
    expect([startDate.getFullYear(), startDate.getMonth(), startDate.getDate()]).toEqual([2024, 0, 1]);
  });

  it("월 경계를 넘는 주도 올바르게 계산한다", () => {
    const jan31 = new Date(2024, 0, 31); // Wednesday
    const { start, end } = weekRange(jan31);
    const startDate = new Date(start);
    const endDate = new Date(end);

    expect([startDate.getFullYear(), startDate.getMonth(), startDate.getDate()]).toEqual([2024, 0, 29]); // Mon Jan 29
    expect([endDate.getFullYear(), endDate.getMonth(), endDate.getDate()]).toEqual([2024, 1, 5]); // Mon Feb 5
  });

  it("항상 정확히 7일 구간이다", () => {
    const { start, end } = weekRange(new Date(2024, 5, 15));
    expect(end - start).toBe(7 * 86_400_000);
  });
});

describe("monthRange", () => {
  it("해당 월의 1일 00:00부터 다음 달 1일 00:00 전까지를 반환한다", () => {
    const { start, end } = monthRange(new Date(2024, 2, 15)); // March 2024
    const startDate = new Date(start);
    const endDate = new Date(end);

    expect([startDate.getFullYear(), startDate.getMonth(), startDate.getDate()]).toEqual([2024, 2, 1]);
    expect([endDate.getFullYear(), endDate.getMonth(), endDate.getDate()]).toEqual([2024, 3, 1]);
  });

  it("연말 경계(12월 -> 다음 해 1월)를 올바르게 넘어간다", () => {
    const { start, end } = monthRange(new Date(2023, 11, 20)); // December 2023
    const startDate = new Date(start);
    const endDate = new Date(end);

    expect([startDate.getFullYear(), startDate.getMonth(), startDate.getDate()]).toEqual([2023, 11, 1]);
    expect([endDate.getFullYear(), endDate.getMonth(), endDate.getDate()]).toEqual([2024, 0, 1]);
  });

  it("2월(윤년) 같은 짧은 달도 다음 달 1일까지만 포함한다", () => {
    const { start, end } = monthRange(new Date(2024, 1, 10)); // Feb 2024 (윤년, 29일)
    const startDate = new Date(start);
    const endDate = new Date(end);

    expect([startDate.getFullYear(), startDate.getMonth(), startDate.getDate()]).toEqual([2024, 1, 1]);
    expect([endDate.getFullYear(), endDate.getMonth(), endDate.getDate()]).toEqual([2024, 2, 1]);
  });
});

describe("toDateStr", () => {
  it("YYYY-MM-DD 형식으로, 월/일을 0으로 패딩한다", () => {
    expect(toDateStr(new Date(2024, 0, 5))).toBe("2024-01-05");
  });

  it("12월 31일처럼 패딩이 필요 없는 경우도 올바르다", () => {
    expect(toDateStr(new Date(2024, 11, 31))).toBe("2024-12-31");
  });
});

describe("buildExportInput", () => {
  it("날짜 범위를 local day boundary로 만들고 exclusive end를 사용한다", () => {
    const input = buildExportInput("2024-01-01", "2024-01-02", "json");
    expect(input).toMatchObject({
      startDate: "2024-01-01",
      endDate: "2024-01-02",
      format: "json",
    });
    expect(input?.timezone).toBeTruthy();
    const firstDayStart = new Date(2024, 0, 1).getTime();
    const secondDayStart = new Date(2024, 0, 2).getTime();
    const thirdDayStart = new Date(2024, 0, 3).getTime();
    expect(input?.dayBoundaries).toEqual([
      { date: "2024-01-01", startMs: firstDayStart, endMs: secondDayStart },
      { date: "2024-01-02", startMs: secondDayStart, endMs: thirdDayStart },
    ]);
    expect(input?.dayStart).toBe(firstDayStart);
    expect(input?.dayEnd).toBe(thirdDayStart);
  });

  it("잘못된 날짜와 366일 초과 범위를 거부한다", () => {
    expect(buildExportInput("2024-02-30", "2024-02-30", "csv")).toBeNull();
    expect(buildExportInput("2024-01-01", "2025-01-02", "markdown")).toBeNull();
  });
});

describe("DataSourceRow", () => {
  it("Knowledge activity 수와 최근 수정 및 freshness를 함께 표시한다", () => {
    const source: SourceStatus = {
      producer: "knowledge-base",
      available: true,
      schemaVersion: 1,
      producerVersion: "0.5.0",
      generatedAt: "2026-08-25T12:00:00Z",
      freshnessMs: 125_000,
      error: null,
      knowledgeActivity: {
        notesModifiedToday: 3,
        lastModifiedAtMs: 1_800_000_000_000,
        identifiedNotes: 3,
        identifiersComplete: true,
        legacySnapshot: false,
      },
    };

    render(createElement(DataSourceRow, { source }));

    expect(screen.getByText("knowledge-base")).toBeTruthy();
    expect(screen.getByText(/v1 · 0.5.0 · 2m 전 갱신/)).toBeTruthy();
    expect(screen.getByText(/오늘 작성·수정 3개/)).toBeTruthy();
    expect(screen.getByText(/마지막 수정/)).toBeTruthy();
  });

  it("잘못된 activity source에서도 version, freshness와 안전한 오류를 유지한다", () => {
    const source: SourceStatus = {
      producer: "knowledge-base",
      available: false,
      schemaVersion: 1,
      producerVersion: "0.5.0",
      generatedAt: "2026-08-25T12:00:00Z",
      freshnessMs: 61_000,
      error: "Knowledge activity view schema를 지원하지 않습니다",
      knowledgeActivity: null,
    };

    render(createElement(DataSourceRow, { source }));

    expect(screen.getByText(/v1 · 0.5.0 · 1m 전 갱신/)).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toBe(
      "Knowledge activity view schema를 지원하지 않습니다",
    );
  });
});
