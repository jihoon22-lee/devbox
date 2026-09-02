import { useEffect, useRef, type CSSProperties, type HTMLAttributes } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import App from "./App";
import { closeSession, startSession } from "./api";
import type { OpenRequest } from "./types";

const mocks = vi.hoisted(() => ({
  nextSession: 0,
  terminalFocus: vi.fn<(id: string) => void>(),
  copySelection: vi.fn<(id: string) => void>(),
  pasteClipboard: vi.fn<(id: string) => void>(),
  openSearch: vi.fn<(id: string) => void>(),
  copyCwd: vi.fn<(id: string) => void>(),
}));

vi.mock("./components/TermPane", () => ({
  default: (props: {
    sessionId: string;
    title: string;
    active: boolean;
    isFocusedPane: boolean;
    style?: CSSProperties;
    registerFocus: (id: string, focus: () => void) => void;
    unregisterFocus: (id: string) => void;
    registerTerminalHandle: (id: string, handle: {
      getCapabilities: () => { hasSelection: boolean; hasCwd: boolean };
      copySelection: () => Promise<void>;
      pasteClipboard: () => Promise<void>;
      openSearch: () => void;
      copyCwd: () => Promise<void>;
    }) => void;
    unregisterTerminalHandle: (id: string) => void;
    onMetadataChange: (id: string, metadata: { title?: string; cwd?: string }) => void;
    onClose: () => void;
    contextMenuTriggerProps: HTMLAttributes<HTMLElement>;
    actionsDisabled: boolean;
  }) => {
    const ref = useRef<HTMLDivElement>(null);
    useEffect(() => {
      props.registerFocus(props.sessionId, () => {
        mocks.terminalFocus(props.sessionId);
        ref.current?.focus();
      });
      props.registerTerminalHandle(props.sessionId, {
        getCapabilities: () => ({ hasSelection: true, hasCwd: true }),
        copySelection: async () => mocks.copySelection(props.sessionId),
        pasteClipboard: async () => mocks.pasteClipboard(props.sessionId),
        openSearch: () => mocks.openSearch(props.sessionId),
        copyCwd: async () => mocks.copyCwd(props.sessionId),
      });
      return () => {
        props.unregisterFocus(props.sessionId);
        props.unregisterTerminalHandle(props.sessionId);
      };
    }, [props.registerFocus, props.registerTerminalHandle, props.sessionId, props.unregisterFocus, props.unregisterTerminalHandle]);
    return (
      <div
        ref={ref}
        className={`pane ${props.isFocusedPane ? "pane-focused" : ""}`}
        style={props.style}
        tabIndex={-1}
        data-pane-id={props.sessionId}
        aria-label={`${props.title} 터미널 팬`}
        {...props.contextMenuTriggerProps}
      >
        <button title={`Close terminal ${props.sessionId}`} disabled={props.actionsDisabled} onClick={props.onClose}>
          close
        </button>
        <button
          title={`Emit title ${props.sessionId}`}
          onClick={() => props.onMetadataChange(props.sessionId, { title: "npm test" })}
        >title</button>
      </div>
    );
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
  startSession: vi.fn().mockImplementation(async () => ({
    sessionId: `session-${++mocks.nextSession}`,
    resumed: false,
    multiplexer: "native",
  })),
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
  onOpenRequest: vi.fn().mockImplementation(async (_handler: (request: OpenRequest) => void) => () => undefined),
}));

const startSessionMock = vi.mocked(startSession);
const closeSessionMock = vi.mocked(closeSession);

/** 앱 내장 대화상자는 취소가 첫 버튼, 확인이 마지막 버튼이다. */
async function openDialog(): Promise<HTMLElement> {
  return screen.findByRole("alertdialog");
}

async function answerDialog(confirmed: boolean, value?: string): Promise<void> {
  const dialog = await openDialog();
  if (value !== undefined) {
    fireEvent.change(within(dialog).getByRole("textbox"), { target: { value } });
  }
  const buttons = within(dialog).getAllByRole("button");
  fireEvent.click(confirmed ? buttons[buttons.length - 1] : buttons[0]);
  await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
}

async function renderWithPane(cwd = "") {
  render(<App />);
  await screen.findAllByRole("option", { name: /Ubuntu/u });
  if (cwd) fireEvent.change(screen.getByPlaceholderText(/경로 열기/u), { target: { value: cwd } });
  const addButton = screen.getByRole("button", { name: "+ 터미널" });
  await waitFor(() => expect(addButton).toBeEnabled());
  fireEvent.click(addButton);
  const pane = await screen.findByLabelText("Ubuntu 터미널 팬") as HTMLDivElement;
  const tab = await screen.findByLabelText("Ubuntu 터미널 탭") as HTMLDivElement;
  return { pane, tab };
}

beforeEach(() => {
  localStorage.clear();
  // 이 스위트는 시작 시 자동으로 열리는 터미널이 아니라 명시적으로 연 터미널을 검증한다.
  localStorage.setItem("wsl-desktop:settings", JSON.stringify({ version: 1, openTerminalOnStart: false }));
  mocks.nextSession = 0;
  mocks.terminalFocus.mockReset();
  mocks.copySelection.mockReset();
  mocks.pasteClipboard.mockReset();
  mocks.openSearch.mockReset();
  mocks.copyCwd.mockReset();
  startSessionMock.mockReset().mockImplementation(async () => ({
    sessionId: `session-${++mocks.nextSession}`,
    resumed: false,
    multiplexer: "native",
  }));
  closeSessionMock.mockReset().mockResolvedValue(undefined);
});

afterEach(() => cleanup());

describe("WSL Desktop pane and tab context menus", () => {
  it("초기 셸이 접근성 위반 없이 렌더링된다", async () => {
    const { container } = render(<App />);
    await screen.findAllByRole("option", { name: /Ubuntu/u });
    await waitFor(() => expect(screen.getByRole("button", { name: "+ 터미널" })).toBeEnabled());
    await assertNoA11yViolations(container);
  });

  it("우클릭한 exact pane의 현재 capability로 정확한 메뉴를 표시하고 action을 전달한다", async () => {
    const { pane } = await renderWithPane();

    fireEvent.contextMenu(pane, { clientX: 20, clientY: 24 });

    expect(pane.className).toContain("pane-focused");
    for (const label of ["복사", "붙여넣기", "검색", "세로 분할", "가로 분할", "cwd 복사", "팬 닫기"]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeInTheDocument();
    }
    for (const label of ["복사", "붙여넣기", "검색", "cwd 복사"]) {
      expect(screen.getByRole("menuitem", { name: label })).not.toHaveAttribute("aria-disabled", "true");
    }
    expect(screen.getByRole("menuitem", { name: "팬 닫기" })).toHaveClass("danger");
    fireEvent.click(screen.getByRole("menuitem", { name: "복사" }));
    expect(mocks.copySelection).toHaveBeenCalledWith("session-1");
  });

  it("활성 pane과 다른 exact pane의 distro·cwd로 세로 분할하고 layout을 전환한다", async () => {
    const firstCwd = "/mnt/c/projects/first";
    const { pane } = await renderWithPane(firstCwd);
    fireEvent.change(screen.getByPlaceholderText(/경로 열기/u), {
      target: { value: "/mnt/c/projects/second" },
    });
    fireEvent.click(screen.getByRole("button", { name: "+ 터미널" }));
    await screen.findByTitle("Close terminal session-2");

    fireEvent.contextMenu(pane);
    fireEvent.click(screen.getByRole("menuitem", { name: "세로 분할" }));

    await waitFor(() => expect(startSessionMock).toHaveBeenNthCalledWith(
      3,
      "Ubuntu",
      firstCwd,
      expect.any(String),
      "native",
    ));
    await waitFor(() => expect(screen.getAllByLabelText("Ubuntu 터미널 팬")).toHaveLength(3));
    const canvas = document.querySelector(".panes") as HTMLElement;
    expect(canvas.style.gridTemplateColumns).toContain("repeat(3");
  });

  it("pane 닫기는 confirmation 전 backend를 호출하지 않고 취소 뒤 terminal focus를 복원한다", async () => {
    const { pane } = await renderWithPane();
    pane.focus();

    fireEvent.keyDown(pane, { key: "F10", code: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "팬 닫기" }));

    await answerDialog(false);
    expect(closeSessionMock).not.toHaveBeenCalled();
    await waitFor(() => expect(mocks.terminalFocus).toHaveBeenCalledWith("session-1"));
    await waitFor(() => expect(document.activeElement).toBe(pane));

    fireEvent.keyDown(pane, { key: "ContextMenu", code: "ContextMenu" });
    fireEvent.click(screen.getByRole("menuitem", { name: "팬 닫기" }));
    await answerDialog(true);
    await waitFor(() => expect(closeSessionMock).toHaveBeenCalledWith("session-1"));
    await waitFor(() => expect(screen.queryByLabelText("Ubuntu 터미널 팬")).not.toBeInTheDocument());
  });

  it("tab 메뉴는 rename과 exact layout submenu를 keyboard로 실행한다", async () => {
    const { tab } = await renderWithPane();
    tab.focus();

    fireEvent.keyDown(tab, { key: "F10", code: "F10", shiftKey: true });
    for (const label of ["닫기", "다른 탭 닫기", "이름 변경", "레이아웃 전환"]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeInTheDocument();
    }
    expect(screen.getByRole("menuitem", { name: "다른 탭 닫기" })).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(screen.getByRole("menuitem", { name: "이름 변경" }));
    await answerDialog(false);
    await waitFor(() => expect(document.activeElement).toBe(tab));

    fireEvent.keyDown(tab, { key: "ContextMenu", code: "ContextMenu" });
    fireEvent.click(screen.getByRole("menuitem", { name: "이름 변경" }));
    await answerDialog(true, "작업 탭");
    const renamed = await screen.findByLabelText("작업 탭 터미널 탭") as HTMLDivElement;

    fireEvent.contextMenu(renamed);
    fireEvent.click(screen.getByRole("menuitem", { name: "레이아웃 전환" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "가로 분할" }));
    expect(screen.getByRole("button", { name: "rows" })).toHaveClass("active");
  });

  it("자동 탭 제목은 활성 pane OSC 제목을 따르고 수동 rename 뒤에는 덮어쓰지 않는다", async () => {
    const { pane, tab } = await renderWithPane();

    fireEvent.click(screen.getByTitle("Emit title session-1"));
    const autoTitle = await screen.findByLabelText("npm test 터미널 탭");
    expect(pane).toHaveAttribute("aria-label", "npm test 터미널 팬");

    fireEvent.contextMenu(autoTitle);
    fireEvent.click(screen.getByRole("menuitem", { name: "이름 변경" }));
    await answerDialog(true, "내 작업");
    const customTitle = await screen.findByLabelText("내 작업 터미널 탭");

    fireEvent.click(screen.getByTitle("Emit title session-1"));
    expect(customTitle).toBeInTheDocument();
    expect(screen.queryByLabelText("npm test 터미널 탭")).not.toBeInTheDocument();
    expect(tab).toHaveAttribute("aria-label", "내 작업 터미널 탭");
  });

  it("다른 탭 닫기는 target tab을 먼저 활성화하고 승인된 다른 session만 닫는다", async () => {
    const { tab } = await renderWithPane();
    fireEvent.click(screen.getByTitle("새 탭 (Ctrl+Shift+T)"));
    await screen.findByLabelText("Ubuntu 2 터미널 탭");

    fireEvent.contextMenu(tab);
    expect(tab).toHaveAttribute("aria-selected", "true");
    fireEvent.click(screen.getByRole("menuitem", { name: "다른 탭 닫기" }));
    await answerDialog(false);
    expect(closeSessionMock).not.toHaveBeenCalled();

    fireEvent.contextMenu(tab);
    fireEvent.click(screen.getByRole("menuitem", { name: "다른 탭 닫기" }));
    await answerDialog(true);
    await waitFor(() => expect(closeSessionMock).toHaveBeenCalledWith("session-2"));
    expect(closeSessionMock).not.toHaveBeenCalledWith("session-1");
    await waitFor(() => expect(screen.queryByLabelText("Ubuntu 2 터미널 탭")).not.toBeInTheDocument());
  });

  it("단일 탭 닫기는 기존 button과 context menu 모두 같은 confirmation을 거친다", async () => {
    const { tab } = await renderWithPane();

    fireEvent.click(screen.getByTitle("탭 닫기"));
    await answerDialog(false);
    expect(closeSessionMock).not.toHaveBeenCalled();
    expect(tab).toBeInTheDocument();

    fireEvent.contextMenu(tab);
    fireEvent.click(screen.getByRole("menuitem", { name: "닫기" }));
    await answerDialog(true);
    await waitFor(() => expect(closeSessionMock).toHaveBeenCalledWith("session-1"));
    await waitFor(() => expect(screen.queryByLabelText("Ubuntu 터미널 탭")).not.toBeInTheDocument());
  });

  it("다른 탭 닫기의 부분 실패는 성공한 session만 제거하고 실패한 팬을 유지한다", async () => {
    const raw = "C:\\secret\\partial-close credential-raw";
    const { tab } = await renderWithPane();
    fireEvent.click(screen.getByTitle("새 탭 (Ctrl+Shift+T)"));
    await screen.findByLabelText("Ubuntu 2 터미널 탭");
    fireEvent.click(screen.getByRole("button", { name: "+ 터미널" }));
    await screen.findByTitle("Close terminal session-3");
    closeSessionMock.mockImplementation(async (id) => {
      if (id === "session-3") throw new Error(raw);
    });
    fireEvent.contextMenu(tab);
    fireEvent.click(screen.getByRole("menuitem", { name: "다른 탭 닫기" }));
    await answerDialog(true);

    await waitFor(() => expect(closeSessionMock).toHaveBeenCalledWith("session-2"));
    await waitFor(() => expect(closeSessionMock).toHaveBeenCalledWith("session-3"));
    await waitFor(() => expect(screen.queryByTitle("Close terminal session-2")).not.toBeInTheDocument());
    expect(screen.getByTitle("Close terminal session-3")).toBeInTheDocument();
    expect(screen.getByLabelText("Ubuntu 2 터미널 탭")).toBeInTheDocument();
    expect(await screen.findByText("터미널 탭을 모두 닫지 못했습니다.")).toBeInTheDocument();
    expect(document.body.textContent).not.toContain(raw);
  });

  it("split·close 실패는 backend raw path나 오류를 화면에 반향하지 않는다", async () => {
    const raw = "C:\\secret\\workspace credential-raw";
    const { pane } = await renderWithPane();
    startSessionMock.mockRejectedValueOnce(new Error(raw));

    fireEvent.contextMenu(pane);
    fireEvent.click(screen.getByRole("menuitem", { name: "가로 분할" }));

    expect(await screen.findByText("터미널 팬을 안전하게 분할하지 못했습니다.")).toBeInTheDocument();
    expect(document.body.textContent).not.toContain(raw);

    closeSessionMock.mockRejectedValueOnce(new Error(raw));
    fireEvent.click(screen.getByTitle("Close terminal session-1"));
    await answerDialog(true);
    expect(await screen.findByText("터미널 팬을 닫지 못했습니다.")).toBeInTheDocument();
    expect(document.body.textContent).not.toContain(raw);
  });

  it("새 터미널 시작 실패는 backend raw path나 오류를 화면에 반향하지 않는다", async () => {
    const raw = "C:\\secret\\start credential-raw";
    render(<App />);
    await screen.findAllByRole("option", { name: /Ubuntu/u });
    const addButton = screen.getByRole("button", { name: "+ 터미널" });
    await waitFor(() => expect(addButton).toBeEnabled());
    startSessionMock.mockRejectedValueOnce(new Error(raw));

    fireEvent.click(addButton);

    expect(await screen.findByText("터미널을 시작하지 못했습니다.")).toBeInTheDocument();
    expect(document.body.textContent).not.toContain(raw);
  });
});
