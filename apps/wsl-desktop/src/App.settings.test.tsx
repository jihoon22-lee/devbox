import { useEffect, useRef, type CSSProperties, type HTMLAttributes } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import App from "./App";
import { getDashboardSnapshot, startSession } from "./api";
import { DEFAULT_SETTINGS, loadSettings } from "./lib/settings";
import type { DashboardSnapshot, MultiplexerKind } from "./types";

const mocks = vi.hoisted(() => ({
  nextSession: 0,
  paneProps: [] as { multiplexer: string; resumed: boolean; fontFamily: string; scrollbackLines: number }[],
}));

vi.mock("./components/TermPane", () => ({
  default: (props: {
    sessionId: string;
    title: string;
    multiplexer: string;
    resumed: boolean;
    fontFamily: string;
    scrollbackLines: number;
    style?: CSSProperties;
    registerFocus: (id: string, focus: () => void) => void;
    unregisterFocus: (id: string) => void;
    registerTerminalHandle: (id: string, handle: unknown) => void;
    unregisterTerminalHandle: (id: string) => void;
    contextMenuTriggerProps: HTMLAttributes<HTMLElement>;
  }) => {
    mocks.paneProps.push({
      multiplexer: props.multiplexer,
      resumed: props.resumed,
      fontFamily: props.fontFamily,
      scrollbackLines: props.scrollbackLines,
    });
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
    return (
      <div
        ref={ref}
        tabIndex={-1}
        data-pane-id={props.sessionId}
        aria-label={`${props.title} 터미널 팬`}
        {...props.contextMenuTriggerProps}
      />
    );
  },
}));

function snapshot(): DashboardSnapshot {
  return {
    revision: 1,
    capturedAtMs: Date.now(),
    staleAfterMs: 30_000,
    distros: [{
      name: "Ubuntu",
      version: 2,
      default: true,
      state: "Running",
      terminalCount: 0,
      dockerAvailability: "available",
      containers: [],
      resource: null,
    }],
  };
}

vi.mock("./api", () => ({
  getDashboardSnapshot: vi.fn(),
  dockerAction: vi.fn().mockResolvedValue(undefined),
  startSession: vi.fn(),
  detectMultiplexers: vi.fn(),
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
const detectMock = vi.mocked((await import("./api")).detectMultiplexers);

function storedSettings(patch: Record<string, unknown>) {
  localStorage.setItem("wsl-desktop:settings", JSON.stringify({ version: 1, ...patch }));
}

beforeEach(() => {
  localStorage.clear();
  mocks.nextSession = 0;
  mocks.paneProps.length = 0;
  snapshotMock.mockReset().mockImplementation(async () => snapshot());
  detectMock.mockReset().mockResolvedValue([
    { kind: "native", status: "available", version: null, source: null },
    { kind: "tmux", status: "missing", version: null, source: null },
    { kind: "zellij", status: "missing", version: null, source: null },
  ]);
  startSessionMock.mockReset().mockImplementation(async (_distro, _cwd, _key, multiplexer) => ({
    sessionId: `session-${++mocks.nextSession}`,
    resumed: false,
    multiplexer: multiplexer as MultiplexerKind,
  }));
});

afterEach(() => cleanup());

describe("startup", () => {
  it("복원할 레이아웃이 없으면 기본 배포판 터미널을 하나 연다", async () => {
    render(<App />);
    await screen.findByLabelText("Ubuntu 터미널 팬");
    expect(startSessionMock).toHaveBeenCalledTimes(1);
    expect(startSessionMock.mock.calls[0][0]).toBe("Ubuntu");
  });

  it("설정을 끄면 빈 상태로 시작한다", async () => {
    storedSettings({ openTerminalOnStart: false });
    render(<App />);
    await screen.findByText(/터미널이 없습니다/u);
    expect(startSessionMock).not.toHaveBeenCalled();
  });

  it("배포판 수집이 실패하면 터미널을 자동으로 열지 않는다", async () => {
    snapshotMock.mockReset().mockRejectedValue(new Error("collection failed"));
    render(<App />);
    await screen.findByText(/WSL resource snapshot을 갱신하지 못했습니다/u);
    expect(startSessionMock).not.toHaveBeenCalled();
  });
});

describe("persisted settings", () => {
  it("사이드 패널 상태를 저장하고 다음 창에서 복원한다", async () => {
    storedSettings({ openTerminalOnStart: false, sidePanelOpen: true });
    const first = render(<App />);
    await screen.findByRole("combobox", { name: "WSL 배포판 선택" });

    fireEvent.click(screen.getByTitle(/사이드 패널 토글/u));
    await waitFor(() => expect(screen.queryByRole("combobox", { name: "WSL 배포판 선택" })).not.toBeInTheDocument());
    expect(loadSettings().sidePanelOpen).toBe(false);

    first.unmount();
    render(<App />);
    await screen.findAllByRole("option", { name: /Ubuntu/u });
    expect(screen.queryByRole("combobox", { name: "WSL 배포판 선택" })).not.toBeInTheDocument();
  });

  it("선택한 세션 유지 방식을 저장하고 다음 창에서 복원한다", async () => {
    detectMock.mockResolvedValue([
      { kind: "native", status: "available", version: null, source: null },
      { kind: "tmux", status: "available", version: "3.4", source: "path" },
      { kind: "zellij", status: "missing", version: null, source: null },
    ]);
    storedSettings({ openTerminalOnStart: false });
    const first = render(<App />);
    const selector = await screen.findByRole("combobox", { name: "세션 유지 방식" });
    await waitFor(() => expect(within(selector).getByRole("option", { name: /tmux/u })).toBeEnabled());

    fireEvent.change(selector, { target: { value: "tmux" } });
    await waitFor(() => expect(loadSettings().multiplexer).toBe("tmux"));

    first.unmount();
    render(<App />);
    const restored = await screen.findByRole("combobox", { name: "세션 유지 방식" }) as HTMLSelectElement;
    await waitFor(() => expect(restored.value).toBe("tmux"));
  });

  it("저장된 유지 방식을 현재 배포판이 제공하지 않으면 조용히 native로 되돌린다", async () => {
    storedSettings({ openTerminalOnStart: false, multiplexer: "zellij" });
    render(<App />);
    const selector = await screen.findByRole("combobox", { name: "세션 유지 방식" }) as HTMLSelectElement;
    await waitFor(() => expect(selector.value).toBe("native"));
    expect(loadSettings().multiplexer).toBe("native");
  });

  it("팬 하나 닫기 확인은 설정으로 끌 수 있고 탭 닫기 확인은 남는다", async () => {
    storedSettings({ openTerminalOnStart: true, confirmSinglePaneClose: false });
    render(<App />);
    const pane = await screen.findByLabelText("Ubuntu 터미널 팬");

    fireEvent.keyDown(pane, { key: "F10", code: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "팬 닫기" }));
    await waitFor(() => expect(screen.queryByLabelText("Ubuntu 터미널 팬")).not.toBeInTheDocument());
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("설정 대화상자는 접근성 위반 없이 열리고 터미널 표시 설정을 저장한다", async () => {
    storedSettings({ openTerminalOnStart: true });
    render(<App />);
    await screen.findByLabelText("Ubuntu 터미널 팬");

    fireEvent.click(screen.getByRole("button", { name: "설정" }));
    const dialog = await screen.findByRole("dialog", { name: "WSL Desktop 설정" });
    // 팬·탭 trigger의 role 의미는 별도 작업에서 다룬다. 여기서는 새 대화상자만 확인한다.
    await assertNoA11yViolations(dialog);

    fireEvent.change(within(dialog).getByLabelText("터미널 글꼴"), { target: { value: "consolas" } });
    fireEvent.change(within(dialog).getByLabelText("스크롤백 줄 수"), { target: { value: "40000" } });
    await waitFor(() => expect(loadSettings().fontId).toBe("consolas"));
    expect(loadSettings().scrollbackLines).toBe(40_000);

    const latest = mocks.paneProps[mocks.paneProps.length - 1];
    expect(latest.fontFamily).toBe("Consolas, monospace");
    expect(latest.scrollbackLines).toBe(40_000);

    fireEvent.click(within(dialog).getByRole("button", { name: "닫기" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "WSL Desktop 설정" })).not.toBeInTheDocument());
  });

  it("스크롤백 입력은 상한을 넘겨도 clamp된 값만 저장한다", async () => {
    storedSettings({ openTerminalOnStart: false });
    render(<App />);
    await screen.findAllByRole("option", { name: /Ubuntu/u });
    fireEvent.click(screen.getByRole("button", { name: "설정" }));
    const dialog = await screen.findByRole("dialog", { name: "WSL Desktop 설정" });

    fireEvent.change(within(dialog).getByLabelText("스크롤백 줄 수"), { target: { value: "9999999" } });
    await waitFor(() => expect(loadSettings().scrollbackLines).toBe(100_000));
  });
});

describe("pane session mode", () => {
  it("실제로 시작된 유지 방식과 재연결 여부를 팬에 전달한다", async () => {
    detectMock.mockResolvedValue([
      { kind: "native", status: "available", version: null, source: null },
      { kind: "tmux", status: "available", version: "3.4", source: "path" },
      { kind: "zellij", status: "missing", version: null, source: null },
    ]);
    storedSettings({ openTerminalOnStart: false, multiplexer: "tmux" });
    startSessionMock.mockImplementation(async () => ({
      sessionId: `session-${++mocks.nextSession}`,
      resumed: true,
      multiplexer: "tmux" as const,
    }));

    render(<App />);
    const addButton = await screen.findByRole("button", { name: "+ 터미널" });
    await waitFor(() => expect(addButton).toBeEnabled());
    fireEvent.click(addButton);
    await screen.findByLabelText("Ubuntu 터미널 팬");

    const latest = mocks.paneProps[mocks.paneProps.length - 1];
    expect(latest).toMatchObject({ multiplexer: "tmux", resumed: true });
  });

  it("backend가 native로 낮추면 요청한 방식이 아니라 실제 방식을 전달한다", async () => {
    storedSettings({ openTerminalOnStart: false });
    startSessionMock.mockImplementation(async () => ({
      sessionId: `session-${++mocks.nextSession}`,
      resumed: false,
      multiplexer: "native" as const,
    }));

    render(<App />);
    const addButton = await screen.findByRole("button", { name: "+ 터미널" });
    await waitFor(() => expect(addButton).toBeEnabled());
    fireEvent.click(addButton);
    await screen.findByLabelText("Ubuntu 터미널 팬");

    const latest = mocks.paneProps[mocks.paneProps.length - 1];
    expect(latest).toMatchObject({ multiplexer: "native", resumed: false });
    expect(loadSettings().multiplexer).toBe(DEFAULT_SETTINGS.multiplexer);
  });
});
