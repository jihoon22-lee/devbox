import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WebSocketPanel } from "./WebSocketPanel";
import type { WebSocketMessage } from "./types";

afterEach(() => cleanup());

const baseProps = {
  state: "open" as const,
  messages: [] as readonly WebSocketMessage[],
  dropped: 0,
  native: true,
  canConnect: true,
  busy: false,
  onConnect: vi.fn(),
  onDisconnect: vi.fn(),
  onSend: vi.fn(),
  onPing: vi.fn(),
  onClose: vi.fn(),
  onSaveBinary: vi.fn(),
};

describe("WebSocketPanel", () => {
  it("exposes explicit connection controls and an accessible live log", () => {
    render(<WebSocketPanel {...baseProps} state="idle" />);

    expect(screen.getByRole("heading", { name: "WebSocket" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "WebSocket 연결" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "WebSocket 연결 해제" }).getAttribute("disabled")).not.toBeNull();
    expect(screen.getByRole("log", { name: "WebSocket 메시지" }).getAttribute("aria-live")).toBe("polite");
    expect(screen.getByRole("status", { name: "WebSocket 대기" }).textContent).toBe("대기");
  });

  it("renders masked message text as text and offers an explicit binary save action", () => {
    const onSaveBinary = vi.fn();
    render(
      <WebSocketPanel
        {...baseProps}
        onSaveBinary={onSaveBinary}
        messages={[
          { id: 1, direction: "received", kind: "text", text: "<script>alert(1)</script>" },
          { id: 2, direction: "received", kind: "binary", binaryHex: "0102", binaryText: "ok", binarySize: 2 },
        ]}
        dropped={3}
      />,
    );

    expect(screen.getByText("<script>alert(1)</script>")).toBeTruthy();
    expect(screen.getByRole("log").innerHTML).toContain("&lt;script&gt;");
    expect(screen.getByText(/2개 유지 · 3개 제외됨/u)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Binary 메시지 2 저장" }));
    expect(onSaveBinary).toHaveBeenCalledWith(2);
  });

  it("keeps send and close controls disabled until the socket is open", () => {
    render(<WebSocketPanel {...baseProps} state="connecting" />);

    expect(screen.getByRole("button", { name: "WebSocket 연결" }).getAttribute("disabled")).not.toBeNull();
    expect(screen.getByRole("button", { name: "WebSocket 연결 해제" }).getAttribute("disabled")).toBeNull();
    expect(screen.getByRole("button", { name: "WebSocket 메시지 보내기" }).getAttribute("disabled")).not.toBeNull();
    expect(screen.getByRole("button", { name: "닫기" }).getAttribute("disabled")).not.toBeNull();
  });
});
