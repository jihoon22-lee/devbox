import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  dispatchWorkspaceTaskControl,
  getWorkspaceTaskControlReceipt,
  listWorkspaceTaskControls,
  type WorkspaceTaskControl,
  type WorkspaceTaskControlReceipt,
} from "../api";
import WorkspaceTaskControlPanel from "./WorkspaceTaskControlPanel";

vi.mock("../api", () => ({
  dispatchWorkspaceTaskControl: vi.fn(),
  getWorkspaceTaskControlReceipt: vi.fn(),
  listWorkspaceTaskControls: vi.fn(),
}));

const listWorkspaceTaskControlsMock = vi.mocked(listWorkspaceTaskControls);
const dispatchWorkspaceTaskControlMock = vi.mocked(dispatchWorkspaceTaskControl);
const getWorkspaceTaskControlReceiptMock = vi.mocked(getWorkspaceTaskControlReceipt);

const tasks: WorkspaceTaskControl[] = [
  {
    id: "process-build",
    label: "Build",
    revision: "a".repeat(64),
    taskKind: "process",
    trusted: true,
    shellTrusted: false,
    available: true,
    hasDependencies: true,
    operationActive: false,
  },
  {
    id: "shell-dev",
    label: "Dev shell",
    revision: "b".repeat(64),
    taskKind: "shell",
    trusted: true,
    shellTrusted: false,
    available: true,
    hasDependencies: false,
    operationActive: true,
  },
  {
    id: "untrusted-test",
    label: "Untrusted test",
    revision: "c".repeat(64),
    taskKind: "process",
    trusted: false,
    shellTrusted: false,
    available: true,
    hasDependencies: false,
    operationActive: false,
  },
  {
    id: "missing-task",
    label: "Missing task",
    revision: "d".repeat(64),
    taskKind: "process",
    trusted: true,
    shellTrusted: false,
    available: false,
    hasDependencies: false,
    operationActive: true,
  },
  {
    id: "idle-task",
    label: "Idle task",
    revision: "e".repeat(64),
    taskKind: "process",
    trusted: true,
    shellTrusted: false,
    available: true,
    hasDependencies: false,
    operationActive: false,
  },
];

const receipt = (
  patch: Partial<WorkspaceTaskControlReceipt> = {},
): WorkspaceTaskControlReceipt => ({
  schemaVersion: 1,
  requestId: "request-1",
  taskId: "process-build",
  action: "start",
  status: "started",
  operationId: "operation-1",
  failureCode: null,
  createdAt: 1,
  updatedAt: 2,
  ...patch,
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  listWorkspaceTaskControlsMock.mockReset().mockResolvedValue(tasks);
  dispatchWorkspaceTaskControlMock.mockReset().mockResolvedValue({
    requestId: "request-1",
    handoffId: "handoff-1",
  });
  getWorkspaceTaskControlReceiptMock.mockReset().mockResolvedValue(null);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("WorkspaceTaskControlPanel", () => {
  it("refreshes the snapshot and gates Start/Stop by the read-only trust state", async () => {
    render(<WorkspaceTaskControlPanel />);

    expect(await screen.findByRole("heading", { name: "Run Manager 작업" })).toBeTruthy();
    expect(listWorkspaceTaskControlsMock).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Build 시작" })).not.toBeDisabled();
    expect(screen.getByRole("button", { name: "Build 중지" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Dev shell 시작" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Dev shell 중지" })).not.toBeDisabled();
    expect(screen.getByRole("button", { name: "Untrusted test 시작" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Untrusted test 중지" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Missing task 시작" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Missing task 중지" })).not.toBeDisabled();
    expect(screen.getByRole("button", { name: "Idle task 시작" })).not.toBeDisabled();
    expect(screen.getAllByText("dependency 포함").length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole("button", { name: "Run Manager task snapshot 새로고침" }));
    await waitFor(() => expect(listWorkspaceTaskControlsMock).toHaveBeenCalledTimes(2));
  });

  it("keeps ARIA status references unique for valid punctuation-bearing task IDs", async () => {
    listWorkspaceTaskControlsMock.mockResolvedValueOnce([
      { ...tasks[0], id: "task:build", label: "Colon task" },
      { ...tasks[0], id: "task.build", label: "Dot task" },
    ]);
    render(<WorkspaceTaskControlPanel />);

    const colonButton = await screen.findByRole("button", { name: "Colon task 시작" });
    const dotButton = screen.getByRole("button", { name: "Dot task 시작" });
    const colonStatus = colonButton.getAttribute("aria-describedby");
    const dotStatus = dotButton.getAttribute("aria-describedby");

    expect(colonStatus).toBe("workspace-task-status-task:build");
    expect(dotStatus).toBe("workspace-task-status-task.build");
    expect(colonStatus).not.toBe(dotStatus);
    expect(document.getElementById(colonStatus!)).toBeTruthy();
    expect(document.getElementById(dotStatus!)).toBeTruthy();
  });

  it("keeps an exact request pending until Run Manager returns a terminal receipt", async () => {
    vi.useFakeTimers();
    getWorkspaceTaskControlReceiptMock
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(receipt());
    render(<WorkspaceTaskControlPanel />);

    await act(async () => {
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole("button", { name: "Build 시작" }));
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(dispatchWorkspaceTaskControlMock).toHaveBeenCalledWith({
      taskId: "process-build",
      action: "start",
      expectedRevision: "a".repeat(64),
    });
    expect(getWorkspaceTaskControlReceiptMock).toHaveBeenCalledWith("request-1");
    expect(screen.getAllByText("Run Manager 창의 확인을 기다리는 중…").length).toBeGreaterThan(0);
    expect(screen.getByText(/Run Manager 창의 확인 전에는 실행되지 않습니다/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Build 시작" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Idle task 시작" })).toBeDisabled();
    expect(screen.queryByText("Run Manager가 시작했습니다.")).toBeNull();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(500);
    });
    expect(getWorkspaceTaskControlReceiptMock).toHaveBeenCalledTimes(2);
    expect(screen.getAllByText("Run Manager가 시작했습니다.").length).toBeGreaterThan(0);
    expect(screen.queryByText(/창의 확인 전에는 실행되지 않습니다/)).toBeNull();
    expect(screen.getByRole("button", { name: "Build 시작" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Build 중지" })).not.toBeDisabled();
    expect(screen.getByRole("button", { name: "Idle task 시작" })).not.toBeDisabled();
  });

  it("allows only one global dispatch while the first request is in flight or pending", async () => {
    const dispatchDeferred = deferred<{
      requestId: string;
      handoffId: string;
    }>();
    dispatchWorkspaceTaskControlMock.mockReturnValueOnce(dispatchDeferred.promise);
    render(<WorkspaceTaskControlPanel />);
    await screen.findByRole("button", { name: "Build 시작" });

    fireEvent.click(screen.getByRole("button", { name: "Build 시작" }));
    expect(dispatchWorkspaceTaskControlMock).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Run Manager task snapshot 새로고침" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Idle task 시작" })).toBeDisabled();
    expect(screen.getByText(/다른 시작\/중지 요청은 잠시 막혀 있습니다/)).toBeTruthy();

    // This second click is intentionally attempted before the first dispatch
    // resolves. The synchronous ref gate must keep it from reaching invoke.
    fireEvent.click(screen.getByRole("button", { name: "Dev shell 중지" }));
    expect(dispatchWorkspaceTaskControlMock).toHaveBeenCalledTimes(1);

    dispatchDeferred.resolve({ requestId: "request-1", handoffId: "handoff-1" });
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(screen.getByRole("button", { name: "Idle task 시작" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Dev shell 중지" })).toBeDisabled();
  });

  it("releases the global guard after dispatch failure", async () => {
    dispatchWorkspaceTaskControlMock.mockRejectedValueOnce(new Error("C:\\private\\SECRET"));
    render(<WorkspaceTaskControlPanel />);
    await screen.findByRole("button", { name: "Build 시작" });

    fireEvent.click(screen.getByRole("button", { name: "Build 시작" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Run Manager task 요청을 완료하지 못했습니다.");
    expect(screen.getByRole("button", { name: "Idle task 시작" })).not.toBeDisabled();
    expect(screen.queryByText(/private|SECRET/)).toBeNull();
  });

  it("ignores a mismatched receipt and only renders the exact request provenance", async () => {
    vi.useFakeTimers();
    getWorkspaceTaskControlReceiptMock
      .mockResolvedValueOnce(receipt({ requestId: "other-request" }))
      .mockResolvedValueOnce(receipt());
    render(<WorkspaceTaskControlPanel />);
    await act(async () => {
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole("button", { name: "Build 시작" }));
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(screen.queryByText("Run Manager가 시작했습니다.")).toBeNull();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(500);
    });
    expect(screen.getAllByText("Run Manager가 시작했습니다.").length).toBeGreaterThan(0);
  });

  it("maps snapshot failures to fixed Korean text without echoing native details", async () => {
    listWorkspaceTaskControlsMock.mockRejectedValueOnce(new Error("C:\\private\\SECRET"));
    render(<WorkspaceTaskControlPanel />);

    expect(await screen.findByRole("alert")).toHaveTextContent("Run Manager task 요청을 완료하지 못했습니다.");
    expect(screen.queryByText(/private|SECRET/)).toBeNull();
  });

  it("clears receipt polling on unmount", async () => {
    vi.useFakeTimers();
    const rendered = render(<WorkspaceTaskControlPanel />);
    await act(async () => {
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole("button", { name: "Build 시작" }));
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    rendered.unmount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_000);
    });
    expect(getWorkspaceTaskControlReceiptMock).toHaveBeenCalledTimes(1);
  });
});
