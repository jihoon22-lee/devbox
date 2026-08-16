import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import App from "./App";

// isTauri()가 false인 환경(MOCK, api.ts의 MOCK_HISTORY)에서 렌더링해 검증한다.

afterEach(() => cleanup());

describe("App", () => {
  beforeEach(() => {
    render(<App />);
  });

  it("mock history를 표시한다", async () => {
    await screen.findByText("History (2)");
    expect(screen.getByText("/hook")).toBeTruthy();
    expect(screen.getByText("/health")).toBeTruthy();
  });

  it("민감 헤더 마스킹 배지는 마스킹된 값이 있는 요청에만 보인다", async () => {
    await screen.findByText("History (2)");
    // MOCK_HISTORY 어느 항목도 "•••••" 값을 갖지 않는다 (실제 마스킹은 Rust
    // history::mask_header가 수행) — mock 데이터에서는 배지가 보이면 안 된다.
    expect(screen.queryByText("민감 헤더 마스킹됨")).toBeNull();
  });

  it("서버가 중지 상태면 포트 입력과 시작 버튼을 보여준다", async () => {
    await screen.findByText(/중지/, { selector: ".status" });
    expect(screen.getByText("시작")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "중지" })).toBeNull();
  });

  it("LAN 공개를 켜면 경고 문구가 보인다", async () => {
    await screen.findByText(/중지/, { selector: ".status" });
    screen.getByRole("checkbox").click();
    await screen.findByText(/LAN 공개는 명시적 설정입니다/);
  });

  it("규칙이 없으면 안내 문구를 보여준다", async () => {
    await screen.findByText(/규칙 없음/);
  });
});
