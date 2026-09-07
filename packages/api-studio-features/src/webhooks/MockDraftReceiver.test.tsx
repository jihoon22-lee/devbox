import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("../transport", () => ({ componentInvoke: () => native.invoke }));
vi.mock("./lib/isTauri", () => ({ isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));
import { MockDraftReceiver, parseMockPreview } from "./MockDraftReceiver";
const rule = { id: "", priority: 0, method: null, path: "/", status: 200,
  headers: [["Content-Type", "text/plain; charset=utf-8"]], body: "safe [REDACTED]", delayMs: 0 };
const preview = { id: "a".repeat(32), producer: "api-playground", expiresAtMs: 600000, redacted: true, rule };
afterEach(() => { cleanup(); vi.clearAllMocks(); });
describe("Mock draft recipient", () => {
  it("focuses a queued preview only after its product route becomes visible", async () => {
    native.invoke.mockResolvedValue(preview);
    const onApply = vi.fn();
    const view = render(<MockDraftReceiver active={false} disabled={false} onApply={onApply} />);
    await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("peek_mock_draft"));
    expect(screen.queryByRole("dialog")).toBeNull();
    view.rerender(<MockDraftReceiver active disabled={false} onApply={onApply} />);
    await screen.findByRole("dialog");
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "취소" }));
  });
  it("previews without changing a draft or listener and consumes only on explicit apply", async () => {
    native.invoke.mockImplementation(async (method: string) => method === "peek_mock_draft" ? preview : rule);
    const onApply = vi.fn(); render(<MockDraftReceiver disabled={false} onApply={onApply} />);
    await screen.findByRole("dialog", { name: "Mock 규칙 초안 미리보기" });
    expect(onApply).not.toHaveBeenCalled();
    expect(native.invoke).toHaveBeenCalledExactlyOnceWith("peek_mock_draft");
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "취소" }));
    fireEvent.click(screen.getByRole("button", { name: "현재 규칙 초안 대신 적용" }));
    await waitFor(() => expect(onApply).toHaveBeenCalledExactlyOnceWith(rule));
    expect(native.invoke.mock.calls.map(call => call[0])).toEqual(["peek_mock_draft", "accept_mock_draft"]);
  });
  it("cancel consumes the preview without replacing the editor", async () => {
    native.invoke.mockImplementation(async (method: string) => method === "peek_mock_draft" ? preview : null);
    const onApply = vi.fn(); render(<MockDraftReceiver disabled={false} onApply={onApply} />);
    await screen.findByRole("dialog"); fireEvent.click(screen.getByRole("button", { name: "취소" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(onApply).not.toHaveBeenCalled();
    expect(native.invoke.mock.calls.map(call => call[0])).toEqual(["peek_mock_draft", "discard_mock_draft"]);
  });
  it("does not trust extra execution fields, a foreign producer or source headers", () => {
    for (const value of [{ ...preview, producer: "workspace.runtime" },
      { ...preview, rule: { ...rule, enabled: true } },
      { ...preview, rule: { ...rule, headers: [["Authorization", "synthetic-secret"]] } }]) {
      expect(() => parseMockPreview(value)).toThrow();
    }
  });
});
