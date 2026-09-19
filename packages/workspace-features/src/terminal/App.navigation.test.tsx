import { useEffect, useRef, type CSSProperties, type HTMLAttributes } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import App from "./App";
import { getDashboardSnapshot, startSession } from "./api";
import type { DashboardSnapshot } from "./types";

const mocks = vi.hoisted(() => ({
  nextSession: 0,
  broadcastTargets: new Map<string, boolean>(),
}));

vi.mock("./components/TermPane", () => ({
  default: (props: {
    sessionId: string;
    title: string;
    isFocusedPane: boolean;
    isBroadcastTarget: boolean;
    style?: CSSProperties;
    registerFocus: (id: string, focus: () => void) => void;
    unregisterFocus: (id: string) => void;
    registerTerminalHandle: (id: string, handle: unknown) => void;
    unregisterTerminalHandle: (id: string) => void;
    contextMenuTriggerProps: HTMLAttributes<HTMLElement>;
  }) => {
    mocks.broadcastTargets.set(props.sessionId, props.isBroadcastTarget);
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
        role="group"
        tabIndex={-1}
        data-pane-id={props.sessionId}
        data-focused={String(props.isFocusedPane)}
        data-broadcast-target={String(props.isBroadcastTarget)}
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

async function renderWithPanes(count: number, layout?: "cols" | "rows") {
  render(<App />);
  await screen.findAllByRole("option", { name: /Ubuntu/u });
  const addButton = screen.getByRole("button", { name: "+ 터미널" });
  await waitFor(() => expect(addButton).toBeEnabled());
  for (let index = 0; index < count; index += 1) {
    fireEvent.click(addButton);
    await waitFor(() => expect(screen.getAllByLabelText(/터미널 팬/u)).toHaveLength(index + 1));
  }
  if (layout) {
    fireEvent.change(screen.getByRole("combobox", { name: "탭 레이아웃" }), { target: { value: layout } });
  }
}

/**
 * 지금 focus된 팬을 DOM에서 직접 읽는다. 렌더 기록의 마지막 항목을 보는 방식은 렌더가
 * 몇 번 어떤 순서로 커밋되는지에 좌우돼 느린 실행기에서 거짓 결과를 낸다.
 */
function focusedPane(): string | null {
  return document.querySelector("[data-focused='true']")?.getAttribute("data-pane-id") ?? null;
}

async function expectFocusedPane(sessionId: string): Promise<void> {
  await waitFor(() => expect(focusedPane()).toBe(sessionId));
}

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem("wsl-desktop:settings", JSON.stringify({ version: 1, openTerminalOnStart: false }));
  mocks.nextSession = 0;
  mocks.broadcastTargets.clear();
  snapshotMock.mockReset().mockImplementation(async () => snapshot());
  startSessionMock.mockReset().mockImplementation(async () => ({
    sessionId: `session-${++mocks.nextSession}`,
    resumed: false,
    multiplexer: "native" as const,
  }));
});

afterEach(() => cleanup());

describe("directional pane focus", () => {
  it("2×2 격자에서 Alt+방향키가 실제 이웃으로 이동한다", async () => {
    await renderWithPanes(4);
    // 팬 순서: 0 1 / 2 3. 마지막으로 추가한 네 번째 팬이 활성이다.
    await expectFocusedPane("session-4");

    fireEvent.keyDown(window, { key: "ArrowLeft", altKey: true });
    await expectFocusedPane("session-3");

    fireEvent.keyDown(window, { key: "ArrowUp", altKey: true });
    await expectFocusedPane("session-1");

    fireEvent.keyDown(window, { key: "ArrowRight", altKey: true });
    await expectFocusedPane("session-2");
  });

  it("격자 가장자리에서는 반대편으로 순환하지 않는다", async () => {
    await renderWithPanes(4);
    await expectFocusedPane("session-4");

    // 오른쪽 아래 팬에서 오른쪽·아래로는 갈 곳이 없다.
    fireEvent.keyDown(window, { key: "ArrowRight", altKey: true });
    fireEvent.keyDown(window, { key: "ArrowDown", altKey: true });
    expect(focusedPane()).toBe("session-4");
  });

  it("세로 분할에서는 위아래 이동이 팬을 바꾸지 않는다", async () => {
    await renderWithPanes(3, "cols");
    await expectFocusedPane("session-3");

    fireEvent.keyDown(window, { key: "ArrowUp", altKey: true });
    fireEvent.keyDown(window, { key: "ArrowDown", altKey: true });
    expect(focusedPane()).toBe("session-3");

    fireEvent.keyDown(window, { key: "ArrowLeft", altKey: true });
    await expectFocusedPane("session-2");
  });
});

describe("broadcast target visibility", () => {
  it("무장된 대상 팬만 표시를 받는다", async () => {
    await renderWithPanes(3);
    fireEvent.click(screen.getByRole("button", { name: /동시 입력 대상 선택/u }));
    const picker = screen.getByRole("group", { name: "동시 입력 대상 팬 선택" });
    const targets = within(picker).getAllByRole("checkbox");
    fireEvent.click(targets[0]);
    fireEvent.click(targets[1]);

    const toggle = screen.getByLabelText("동시 입력 활성화");
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);

    await waitFor(() => expect(mocks.broadcastTargets.get("session-1")).toBe(true));
    expect(mocks.broadcastTargets.get("session-2")).toBe(true);
    expect(mocks.broadcastTargets.get("session-3")).toBe(false);
  });

  it("동시 입력이 꺼지면 표시도 사라진다", async () => {
    await renderWithPanes(2);
    fireEvent.click(screen.getByRole("button", { name: /동시 입력 대상 선택/u }));
    const picker = screen.getByRole("group", { name: "동시 입력 대상 팬 선택" });
    for (const target of within(picker).getAllByRole("checkbox")) fireEvent.click(target);
    const toggle = screen.getByLabelText("동시 입력 활성화");
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    await waitFor(() => expect(mocks.broadcastTargets.get("session-1")).toBe(true));

    fireEvent.click(toggle);
    await waitFor(() => expect(mocks.broadcastTargets.get("session-1")).toBe(false));
  });
});

describe("tab and error affordances", () => {
  it("새 탭은 활성 팬의 cwd를 물려받는다", async () => {
    render(<App />);
    await screen.findAllByRole("option", { name: /Ubuntu/u });
    const addButton = screen.getByRole("button", { name: "+ 터미널" });
    await waitFor(() => expect(addButton).toBeEnabled());
    fireEvent.change(screen.getByPlaceholderText(/경로 열기/u), { target: { value: "/srv/app" } });
    fireEvent.click(addButton);
    await screen.findByLabelText(/터미널 팬/u);
    expect(startSessionMock.mock.calls[0][1]).toBe("/srv/app");

    fireEvent.click(screen.getByTitle("새 탭 (Ctrl+Shift+T)"));
    await waitFor(() => expect(startSessionMock).toHaveBeenCalledTimes(2));
    expect(startSessionMock.mock.calls[1][1]).toBe("/srv/app");
  });

  it("오류 배너는 직접 닫을 수 있다", async () => {
    render(<App />);
    await screen.findAllByRole("option", { name: /Ubuntu/u });
    const addButton = screen.getByRole("button", { name: "+ 터미널" });
    await waitFor(() => expect(addButton).toBeEnabled());
    startSessionMock.mockRejectedValueOnce(new Error("boom"));
    fireEvent.click(addButton);

    const banner = await screen.findByText("터미널을 시작하지 못했습니다.");
    fireEvent.click(screen.getByRole("button", { name: "오류 메시지 닫기" }));
    await waitFor(() => expect(banner).not.toBeInTheDocument());
  });

  it("단축키 안내는 실제 matcher 표를 그대로 보여 준다", async () => {
    render(<App />);
    await screen.findAllByRole("option", { name: /Ubuntu/u });
    fireEvent.click(screen.getByRole("button", { name: "단축키" }));

    const dialog = await screen.findByRole("dialog", { name: "키보드 단축키" });
    expect(within(dialog).getByText("Ctrl+Shift+T")).toBeInTheDocument();
    expect(within(dialog).getByText("Alt+→")).toBeInTheDocument();
    expect(within(dialog).getByText("Ctrl+Shift+F")).toBeInTheDocument();
    await assertNoA11yViolations(dialog);
  });

  it("탭과 팬이 있는 상태의 셸도 접근성 위반이 없다", async () => {
    const { container } = render(<App />);
    await screen.findAllByRole("option", { name: /Ubuntu/u });
    const addButton = screen.getByRole("button", { name: "+ 터미널" });
    await waitFor(() => expect(addButton).toBeEnabled());
    fireEvent.click(addButton);
    await screen.findByLabelText(/터미널 팬/u);
    fireEvent.click(screen.getByTitle("새 탭 (Ctrl+Shift+T)"));
    await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(2));

    await assertNoA11yViolations(container);
  });
});
