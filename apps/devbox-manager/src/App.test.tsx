import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import App from "./App";

// isTauri()가 false인 환경(MOCK)에서 렌더링해 카탈로그·manifest 동작을 검증한다.

afterEach(() => cleanup());

describe("App", () => {
  beforeEach(() => {
    render(<App />);
  });

  it("managerVisible 앱 9개만 표시한다 (devbox-manager 제외)", async () => {
    await screen.findByText("Code Pad"); // 데이터 로드 대기
    const rows = screen.getAllByRole("row");
    // header + 9 rows
    expect(rows.length).toBe(10);
    // "Devbox Manager"는 헤더 타이틀 1건뿐 (목록 row에는 없음)
    expect(screen.getAllByText("Devbox Manager").length).toBe(1);
    expect(screen.getByText("Port Manager")).toBeTruthy();
  });

  it("앱별 서로 다른 최신 버전을 표시한다", async () => {
    await screen.findByText("Port Manager");
    // LATEST 컬럼: code-pad 0.3.0, port-manager 0.2.0
    expect(screen.getAllByText("0.3.0").length).toBeGreaterThan(0);
    expect(screen.getAllByText("0.2.0").length).toBeGreaterThan(0);
  });

  it("업데이트 판정이 앱별로 독립적이다", async () => {
    await screen.findByText("Port Manager");
    // port-manager는 0.2.0 설치 + 최신 0.2.0 → up to date
    expect(screen.getAllByText("up to date").length).toBe(1);
    // code-pad는 미설치 → Install 버튼이 보인다
    expect(screen.getAllByText("Install (portable)").length).toBeGreaterThan(0);
  });

  it("이전 버전이 있는 portable 앱에 rollback 버튼이 보인다", async () => {
    await screen.findByText("Port Manager");
    // port-manager: portable + previousVersion 0.1.0 → rollback 표시
    expect(screen.getAllByText("Rollback").length).toBe(1);
    expect(screen.getByText(/prev 0\.1\.0/)).toBeTruthy();
  });
});
