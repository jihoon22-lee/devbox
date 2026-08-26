import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import QuickCaptureDialog from "./QuickCaptureDialog";
import {
  previewQuickCapture,
  readClipboardText,
  saveQuickCapture,
} from "../api";
import type { QuickCapturePreview } from "../types";

vi.mock("../api", () => ({
  previewQuickCapture: vi.fn(async (input: { title: string; body: string; tags: string[] }) => ({
    target: "Inbox",
    ...input,
  })),
  readClipboardText: vi.fn(async () => "clipboard body"),
  saveQuickCapture: vi.fn(async () => ({ path: "Inbox/quick-capture-test.md" })),
}));

const previewMock = vi.mocked(previewQuickCapture);
const clipboardMock = vi.mocked(readClipboardText);
const saveMock = vi.mocked(saveQuickCapture);

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  clipboardMock.mockResolvedValue("clipboard body");
  previewMock.mockImplementation(async (input) => ({ target: "Inbox", ...input }));
  saveMock.mockResolvedValue({ path: "Inbox/quick-capture-test.md" });
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
    await waitFor(() => expect(clipboardMock).toHaveBeenCalledTimes(1));
    expect(within(dialog).getByLabelText(/본문/u)).toHaveValue("clipboard body");

    fireEvent.keyDown(within(dialog).getByLabelText(/본문/u), { key: "Escape" });
    expect(await screen.findByRole("button", { name: "빠른 캡처 닫기" })).toBeInTheDocument();
  });

  it("does not retain an oversized clipboard payload in the draft", async () => {
    clipboardMock.mockResolvedValueOnce("x".repeat(64 * 1024 + 1));
    renderDialog();
    const dialog = screen.getByRole("dialog", { name: "빠른 캡처" });
    fireEvent.click(within(dialog).getByRole("button", { name: "클립보드에서 본문 가져오기" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("빠른 캡처 입력이 올바르지 않습니다");
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

  it("previews the fixed Inbox target before saving and forwards normalized fields", async () => {
    previewMock.mockResolvedValueOnce({
      target: "Inbox",
      title: "Idea",
      body: "body",
      tags: ["rust", "offline"],
    });
    const { onClose, onSaved } = renderDialog();
    const dialog = screen.getByRole("dialog", { name: "빠른 캡처" });
    fireEvent.change(within(dialog).getByLabelText(/제목/), { target: { value: "  Idea  " } });
    fireEvent.change(within(dialog).getByLabelText(/본문/), { target: { value: "body\r\n" } });
    fireEvent.change(within(dialog).getByLabelText(/태그/), { target: { value: "rust, rust, offline" } });

    fireEvent.click(within(dialog).getByRole("button", { name: "미리보기" }));
    await waitFor(() => expect(previewMock).toHaveBeenCalledWith({
      title: "  Idea  ",
      body: "body\n",
      tags: ["rust", "rust", "offline"],
    }));
    expect(dialog).toHaveTextContent("Inbox");
    expect(dialog).toHaveTextContent("Idea");
    expect(dialog).toHaveTextContent("rust, offline");
    expect(dialog).toHaveTextContent("body");
    expect(saveMock).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: "저장" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledWith({
      title: "Idea",
      body: "body",
      tags: ["rust", "offline"],
    }));
    expect(onSaved).toHaveBeenCalledWith({ path: "Inbox/quick-capture-test.md" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not submit a composing Ctrl+Enter shortcut", () => {
    renderDialog();
    const body = screen.getByRole("textbox", { name: /본문/u });
    fireEvent.change(body, { target: { value: "조합 중" } });
    fireEvent.keyDown(body, { key: "Enter", ctrlKey: true, isComposing: true });
    expect(previewMock).not.toHaveBeenCalled();
  });

  it("blocks duplicate saves and ignores Escape while native save is busy", async () => {
    let resolveSave: ((value: { path: string }) => void) | undefined;
    saveMock.mockImplementationOnce(() => new Promise((resolve) => { resolveSave = resolve; }));
    const { onClose } = renderDialog();
    const dialog = screen.getByRole("dialog", { name: "빠른 캡처" });
    fireEvent.change(within(dialog).getByLabelText(/본문/), { target: { value: "body" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "미리보기" }));
    await screen.findByRole("button", { name: "저장" });
    fireEvent.click(screen.getByRole("button", { name: "저장" }));
    fireEvent.click(screen.getByRole("button", { name: "저장 중…" }));
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(saveMock).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();

    resolveSave?.({ path: "Inbox/quick-capture-test.md" });
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("discards a late preview completion after unmount", async () => {
    let resolvePreview: ((value: QuickCapturePreview) => void) | undefined;
    previewMock.mockImplementationOnce(() => new Promise((resolve) => { resolvePreview = resolve; }));
    renderDialog();
    const dialog = screen.getByRole("dialog", { name: "빠른 캡처" });
    fireEvent.change(within(dialog).getByLabelText(/본문/u), { target: { value: "body" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "미리보기" }));
    cleanup();
    resolvePreview?.({ target: "Inbox", title: "late", body: "late", tags: [] });
    await waitFor(() => expect(previewMock).toHaveBeenCalledTimes(1));
  });

  it("shows a safe validation message without echoing rejected input", async () => {
    previewMock.mockRejectedValueOnce(new Error("민감한 정보가 포함되어 있어 저장하지 않았습니다"));
    renderDialog();
    const dialog = screen.getByRole("dialog", { name: "빠른 캡처" });
    fireEvent.change(within(dialog).getByLabelText(/본문/), {
      target: { value: "api_key=super-secret-value" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "미리보기" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("민감한 정보가 포함되어 있어 저장하지 않았습니다");
    expect(alert).not.toHaveTextContent("super-secret-value");
  });
});
