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
    expect(screen.getByRole("button", { name: "Connect WebSocket" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Disconnect WebSocket" }).getAttribute("disabled")).not.toBeNull();
    expect(screen.getByRole("log", { name: "WebSocket messages" }).getAttribute("aria-live")).toBe("polite");
    expect(screen.getByRole("status", { name: "WebSocket Idle" }).textContent).toBe("Idle");
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
    expect(screen.getByText(/2 retained · 3 evicted/u)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Save binary message 2" }));
    expect(onSaveBinary).toHaveBeenCalledWith(2);
  });

  it("keeps send and close controls disabled until the socket is open", () => {
    render(<WebSocketPanel {...baseProps} state="connecting" />);

    expect(screen.getByRole("button", { name: "Connect WebSocket" }).getAttribute("disabled")).not.toBeNull();
    expect(screen.getByRole("button", { name: "Disconnect WebSocket" }).getAttribute("disabled")).toBeNull();
    expect(screen.getByRole("button", { name: "Send WebSocket message" }).getAttribute("disabled")).not.toBeNull();
    expect(screen.getByRole("button", { name: /^Close$/u }).getAttribute("disabled")).not.toBeNull();
  });
});
