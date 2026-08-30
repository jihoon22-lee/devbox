import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  acceptLogSource,
  discardLogSource,
  onOpenRequest,
  previewLogSource,
  readSources,
  renewLogSource,
  takePendingOpen,
} from "./api";
import type { LogSourcePreview } from "./types";

const mocks = vi.hoisted(() => ({
  openHandler: null as (() => void) | null,
}));

vi.mock("./api", () => ({
  acceptLogSource: vi.fn(),
  cancelRead: vi.fn(async () => undefined),
  discardLogSource: vi.fn(),
  exportRecords: vi.fn(),
  onOpenRequest: vi.fn(),
  previewLogSource: vi.fn(),
  readSources: vi.fn(),
  renewLogSource: vi.fn(),
  sendSelectionToToolbox: vi.fn(),
  takePendingOpen: vi.fn(),
  classifyHandoffError: (error: unknown) => {
    const code = error instanceof Error ? error.message : "";
    return ["handoff-storage-failed", "handoff-claim-storage-failed", "handoff-restore-failed", "handoff-response-invalid"]
      .includes(code)
      ? "retryable"
      : "terminal";
  },
  handoffErrorCode: (error: unknown) => error instanceof Error ? error.message : null,
}));

const acceptLogSourceMock = vi.mocked(acceptLogSource);
const discardLogSourceMock = vi.mocked(discardLogSource);
const onOpenRequestMock = vi.mocked(onOpenRequest);
const previewLogSourceMock = vi.mocked(previewLogSource);
const readSourcesMock = vi.mocked(readSources);
const renewLogSourceMock = vi.mocked(renewLogSource);
const takePendingOpenMock = vi.mocked(takePendingOpen);

const firstId = "0123456789abcdef0123456789abcdef";
const secondId = "fedcba9876543210fedcba9876543210";

function previewFor(id: string): LogSourcePreview {
  return {
    id,
    kind: "log-source/v1",
    sourceApp: "run-manager",
    expiresAtMs: Date.now() + 600_000,
    leaseUntilMs: Date.now() + 60_000,
    source: {
      sourceId: "log-source:0123456789abcdef",
      kind: "run",
      displayName: "Run Manager handoff",
      readOnly: true,
      handoff: true,
    },
  };
}

function request(id: string) {
  return {
    target: { kind: "handoff" as const, handoffKind: "log-source/v1", id },
    from: "run-manager",
  };
}

async function openPreview(id = firstId) {
  takePendingOpenMock.mockResolvedValueOnce(request(id));
  await act(async () => mocks.openHandler?.());
  return screen.findByRole("dialog", { name: "Log Lens source 미리보기" });
}

beforeEach(() => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  mocks.openHandler = null;
  onOpenRequestMock.mockReset().mockImplementation(async (handler) => {
    mocks.openHandler = handler;
    return () => undefined;
  });
  takePendingOpenMock.mockReset().mockResolvedValue(null);
  previewLogSourceMock.mockReset().mockImplementation(async (id) => previewFor(id));
  readSourcesMock.mockReset().mockResolvedValue({
    operationId: "test-operation",
    generation: 1,
    sources: [],
    records: [],
    cursors: [],
    statuses: [],
    truncated: false,
    droppedRecords: 0,
    droppedBytes: 0,
  });
  acceptLogSourceMock.mockReset().mockResolvedValue({
    kind: "run",
    sourceId: "run-manager:run-1:stdout",
  });
  discardLogSourceMock.mockReset().mockResolvedValue(undefined);
  renewLogSourceMock.mockReset().mockResolvedValue(Date.now() + 60_000);
});

afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});

describe("Log Lens handoff lifecycle", () => {
  it("clears a stale modal on terminal cancel and drains the newest queued id", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    await openPreview();

    discardLogSourceMock.mockRejectedValueOnce(new Error("handoff-expired"));
    takePendingOpenMock.mockResolvedValueOnce(request(secondId));
    await act(async () => mocks.openHandler?.());
    fireEvent.click(screen.getByRole("button", { name: "취소" }));

    await waitFor(() => expect(previewLogSourceMock).toHaveBeenCalledWith(secondId));
    expect(screen.getByRole("dialog", { name: "Log Lens source 미리보기" })).toBeTruthy();
    expect(discardLogSourceMock).toHaveBeenCalledWith(firstId);
  });

  it("clears a stale modal on terminal accept without exposing native details", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    await openPreview();

    acceptLogSourceMock.mockRejectedValueOnce(new Error("handoff-lease-expired"));
    takePendingOpenMock.mockResolvedValueOnce(request(secondId));
    await act(async () => mocks.openHandler?.());
    fireEvent.click(screen.getByRole("button", { name: "읽기 전용 source 추가" }));

    await waitFor(() => expect(previewLogSourceMock).toHaveBeenCalledWith(secondId));
    expect(screen.getByRole("dialog", { name: "Log Lens source 미리보기" })).toBeTruthy();
    expect(document.body.textContent).not.toContain("handoff-lease-expired");
    expect(acceptLogSourceMock).toHaveBeenCalledWith(firstId);
  });

  it("retains the exact claim for bounded discard retry", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    await openPreview();

    discardLogSourceMock
      .mockRejectedValueOnce(new Error("handoff-restore-failed"))
      .mockResolvedValueOnce(undefined);
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    await screen.findByRole("button", { name: "복구 재시도" });
    expect(screen.getByRole("dialog", { name: "Log Lens source 미리보기" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "복구 재시도" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Log Lens source 미리보기" })).toBeNull());
    expect(discardLogSourceMock).toHaveBeenNthCalledWith(1, firstId);
    expect(discardLogSourceMock).toHaveBeenNthCalledWith(2, firstId);
  });

  it("offers cleanup retry when a claimed preview response is malformed", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    previewLogSourceMock.mockRejectedValueOnce(new Error("handoff-response-invalid"));
    takePendingOpenMock.mockResolvedValueOnce(request(firstId));
    await act(async () => mocks.openHandler?.());

    await screen.findByRole("button", { name: "복구 재시도" });
    expect(screen.getByRole("dialog", { name: "Log Lens source handoff 복구" })).toBeTruthy();
    discardLogSourceMock.mockResolvedValueOnce(undefined);
    fireEvent.click(screen.getByRole("button", { name: "복구 재시도" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Log Lens source handoff 복구" })).toBeNull());
    expect(discardLogSourceMock).toHaveBeenCalledWith(firstId);
  });

  it("retains the exact claim for bounded accept retry", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    await openPreview();

    acceptLogSourceMock
      .mockRejectedValueOnce(new Error("handoff-storage-failed"))
      .mockResolvedValueOnce({ kind: "run", sourceId: "run-manager:run-1:stdout" });
    fireEvent.click(screen.getByRole("button", { name: "읽기 전용 source 추가" }));
    await screen.findByRole("button", { name: "source 추가 재시도" });
    expect(screen.getByRole("dialog", { name: "Log Lens source 미리보기" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "source 추가 재시도" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Log Lens source 미리보기" })).toBeNull());
    expect(acceptLogSourceMock).toHaveBeenNthCalledWith(1, firstId);
    expect(acceptLogSourceMock).toHaveBeenNthCalledWith(2, firstId);
  });

  it("stops recovery retries at the bounded limit without dropping the modal claim", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    await openPreview();

    discardLogSourceMock.mockRejectedValue(new Error("handoff-restore-failed"));
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    for (const expectedCalls of [2, 3]) {
      const retry = await screen.findByRole("button", { name: "복구 재시도" });
      fireEvent.click(retry);
      await waitFor(() => expect(discardLogSourceMock).toHaveBeenCalledTimes(expectedCalls));
    }

    const retry = screen.getByRole("button", { name: "복구 재시도" }) as HTMLButtonElement;
    expect(retry.disabled).toBe(true);
    expect(screen.getByText(/Log Lens를 재시작한 뒤 새 handoff/)).toBeTruthy();
    expect(screen.getByRole("dialog", { name: "Log Lens source 미리보기" })).toBeTruthy();
  });
});
