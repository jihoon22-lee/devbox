import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  cancelRead,
  discardLogSource,
  exportRecords,
  listSavedViews,
  onOpenRequest,
  readSources,
  removeSavedView,
  saveSavedView,
  sendSelectionToToolbox,
  takePendingOpen,
} from "./api";
import { browserSnapshot } from "./browserFixture";

vi.mock("./api", () => ({
  acceptLogSource: vi.fn(),
  cancelRead: vi.fn(async () => undefined),
  classifyHandoffError: vi.fn(() => "terminal"),
  discardLogSource: vi.fn(),
  exportRecords: vi.fn(),
  listSavedViews: vi.fn(),
  handoffErrorCode: vi.fn(() => null),
  onOpenRequest: vi.fn(),
  previewLogSource: vi.fn(),
  readSources: vi.fn(),
  removeSavedView: vi.fn(),
  renewLogSource: vi.fn(),
  saveSavedView: vi.fn(),
  sendSelectionToToolbox: vi.fn(),
  takePendingOpen: vi.fn(),
}));

const cancelReadMock = vi.mocked(cancelRead);
const discardLogSourceMock = vi.mocked(discardLogSource);
const exportRecordsMock = vi.mocked(exportRecords);
const listSavedViewsMock = vi.mocked(listSavedViews);
const onOpenRequestMock = vi.mocked(onOpenRequest);
const readSourcesMock = vi.mocked(readSources);
const removeSavedViewMock = vi.mocked(removeSavedView);
const saveSavedViewMock = vi.mocked(saveSavedView);
const sendSelectionToToolboxMock = vi.mocked(sendSelectionToToolbox);
const takePendingOpenMock = vi.mocked(takePendingOpen);

const writeText = vi.fn<(value: string) => Promise<void>>();

beforeEach(() => {
  writeText.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
  cancelReadMock.mockReset().mockResolvedValue(undefined);
  discardLogSourceMock.mockReset().mockResolvedValue(undefined);
  exportRecordsMock.mockReset().mockResolvedValue({ text: "exported selection\n", truncated: false });
  listSavedViewsMock.mockReset().mockResolvedValue({ schemaVersion: 1, revision: 0, views: [] });
  onOpenRequestMock.mockReset().mockResolvedValue(() => undefined);
  readSourcesMock.mockReset().mockResolvedValue(browserSnapshot([], "refresh", 1));
  removeSavedViewMock.mockReset().mockResolvedValue({ schemaVersion: 1, revision: 0, views: [] });
  saveSavedViewMock.mockReset().mockResolvedValue({ schemaVersion: 1, revision: 0, views: [] });
  sendSelectionToToolboxMock.mockReset().mockResolvedValue({
    handoffId: "0123456789abcdef0123456789abcdef",
    redacted: false,
  });
  takePendingOpenMock.mockReset().mockResolvedValue(null);
});

afterEach(() => cleanup());

function firstLogCheckbox(): HTMLInputElement {
  return screen.getByRole("checkbox", { name: "로그 줄 0 선택" }) as HTMLInputElement;
}

function toolboxButton(): HTMLButtonElement {
  return screen.getByRole("button", { name: "선택 로그를 Developer Toolbox로 보내기" }) as HTMLButtonElement;
}

describe("Log Lens Developer Toolbox activity handoff", () => {
  it("sends only explicitly selected current records and reports redaction", async () => {
    sendSelectionToToolboxMock.mockResolvedValueOnce({
      handoffId: "0123456789abcdef0123456789abcdef",
      redacted: true,
    });
    render(<App />);

    expect(toolboxButton().disabled).toBe(true);
    fireEvent.click(firstLogCheckbox());
    expect(toolboxButton().disabled).toBe(false);

    fireEvent.click(toolboxButton());
    await waitFor(() => expect(sendSelectionToToolboxMock).toHaveBeenCalledWith("exported selection\n"));
    expect(exportRecordsMock).toHaveBeenCalledTimes(1);
    expect(exportRecordsMock.mock.calls[0][0]).toHaveLength(1);
    expect(writeText).not.toHaveBeenCalled();
    expect(screen.getByRole("status").textContent).toContain("민감정보를 마스킹");
  });

  it("has no visible-record fallback and stays disabled without an explicit selection", () => {
    render(<App />);

    expect(toolboxButton().disabled).toBe(true);
    fireEvent.click(toolboxButton());
    expect(exportRecordsMock).not.toHaveBeenCalled();
    expect(sendSelectionToToolboxMock).not.toHaveBeenCalled();
    expect(writeText).not.toHaveBeenCalled();
  });

  it("rejects a selection as soon as a source generation changes", async () => {
    render(<App />);
    fireEvent.click(firstLogCheckbox());
    expect(toolboxButton().disabled).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "새로고침" }));
    await waitFor(() => expect(readSourcesMock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("선택한 로그가 최신 상태가 아닙니다"));
    expect(toolboxButton().disabled).toBe(true);
    expect(sendSelectionToToolboxMock).not.toHaveBeenCalled();
  });

  it("drops a late export when the explicit selection changes", async () => {
    let resolveExport!: (value: { text: string; truncated: boolean }) => void;
    exportRecordsMock.mockImplementationOnce(() => new Promise((resolve) => {
      resolveExport = resolve;
    }));
    render(<App />);
    fireEvent.click(firstLogCheckbox());
    fireEvent.click(toolboxButton());

    fireEvent.click(firstLogCheckbox());
    resolveExport({ text: "late selection\n", truncated: false });

    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("선택한 로그가 최신 상태가 아닙니다"));
    expect(sendSelectionToToolboxMock).not.toHaveBeenCalled();
  });

  it("surfaces a truncated clipboard export after the write completes", async () => {
    exportRecordsMock.mockResolvedValueOnce({ text: "partial\n", truncated: true });
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "복사" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("partial\n"));
    expect(screen.getByRole("alert").textContent).toBe("내보내기 안전 제한에 도달해 일부 내용만 처리했습니다.");
  });

  it("uses fixed handoff failure feedback without clipboard fallback", async () => {
    sendSelectionToToolboxMock.mockRejectedValueOnce(new Error("C:\\private\\secret.log"));
    render(<App />);
    fireEvent.click(firstLogCheckbox());
    fireEvent.click(toolboxButton());

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("선택한 로그를 Developer Toolbox로 보내지 못했습니다. 클립보드로 자동 전환하지 않았습니다.");
    expect(alert.textContent).not.toContain("secret.log");
    expect(writeText).not.toHaveBeenCalled();
  });
});
