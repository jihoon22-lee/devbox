import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createKnowledgeDraftHandoff } from "../api";
import { KnowledgeDraftAction } from "./KnowledgeDraftAction";

vi.mock("../api", () => ({
  createKnowledgeDraftHandoff: vi.fn(),
  KNOWLEDGE_DRAFT_BROWSER_ERROR:
    "Knowledge 초안 전달은 데스크톱 앱에서만 사용할 수 있습니다. 클립보드로 자동 전환하지 않습니다",
  KNOWLEDGE_DRAFT_CREATE_ERROR:
    "Knowledge 초안을 만들거나 전달하지 못했습니다. 클립보드로 자동 전환하지 않습니다",
  KNOWLEDGE_DRAFT_INPUT_ERROR: "Knowledge 초안으로 전달할 텍스트가 유효하지 않습니다",
  KNOWLEDGE_DRAFT_INVALID_ERROR: "Knowledge 초안 응답을 사용할 수 없습니다",
  KNOWLEDGE_DRAFT_TARGET_UNAVAILABLE_ERROR:
    "Knowledge를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다",
}));

const createKnowledgeDraftHandoffMock = vi.mocked(createKnowledgeDraftHandoff);
const handoffId = "0123456789abcdef0123456789abcdef";

beforeEach(() => {
  createKnowledgeDraftHandoffMock.mockReset().mockResolvedValue({
    handoffId,
    redacted: false,
  });
});

afterEach(() => cleanup());

function openPreview(value = "current output") {
  render(<KnowledgeDraftAction value={value} />);
  fireEvent.click(screen.getByRole("button", { name: "Knowledge에 초안 저장" }));
  return screen.getByRole("dialog", { name: "Knowledge 초안 미리보기" });
}

describe("Knowledge draft output action", () => {
  it("previews locally, focuses Cancel, and publishes only after confirmation", async () => {
    const dialog = openPreview("current output");

    expect(dialog.textContent).toContain("current output");
    expect(dialog.textContent).toMatch(/자 · .*바이트/u);
    expect(createKnowledgeDraftHandoffMock).not.toHaveBeenCalled();
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "취소" })));

    fireEvent.click(screen.getByRole("button", { name: "초안 저장" }));
    await waitFor(() => expect(createKnowledgeDraftHandoffMock).toHaveBeenCalledWith("current output"));
    expect(screen.queryByRole("dialog", { name: "Knowledge 초안 미리보기" })).toBeNull();
    expect(screen.getByRole("status").textContent).toContain("Knowledge 초안 미리보기로 전달했습니다");
    expect(screen.getByRole("status").textContent).toContain("저장은 Knowledge에서 확인하세요");
  });

  it("distinguishes a redacted success without exposing the handoff id", async () => {
    createKnowledgeDraftHandoffMock.mockResolvedValueOnce({ handoffId, redacted: true });
    openPreview("password=raw-value");

    fireEvent.click(screen.getByRole("button", { name: "초안 저장" }));
    const status = await screen.findByRole("status");
    expect(status.textContent).toContain("민감한 값은 마스킹되었습니다");
    expect(status.textContent).not.toContain(handoffId);
  });

  it("cancels without publishing or using clipboard", () => {
    const writeText = vi.fn();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    openPreview("cancelled output");

    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    expect(createKnowledgeDraftHandoffMock).not.toHaveBeenCalled();
    expect(writeText).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog", { name: "Knowledge 초안 미리보기" })).toBeNull();
  });

  it("rejects an unbounded output with a fixed error before native invocation", () => {
    openPreview("unsafe\0output");

    fireEvent.click(screen.getByRole("button", { name: "초안 저장" }));
    expect(screen.getByRole("alert").textContent).toContain("Knowledge 초안으로 전달할 텍스트가 유효하지 않습니다");
    expect(createKnowledgeDraftHandoffMock).not.toHaveBeenCalled();
  });

  it("closes on output change and ignores a late publish response", async () => {
    let resolveDispatch!: (value: { handoffId: string; redacted: boolean }) => void;
    createKnowledgeDraftHandoffMock.mockImplementationOnce(
      () => new Promise((resolve) => { resolveDispatch = resolve; }),
    );
    const view = render(<KnowledgeDraftAction value="first output" />);
    fireEvent.click(screen.getByRole("button", { name: "Knowledge에 초안 저장" }));
    fireEvent.click(screen.getByRole("button", { name: "초안 저장" }));
    expect(createKnowledgeDraftHandoffMock).toHaveBeenCalledWith("first output");

    view.rerender(<KnowledgeDraftAction value="second output" />);
    expect(screen.queryByRole("dialog", { name: "Knowledge 초안 미리보기" })).toBeNull();
    resolveDispatch({ handoffId, redacted: false });
    await act(async () => Promise.resolve());
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.getByRole("button", { name: "Knowledge에 초안 저장" })).toBeTruthy();
  });

  it("ignores a late response after unmount", async () => {
    let resolveDispatch!: (value: { handoffId: string; redacted: boolean }) => void;
    createKnowledgeDraftHandoffMock.mockImplementationOnce(
      () => new Promise((resolve) => { resolveDispatch = resolve; }),
    );
    const view = render(<KnowledgeDraftAction value="unmounted output" />);
    fireEvent.click(screen.getByRole("button", { name: "Knowledge에 초안 저장" }));
    fireEvent.click(screen.getByRole("button", { name: "초안 저장" }));
    view.unmount();

    await act(async () => {
      resolveDispatch({ handoffId, redacted: false });
      await Promise.resolve();
    });
    expect(createKnowledgeDraftHandoffMock).toHaveBeenCalledTimes(1);
  });

  it("does not echo an unexpected native error or copy output", async () => {
    createKnowledgeDraftHandoffMock.mockRejectedValueOnce(new Error("/private/output/path"));
    const writeText = vi.fn();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    openPreview("safe output");
    fireEvent.click(screen.getByRole("button", { name: "초안 저장" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("Knowledge 초안을 만들거나 전달하지 못했습니다");
    expect(alert.textContent).not.toContain("/private/output/path");
    expect(writeText).not.toHaveBeenCalled();
  });
});
