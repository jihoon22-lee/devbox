import { useEffect, useRef, type CSSProperties, type HTMLAttributes } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { dockerAction, getDashboardSnapshot, startSession } from "./api";
import type { DashboardSnapshot } from "./types";

const mocks = vi.hoisted(() => ({ nextSession: 0 }));

vi.mock("./components/TermPane", () => ({
  default: (props: {
    sessionId: string;
    title: string;
    isFocusedPane: boolean;
    style?: CSSProperties;
    registerFocus: (id: string, focus: () => void) => void;
    unregisterFocus: (id: string) => void;
    registerTerminalHandle: (id: string, handle: unknown) => void;
    unregisterTerminalHandle: (id: string) => void;
    contextMenuTriggerProps: HTMLAttributes<HTMLElement>;
  }) => {
    const ref = useRef<HTMLDivElement>(null);
    useEffect(() => {
      props.registerFocus(props.sessionId, () => undefined);
      props.registerTerminalHandle(props.sessionId, {
        getCapabilities: () => ({ hasSelection: false, hasCwd: false }),
        copySelection: async () => undefined,
        pasteClipboard: async () => undefined,
        openSearch: () => undefined,
        copyCwd: async () => undefined,
      });
      return () => {
        props.unregisterFocus(props.sessionId);
        props.unregisterTerminalHandle(props.sessionId);
      };
    }, [props]);
    return <div ref={ref} data-pane-id={props.sessionId} aria-label={`${props.title} 터미널 팬`} />;
  },
}));

function snapshot(capturedAtMs = Date.now()): DashboardSnapshot {
  return {
    revision: 1,
    capturedAtMs,
    staleAfterMs: 30_000,
    distros: [{
      name: "Ubuntu",
      version: 2,
      default: true,
      state: "Running",
      terminalCount: 0,
      dockerAvailability: "available",
      containers: [{ id: "abc123", name: "api", image: "api:latest", status: "Created", ports: "" }],
      resource: null,
    }],
  };
}

vi.mock("./api", () => ({
  configureQuickSummon: vi.fn().mockResolvedValue({
    shortcutRegistered: true,
    activeShortcut: "Ctrl+Alt+Space",
    trayEnabled: false,
    closeBehavior: "exit",
    issues: [],
  }),
  getDashboardSnapshot: vi.fn(),
  dockerAction: vi.fn().mockResolvedValue(undefined),
  startSession: vi.fn(),
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
  takePendingOpen: vi.fn().mockResolvedValue(null),
  onOpenRequest: vi.fn().mockResolvedValue(() => undefined),
}));

const snapshotMock = vi.mocked(getDashboardSnapshot);
const startSessionMock = vi.mocked(startSession);
const dockerActionMock = vi.mocked(dockerAction);

async function armBroadcast(): Promise<HTMLInputElement> {
  render(<App />);
  await screen.findAllByRole("option", { name: /Ubuntu/u });
  const addButton = screen.getByRole("button", { name: "+ 터미널" });
  await waitFor(() => expect(addButton).toBeEnabled());
  fireEvent.click(addButton);
  await screen.findByLabelText("Ubuntu 터미널 팬");
  fireEvent.click(addButton);
  await waitFor(() => expect(screen.getAllByLabelText("Ubuntu 터미널 팬")).toHaveLength(2));

  fireEvent.click(screen.getByRole("button", { name: /동시 입력 대상 선택/u }));
  const picker = screen.getByRole("group", { name: "동시 입력 대상 팬 선택" });
  for (const target of within(picker).getAllByRole("checkbox")) {
    await waitFor(() => expect(target).toBeEnabled());
    fireEvent.click(target);
  }
  const toggle = screen.getByLabelText("동시 입력 활성화") as HTMLInputElement;
  await waitFor(() => expect(toggle).toBeEnabled());
  fireEvent.click(toggle);
  expect(toggle.checked).toBe(true);
  return toggle;
}

beforeEach(() => {
  localStorage.clear();
  // 이 스위트는 시작 시 자동으로 열리는 터미널이 아니라 명시적으로 연 터미널을 검증한다.
  localStorage.setItem("wsl-desktop:settings", JSON.stringify({ version: 1, openTerminalOnStart: false }));
  mocks.nextSession = 0;
  snapshotMock.mockReset().mockImplementation(async () => snapshot());
  dockerActionMock.mockReset().mockResolvedValue(undefined);
  startSessionMock.mockReset().mockImplementation(async () => ({
    sessionId: `session-${++mocks.nextSession}`,
    resumed: false,
    multiplexer: "native" as const,
  }));
});

afterEach(() => cleanup());

describe("snapshot-gated controls", () => {
  it("동시 입력은 진행 중인 refresh를 넘겨 유지된다", async () => {
    const toggle = await armBroadcast();

    let release: (() => void) | undefined;
    snapshotMock.mockImplementationOnce(
      () => new Promise((resolve) => { release = () => resolve(snapshot()); }),
    );
    fireEvent.click(screen.getByRole("button", { name: "새로고침" }));

    await screen.findByText("새로 고치는 중…");
    expect(toggle.checked).toBe(true);
    expect(toggle).toBeEnabled();

    release?.();
    await waitFor(() => expect(screen.getByText("최신 snapshot")).toBeInTheDocument());
    expect(toggle.checked).toBe(true);
  });

  it("수집이 실패하면 동시 입력은 계속 fail-closed된다", async () => {
    const toggle = await armBroadcast();

    snapshotMock.mockRejectedValueOnce(new Error("collection failed"));
    fireEvent.click(screen.getByRole("button", { name: "새로고침" }));

    await waitFor(() => expect(toggle.checked).toBe(false));
    expect(toggle).toBeDisabled();
  });

  it("대상 팬이 사라지면 동시 입력은 계속 fail-closed된다", async () => {
    const toggle = await armBroadcast();

    fireEvent.click(screen.getByTitle("새 탭 (Ctrl+Shift+T)"));
    await waitFor(() => expect(toggle.checked).toBe(false));
  });

  it("Docker 조작은 refresh 중에도 마지막 정상 snapshot으로 계속 쓸 수 있다", async () => {
    render(<App />);
    const startButton = await screen.findByRole("button", { name: "시작" });
    await waitFor(() => expect(startButton).toBeEnabled());

    let release: (() => void) | undefined;
    snapshotMock.mockImplementationOnce(
      () => new Promise((resolve) => { release = () => resolve(snapshot()); }),
    );
    fireEvent.click(screen.getByRole("button", { name: "새로고침" }));
    await screen.findByText("새로 고치는 중…");

    expect(startButton).toBeEnabled();
    fireEvent.click(startButton);
    await waitFor(() => expect(dockerActionMock).toHaveBeenCalledWith("Ubuntu", "abc123", "start"));
    release?.();
  });

  it("만료된 snapshot에서는 Docker 조작과 동시 입력을 모두 막는다", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      snapshotMock.mockImplementation(async () => snapshot(Date.now()));
      render(<App />);
      const startButton = await screen.findByRole("button", { name: "시작" });
      await waitFor(() => expect(startButton).toBeEnabled());

      // Hold every later collection open so only the TTL, not a failure, closes the gate.
      snapshotMock.mockImplementation(() => new Promise(() => undefined));
      await vi.advanceTimersByTimeAsync(31_000);

      await waitFor(() => expect(screen.getByRole("button", { name: "시작" })).toBeDisabled());
      expect(screen.getByLabelText("동시 입력 활성화")).toBeDisabled();
    } finally {
      vi.useRealTimers();
    }
  });
});
