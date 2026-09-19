import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  dockerAction,
  getDashboardSnapshot,
  getWindowsBuildNumber,
  listWorkspaceProfiles,
  onOpenRequest,
  onTerminalClosed,
  onTerminalOutput,
  openWslJournalInLogLens,
  startSession,
  takePendingOpen,
} from "./api";
import type { OpenRequest, WorkspaceProfile } from "./types";

const mocks = vi.hoisted(() => ({
  openHandler: null as ((request: OpenRequest) => void) | null,
  order: [] as string[],
  paneCanvasProps: null as unknown,
}));

vi.mock("./components/PaneCanvas", () => ({
  default: (props: unknown) => {
    mocks.paneCanvasProps = props;
    return <div data-testid="pane-canvas" />;
  },
}));
vi.mock("./api", () => ({
  configureQuickSummon: vi.fn().mockResolvedValue({
    shortcutRegistered: true,
    activeShortcut: "Ctrl+Alt+Space",
    trayEnabled: false,
    closeBehavior: "exit",
    issues: [],
  }),
  getDashboardSnapshot: vi.fn().mockResolvedValue({
    revision: 1,
    capturedAtMs: Date.now(),
    staleAfterMs: 30_000,
    distros: [
      {
        name: "Ubuntu",
        version: 2,
        default: true,
        state: "Running",
        terminalCount: 0,
        dockerAvailability: "available",
        containers: [],
        resource: {
          cpuPercent: 10,
          memoryUsedBytes: 1,
          memoryTotalBytes: 2,
          diskUsedBytes: 1,
          diskTotalBytes: 2,
        },
      },
    ],
  }),
  listDistros: vi.fn().mockResolvedValue([
    { name: "Ubuntu", version: 2, default: true, state: "Running" },
  ]),
  dockerPs: vi.fn().mockResolvedValue([]),
  dockerAction: vi.fn().mockResolvedValue(undefined),
  startSession: vi.fn().mockResolvedValue({ sessionId: "session-1", resumed: false, multiplexer: "native" }),
  detectMultiplexers: vi.fn().mockResolvedValue([
    { kind: "native", status: "available", version: null, source: null },
    { kind: "tmux", status: "missing", version: null, source: null },
    { kind: "zellij", status: "missing", version: null, source: null },
  ]),
  listWorkspaceProfiles: vi.fn().mockResolvedValue([]),
  saveWorkspaceProfile: vi.fn(),
  deleteWorkspaceProfile: vi.fn(),
  closeSession: vi.fn().mockResolvedValue(undefined),
  onTerminalClosed: vi.fn().mockResolvedValue(() => undefined),
  onTerminalOutput: vi.fn().mockResolvedValue(() => undefined),
  getWindowsBuildNumber: vi.fn().mockResolvedValue(null),
  openWslFileInLogLens: vi.fn().mockResolvedValue(undefined),
  openWslJournalInLogLens: vi.fn().mockResolvedValue(undefined),
  takePendingOpen: vi.fn().mockImplementation(async () => {
    mocks.order.push("take");
    return null;
  }),
  onOpenRequest: vi.fn().mockImplementation(async (handler: (request: OpenRequest) => void) => {
    mocks.order.push("listen");
    mocks.openHandler = handler;
    return () => undefined;
  }),
}));

const startSessionMock = vi.mocked(startSession);
const onOpenRequestMock = vi.mocked(onOpenRequest);
const takePendingOpenMock = vi.mocked(takePendingOpen);
const getWindowsBuildNumberMock = vi.mocked(getWindowsBuildNumber);
const listWorkspaceProfilesMock = vi.mocked(listWorkspaceProfiles);
const dockerActionMock = vi.mocked(dockerAction);
const getDashboardSnapshotMock = vi.mocked(getDashboardSnapshot);
const openWslJournalInLogLensMock = vi.mocked(openWslJournalInLogLens);
const onTerminalClosedMock = vi.mocked(onTerminalClosed);
const onTerminalOutputMock = vi.mocked(onTerminalOutput);

const profile: WorkspaceProfile = {
  id: "profile-1",
  name: "개발",
  tabs: [{
    id: "tab-1",
    title: "dev",
    customTitle: true,
    layout: "cols",
    paneKeys: ["pane-1", "pane-2"],
    sizing: { columns: [0.65, 0.35], rows: [1] },
  }],
  panes: [
    { key: "pane-1", distro: "Ubuntu", cwd: "/mnt/e/projects/devbox", startCommand: null, multiplexer: "native" },
    { key: "pane-2", distro: "Ubuntu", cwd: "/mnt/e/projects/devbox", startCommand: null, multiplexer: "tmux" },
  ],
  activeTabId: "tab-1",
  activePaneKey: "pane-2",
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  localStorage.clear();
  // 이 스위트는 시작 시 자동으로 열리는 터미널이 아니라 명시적으로 연 터미널을 검증한다.
  localStorage.setItem("wsl-desktop:settings", JSON.stringify({ version: 1, openTerminalOnStart: false }));
  mocks.openHandler = null;
  mocks.order.length = 0;
  mocks.paneCanvasProps = null;
  startSessionMock.mockReset().mockImplementation(async (_distro, _cwd, paneKey, requestedMultiplexer) => ({
    sessionId: `session-${paneKey}`,
    resumed: requestedMultiplexer !== "native",
    multiplexer: requestedMultiplexer,
  }));
  onOpenRequestMock.mockClear();
  takePendingOpenMock.mockClear();
  getWindowsBuildNumberMock.mockClear();
  listWorkspaceProfilesMock.mockReset().mockResolvedValue([]);
  dockerActionMock.mockReset().mockResolvedValue(undefined);
  getDashboardSnapshotMock.mockClear();
  openWslJournalInLogLensMock.mockReset().mockResolvedValue(undefined);
  onTerminalClosedMock.mockReset().mockResolvedValue(() => undefined);
  onTerminalOutputMock.mockReset().mockResolvedValue(() => undefined);

});

/** 앱 내장 대화상자를 승인한다. 취소가 첫 버튼, 확인이 마지막 버튼이다. */
async function acceptDialog(): Promise<void> {
  const dialog = await screen.findByRole("alertdialog");
  const buttons = within(dialog).getAllByRole("button");
  fireEvent.click(buttons[buttons.length - 1]);
  await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
}

afterEach(() => cleanup());

describe("App app-link delivery", () => {
  it("waits for the Windows build lookup before mounting PaneCanvas", async () => {
    let resolveBuild: ((value: number | null) => void) | undefined;
    getWindowsBuildNumberMock.mockImplementationOnce(
      () => new Promise<number | null>((resolve) => {
        resolveBuild = resolve;
      }),
    );

    render(<App />);
    expect(mocks.paneCanvasProps).toBeNull();

    await act(async () => resolveBuild?.(22631));
    await waitFor(() => expect(mocks.paneCanvasProps).not.toBeNull());
    expect((mocks.paneCanvasProps as { windowsBuildNumber: number }).windowsBuildNumber).toBe(22631);
  });

  it("releases terminal listeners that finish registering after unmount", async () => {
    const outputRegistration = deferred<() => void>();
    const closedRegistration = deferred<() => void>();
    const stopOutput = vi.fn();
    const stopClosed = vi.fn();
    onTerminalOutputMock.mockReturnValueOnce(outputRegistration.promise);
    onTerminalClosedMock.mockReturnValueOnce(closedRegistration.promise);
    const view = render(<App />);
    await waitFor(() => expect(onTerminalOutputMock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(onTerminalClosedMock).toHaveBeenCalledTimes(1));

    view.unmount();
    await act(async () => {
      outputRegistration.resolve(stopOutput);
      closedRegistration.resolve(stopClosed);
      await Promise.all([outputRegistration.promise, closedRegistration.promise]);
    });

    expect(stopOutput).toHaveBeenCalledTimes(1);
    expect(stopClosed).toHaveBeenCalledTimes(1);
  });

  it("listens before the initial take and consumes the pending slot on relaunch", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));
    expect(mocks.order.slice(0, 2)).toEqual(["listen", "take"]);

    takePendingOpenMock.mockResolvedValueOnce({
      target: { kind: "path", path: "/mnt/e/projects/devbox", line: null, column: null },
      from: "workbench",
    });
    await act(async () => {
      mocks.openHandler?.({ target: { kind: "query", text: "stale" }, from: "test" });
    });

    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(startSessionMock).toHaveBeenCalledWith("Ubuntu", "/mnt/e/projects/devbox", expect.any(String), "native"),
    );
  });

  it("consumes a cold-start request when app-link listener registration fails", async () => {
    onOpenRequestMock.mockRejectedValueOnce(new Error("listener unavailable"));
    takePendingOpenMock.mockResolvedValueOnce({
      target: { kind: "path", path: "/mnt/e/projects/devbox", line: null, column: null },
      from: "workbench",
    });

    render(<App />);

    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(startSessionMock).toHaveBeenCalledWith("Ubuntu", "/mnt/e/projects/devbox", expect.any(String), "native"),
    );
  });

  it("cold-start profile target restores stable pane keys and layout", async () => {
    listWorkspaceProfilesMock.mockResolvedValueOnce([profile]);
    takePendingOpenMock.mockResolvedValueOnce({ target: { kind: "profile", id: profile.id }, from: "devbox-launcher" });

    render(<App />);

    await waitFor(() => expect(startSessionMock).toHaveBeenCalledTimes(2));
    expect(startSessionMock).toHaveBeenNthCalledWith(1, "Ubuntu", "/mnt/e/projects/devbox", "pane-2", "tmux");
    expect(startSessionMock).toHaveBeenNthCalledWith(2, "Ubuntu", "/mnt/e/projects/devbox", "pane-1", "native");
    await waitFor(() => expect((mocks.paneCanvasProps as { activePaneId: string }).activePaneId).toBe("session-pane-2"));
    expect((mocks.paneCanvasProps as { tabs: Array<{ sizing: unknown }> }).tabs[0].sizing)
      .toEqual({ columns: [0.65, 0.35], rows: [1] });
  });

  it("waits for the active pane before starting any remaining restore work", async () => {
    const activeStart = deferred<Awaited<ReturnType<typeof startSession>>>();
    listWorkspaceProfilesMock.mockResolvedValueOnce([profile]);
    takePendingOpenMock.mockResolvedValueOnce({ target: { kind: "profile", id: profile.id }, from: "devbox-launcher" });
    startSessionMock.mockImplementation(async (_distro, _cwd, paneKey, requestedMultiplexer) => {
      if (paneKey === "pane-2") return activeStart.promise;
      return { sessionId: `session-${paneKey}`, resumed: false, multiplexer: requestedMultiplexer };
    });

    render(<App />);

    await waitFor(() => expect(startSessionMock).toHaveBeenCalledTimes(1));
    expect(startSessionMock.mock.calls[0]?.[2]).toBe("pane-2");
    expect((mocks.paneCanvasProps as {
      activePaneId: string;
      panes: Array<{ sessionId: string | null; restoreStatus?: string }>;
    })).toMatchObject({
      activePaneId: "pane-2",
      panes: [
        { sessionId: null, restoreStatus: "connecting" },
        { sessionId: null, restoreStatus: "connecting" },
      ],
    });
    await act(async () => {
      activeStart.resolve({ sessionId: "session-pane-2", resumed: false, multiplexer: "tmux" });
      await activeStart.promise;
    });
    await waitFor(() => expect(startSessionMock).toHaveBeenCalledTimes(2));
    expect(startSessionMock.mock.calls[1]?.[2]).toBe("pane-1");
  });

  it("hot profile target consumes the pending slot and follows the same restore path", async () => {
    listWorkspaceProfilesMock.mockResolvedValueOnce([profile]);
    render(<App />);
    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));
    takePendingOpenMock.mockResolvedValueOnce({ target: { kind: "profile", id: profile.id }, from: "devbox-launcher" });

    await act(async () => {
      mocks.openHandler?.({ target: { kind: "profile", id: profile.id }, from: "devbox-launcher" });
    });

    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(startSessionMock).toHaveBeenCalledTimes(2));
    expect(startSessionMock.mock.calls.map((call) => call[2])).toEqual(["pane-2", "pane-1"]);
  });

  it("profile 일부 세션 시작 실패 시 원래 자리와 active identity를 보존하고 재시도한다", async () => {
    listWorkspaceProfilesMock.mockResolvedValueOnce([profile]);
    takePendingOpenMock.mockResolvedValueOnce({ target: { kind: "profile", id: profile.id }, from: "devbox-launcher" });
    startSessionMock.mockImplementation(async (_distro, _cwd, paneKey, requestedMultiplexer) => {
      if (paneKey === "pane-2") throw new Error("fixture failure");
      return { sessionId: `session-${paneKey}`, resumed: false, multiplexer: requestedMultiplexer };
    });

    render(<App />);

    await waitFor(() => expect(startSessionMock).toHaveBeenCalledTimes(2));
    await waitFor(() => {
      const props = mocks.paneCanvasProps as {
        activePaneId: string;
        panes: Array<{ key: string; sessionId: string | null; restoreStatus?: string }>;
      };
      expect(props.panes).toHaveLength(2);
      expect(props.panes.find((pane) => pane.key === "pane-2")).toMatchObject({
        sessionId: null,
        restoreStatus: "failed",
      });
      expect(props.activePaneId).toBe("pane-2");
    });

    startSessionMock.mockImplementation(async (_distro, _cwd, paneKey, requestedMultiplexer) => ({
      sessionId: `retry-${paneKey}`,
      resumed: false,
      multiplexer: requestedMultiplexer,
    }));
    await act(async () => {
      await (mocks.paneCanvasProps as { onRetryPane: (key: string) => Promise<void> }).onRetryPane("pane-2");
    });
    await waitFor(() => expect(
      (mocks.paneCanvasProps as { activePaneId: string }).activePaneId,
    ).toBe("retry-pane-2"));
  });

  it("isolates a pending Log Lens handoff from Docker, distro, and new terminal actions", async () => {
    let resolveHandoff: (() => void) | undefined;
    openWslJournalInLogLensMock.mockImplementationOnce(
      () => new Promise<void>((resolve) => {
        resolveHandoff = resolve;
      }),
    );
    getDashboardSnapshotMock.mockResolvedValueOnce({
      revision: 2,
      capturedAtMs: Date.now(),
      staleAfterMs: 30_000,
      distros: [{
        name: "Ubuntu",
        version: 2,
        default: true,
        state: "Running",
        terminalCount: 0,
        dockerAvailability: "available",
        containers: [
          { id: "container-1", name: "worker", image: "worker:latest", status: "Exited (1)", ports: "" },
        ],
        resource: {
          cpuPercent: 10,
          memoryUsedBytes: 1,
          memoryTotalBytes: 2,
          diskUsedBytes: 1,
          diskTotalBytes: 2,
        },
      }],
    });

    render(<App />);
    const journalButton = await screen.findByRole("button", { name: "Log Lens에서 저널 열기" });
    const startButton = await screen.findByRole("button", { name: "시작" });
    const addTerminalButton = await screen.findByRole("button", { name: "+ 터미널" });

    fireEvent.click(journalButton);
    await acceptDialog();
    await waitFor(() => expect(openWslJournalInLogLensMock).toHaveBeenCalledWith("Ubuntu", null));
    expect(journalButton).toBeDisabled();
    expect(startButton).toBeDisabled();
    expect(addTerminalButton).toBeDisabled();
    // 배포판 선택기는 툴바 하나로 합쳐졌다. 패널은 카드로만 배포판을 다룬다.
    expect(screen.getByRole("combobox", { name: "현재 WSL 배포판" })).toBeDisabled();
    expect(screen.queryByRole("combobox", { name: "WSL 배포판 선택" })).not.toBeInTheDocument();

    fireEvent.click(startButton);
    expect(dockerActionMock).not.toHaveBeenCalled();
    await act(async () => resolveHandoff?.());
    await waitFor(() => expect(journalButton).toBeEnabled());
    expect(startButton).toBeEnabled();
    expect(addTerminalButton).toBeEnabled();
  });

  it("clears a failed handoff error after a later successful handoff", async () => {
    openWslJournalInLogLensMock
      .mockRejectedValueOnce(new Error("native path /secret/run.log"))
      .mockResolvedValueOnce(undefined);

    render(<App />);
    const journalButton = await screen.findByRole("button", { name: "Log Lens에서 저널 열기" });
    fireEvent.click(journalButton);
    await acceptDialog();
    await screen.findByText("Log Lens journal handoff를 시작하지 못했습니다.");

    fireEvent.click(journalButton);
    await acceptDialog();
    await waitFor(() => expect(openWslJournalInLogLensMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByText("Log Lens journal handoff를 시작하지 못했습니다.")).not.toBeInTheDocument());
  });
});
