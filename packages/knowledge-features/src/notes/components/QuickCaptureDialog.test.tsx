import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import QuickCaptureDialog from "./QuickCaptureDialog";
import { captureNote, readClipboardText } from "../api";
import { assertNoA11yViolations } from "@devbox/a11y/testing";

vi.mock("../api", () => ({
  captureNote: vi.fn(async () => ({ path: "Inbox/quick-capture-test.md", revision: "r1" })),
  readClipboardText: vi.fn(async () => "clipboard body"),
}));
const clipboardMock = vi.mocked(readClipboardText);
const saveMock = vi.mocked(captureNote);
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  clipboardMock.mockResolvedValue("clipboard body");
  saveMock.mockResolvedValue({ path: "Inbox/quick-capture-test.md", revision: "r1" });
});

function renderDialog(onClose = vi.fn(), onSaved = vi.fn()) {
  render(<QuickCaptureDialog open onClose={onClose} onSaved={onSaved} />);
  return { onClose, onSaved };
}

describe("Knowledge quick capture dialog", () => {
  it("reads clipboard only after explicit action and keeps the modal keyboard accessible", async () => {
    renderDialog();
    const dialog = screen.getByRole("dialog", { name: "빠른 캡처" });
    expect(clipboardMock).not.toHaveBeenCalled();
    expect(within(dialog).getByLabelText(/본문/u)).toHaveValue("");

    fireEvent.click(within(dialog).getByRole("button", { name: "클립보드에서 본문 가져오기" }));
    await waitFor(() => {
      expect(clipboardMock).toHaveBeenCalledTimes(1);
      expect(within(dialog).getByLabelText(/본문/u)).toHaveValue("clipboard body");
    });

    fireEvent.keyDown(within(dialog).getByLabelText(/본문/u), { key: "Escape" });
    expect(await screen.findByRole("button", { name: "빠른 캡처 닫기" })).toBeInTheDocument();
  });

  it("does not retain an oversized clipboard payload in the draft", async () => {
    clipboardMock.mockResolvedValueOnce("x".repeat(128 * 1024 + 1));
    renderDialog();
    const dialog = screen.getByRole("dialog", { name: "빠른 캡처" });
    fireEvent.click(within(dialog).getByRole("button", { name: "클립보드에서 본문 가져오기" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("본문은 LF 기준 64 KiB(원문 128 KiB) 이내로 입력하세요");
    expect(within(dialog).getByLabelText(/본문/u)).toHaveValue("");
  });

  it("rejects credential-like clipboard text before retaining it in the draft", async () => {
    clipboardMock.mockResolvedValueOnce("X-API-Key: super-secret-value");
    renderDialog();
    const dialog = screen.getByRole("dialog", { name: "빠른 캡처" });
    fireEvent.click(within(dialog).getByRole("button", { name: "클립보드에서 본문 가져오기" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("민감한 정보가 포함되어 있어 저장하지 않았습니다");
    expect(within(dialog).getByLabelText(/본문/u)).toHaveValue("");
    expect(alert).not.toHaveTextContent("super-secret-value");
  });

  it("creates a note with one save and no preview", async () => {
    const { onClose, onSaved } = renderDialog();
    fireEvent.change(screen.getByLabelText(/본문/u), { target: { value: "body" } });
    expect(screen.queryByRole("button", { name: "미리보기" })).toBeNull();
    await assertNoA11yViolations(screen.getByRole("dialog"));
    fireEvent.click(screen.getByRole("button", { name: "저장" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledWith({ title: "", body: "body", tags: [] }));
    expect(onSaved).toHaveBeenCalledWith({ path: "Inbox/quick-capture-test.md", revision: "r1" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not submit a composing Ctrl+Enter shortcut", () => {
    renderDialog();
    const body = screen.getByRole("textbox", { name: /본문/u });
    fireEvent.change(body, { target: { value: "조합 중" } });
    fireEvent.keyDown(body, { key: "Enter", ctrlKey: true, isComposing: true });
    expect(saveMock).not.toHaveBeenCalled();
  });

  it("blocks duplicate saves and ignores Escape while native save is busy", async () => {
    let resolveSave: ((value: { path: string; revision: string }) => void) | undefined;
    saveMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveSave = resolve;
        }),
    );
    const { onClose } = renderDialog();
    const dialog = screen.getByRole("dialog", { name: "빠른 캡처" });
    fireEvent.change(within(dialog).getByLabelText(/본문/), { target: { value: "body" } });
    fireEvent.click(screen.getByRole("button", { name: "저장" }));
    fireEvent.click(screen.getByRole("button", { name: "저장 중…" }));
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(saveMock).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();

    resolveSave?.({ path: "Inbox/quick-capture-test.md", revision: "r1" });
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("shows a safe validation message without echoing rejected input", async () => {
    saveMock.mockRejectedValueOnce(new Error("민감한 정보가 포함되어 있어 저장하지 않았습니다"));
    renderDialog();
    const dialog = screen.getByRole("dialog", { name: "빠른 캡처" });
    fireEvent.change(within(dialog).getByLabelText(/본문/), {
      target: { value: "api_key=super-secret-value" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "저장" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("민감한 정보가 포함되어 있어 저장하지 않았습니다");
    expect(alert).not.toHaveTextContent("super-secret-value");
  });

  it("shows UTF-8 byte budgets beside editable fields", () => {
    renderDialog();
    const dialog = screen.getByRole("dialog", { name: "빠른 캡처" });
    expect(dialog).toHaveTextContent("0 / 800 bytes · 0 / 200자");
    expect(dialog).toHaveTextContent("LF 0 / 65536 bytes · 원문 0 / 131072 bytes");
    expect(dialog).toHaveTextContent("0 / 20개 · 0 / 1024 bytes");
  });
});
