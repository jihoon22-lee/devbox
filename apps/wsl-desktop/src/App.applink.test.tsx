import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { getWindowsBuildNumber, listWorkspaceProfiles, onOpenRequest, startSession, takePendingOpen } from "./api";
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
    { kind: "native", available: true, version: null },
    { kind: "tmux", available: false, version: null },
    { kind: "zellij", available: false, version: null },
  ]),
  listWorkspaceProfiles: vi.fn().mockResolvedValue([]),
  saveWorkspaceProfile: vi.fn(),
  deleteWorkspaceProfile: vi.fn(),
  closeSession: vi.fn().mockResolvedValue(undefined),
  onTerminalClosed: vi.fn().mockResolvedValue(() => undefined),
  onTerminalOutput: vi.fn().mockResolvedValue(() => undefined),
  getWindowsBuildNumber: vi.fn().mockResolvedValue(null),
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

const profile: WorkspaceProfile = {
  id: "profile-1",
  name: "개발",
  tabs: [{ id: "tab-1", title: "dev", customTitle: true, layout: "cols", paneKeys: ["pane-1", "pane-2"] }],
  panes: [
    { key: "pane-1", distro: "Ubuntu", cwd: "/mnt/e/projects/devbox", startCommand: null, multiplexer: "native" },
    { key: "pane-2", distro: "Ubuntu", cwd: "/mnt/e/projects/devbox", startCommand: null, multiplexer: "tmux" },
  ],
  activeTabId: "tab-1",
  activePaneKey: "pane-2",
};

beforeEach(() => {
  localStorage.clear();
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
});

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
    expect(startSessionMock).toHaveBeenNthCalledWith(1, "Ubuntu", "/mnt/e/projects/devbox", "pane-1", "native");
    expect(startSessionMock).toHaveBeenNthCalledWith(2, "Ubuntu", "/mnt/e/projects/devbox", "pane-2", "tmux");
    await waitFor(() => expect((mocks.paneCanvasProps as { activePaneId: string }).activePaneId).toBe("session-pane-2"));
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
    expect(startSessionMock.mock.calls.map((call) => call[2])).toEqual(["pane-1", "pane-2"]);
  });

  it("profile 일부 세션 시작 실패 시 성공한 팬과 유효한 active identity를 보존한다", async () => {
    listWorkspaceProfilesMock.mockResolvedValueOnce([profile]);
    takePendingOpenMock.mockResolvedValueOnce({ target: { kind: "profile", id: profile.id }, from: "devbox-launcher" });
    startSessionMock.mockImplementation(async (_distro, _cwd, paneKey, requestedMultiplexer) => {
      if (paneKey === "pane-2") throw new Error("fixture failure");
      return { sessionId: `session-${paneKey}`, resumed: false, multiplexer: requestedMultiplexer };
    });

    render(<App />);

    await waitFor(() => expect(startSessionMock).toHaveBeenCalledTimes(2));
    await waitFor(() => {
      const props = mocks.paneCanvasProps as { activePaneId: string; panes: unknown[] };
      expect(props.panes).toHaveLength(1);
      expect(props.activePaneId).toBe("session-pane-1");
    });
  });
});
