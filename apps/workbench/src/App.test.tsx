import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import App from "./App";

// isTauri()가 false인 환경(MOCK, api.ts의 MOCK_PROFILES)에서 렌더링해 검증한다.

afterEach(() => cleanup());

describe("App", () => {
  beforeEach(() => {
    render(<App />);
  });

  it("mock 프로필을 사이드바에 표시하고 자동 선택한다", async () => {
    await screen.findByRole("button", { name: "devbox" });
    // 선택되면 프로필 이름이 상세 패널 제목으로도 나타난다.
    expect(screen.getAllByText("devbox").length).toBeGreaterThan(1);
  });

  it("선택된 프로필의 health 항목을 표시한다", async () => {
    // "Health" 제목은 selectedId가 정해지면 바로 렌더링되지만 항목은 projectHealth()의
    // 비동기 응답을 기다려야 한다 — 항목 텍스트 자체를 findByText로 기다린다.
    await screen.findByText("git");
    expect(screen.getByText("wsl")).toBeTruthy();
    expect(screen.getByText("ports")).toBeTruthy();
    expect(screen.getByText("services")).toBeTruthy();
  });

  it("Start Workspace 실행 후 Stop What I Started 버튼이 나타난다", async () => {
    const start = await screen.findByRole("button", { name: "Start Workspace" });
    start.click();
    await screen.findByRole("button", { name: "Stop What I Started" });
  });

  it("+ 프로필을 누르면 빈 프로필 편집 폼이 열린다", async () => {
    await screen.findByRole("button", { name: "devbox" });
    screen.getByRole("button", { name: "+ 프로필" }).click();
    await screen.findByText("새 프로필");
    const saveBtn = screen.getByRole("button", { name: "저장" });
    // 이름이 비어 있으면 저장 비활성 (App.tsx: disabled={busy || !editing.name.trim()}).
    expect(saveBtn).toBeDisabled();
  });
});
