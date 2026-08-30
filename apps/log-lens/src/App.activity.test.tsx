import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  cancelRead,
  discardLogSource,
  exportRecords,
  onOpenRequest,
  readSources,
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
  handoffErrorCode: vi.fn(() => null),
  onOpenRequest: vi.fn(),
  previewLogSource: vi.fn(),
  readSources: vi.fn(),
  renewLogSource: vi.fn(),
  sendSelectionToToolbox: vi.fn(),
  takePendingOpen: vi.fn(),
}));

const cancelReadMock = vi.mocked(cancelRead);
const discardLogSourceMock = vi.mocked(discardLogSource);
const exportRecordsMock = vi.mocked(exportRecords);
const onOpenRequestMock = vi.mocked(onOpenRequest);
const readSourcesMock = vi.mocked(readSources);
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
  onOpenRequestMock.mockReset().mockResolvedValue(() => undefined);
  readSourcesMock.mockReset().mockResolvedValue(browserSnapshot([], "refresh", 1));
  sendSelectionToToolboxMock.mockReset().mockResolvedValue({
    handoffId: "0123456789abcdef0123456789abcdef",
    redacted: false,
  });
  takePendingOpenMock.mockReset().mockResolvedValue(null);
});

afterEach(() => cleanup());

function firstLogCheckbox(): HTMLInputElement {
  return screen.getByRole("checkbox", { name: "Select log line 0" }) as HTMLInputElement;
}

function toolboxButton(): HTMLButtonElement {
  return screen.getByRole("button", { name: "Send selected logs to Developer Toolbox" }) as HTMLButtonElement;
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
    expect(screen.getByRole("status").textContent).toContain("sensitive values were redacted");
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

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(readSourcesMock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("Selected logs are stale"));
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

    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("Selected logs are stale"));
    expect(sendSelectionToToolboxMock).not.toHaveBeenCalled();
  });

  it("surfaces a truncated clipboard export after the write completes", async () => {
    exportRecordsMock.mockResolvedValueOnce({ text: "partial\n", truncated: true });
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("partial\n"));
    expect(screen.getByRole("alert").textContent).toBe("Export reached the safety limit and was truncated.");
  });

  it("uses fixed handoff failure feedback without clipboard fallback", async () => {
    sendSelectionToToolboxMock.mockRejectedValueOnce(new Error("C:\\private\\secret.log"));
    render(<App />);
    fireEvent.click(firstLogCheckbox());
    fireEvent.click(toolboxButton());

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("Selected logs could not be sent to Developer Toolbox. Clipboard fallback was not used.");
    expect(alert.textContent).not.toContain("secret.log");
    expect(writeText).not.toHaveBeenCalled();
  });
});
