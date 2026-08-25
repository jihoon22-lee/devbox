import { useEffect, useRef, type CSSProperties, type HTMLAttributes } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { closeSession, startSession } from "./api";
import type { OpenRequest } from "./types";

const mocks = vi.hoisted(() => ({
  nextSession: 0,
  terminalFocus: vi.fn<(id: string) => void>(),
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
      return () => props.unregisterFocus(props.sessionId);
    }, [props.registerFocus, props.sessionId, props.unregisterFocus]);
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
      </div>
    );
  },
}));

vi.mock("./api", () => ({
  listDistros: vi.fn().mockResolvedValue([
    { name: "Ubuntu", version: 2, default: true, state: "Running" },
  ]),
  dockerPs: vi.fn().mockResolvedValue([]),
  dockerAction: vi.fn().mockResolvedValue(undefined),
  startSession: vi.fn().mockImplementation(async () => `session-${++mocks.nextSession}`),
  closeSession: vi.fn().mockResolvedValue(undefined),
  onTerminalClosed: vi.fn().mockResolvedValue(() => undefined),
  onTerminalOutput: vi.fn().mockResolvedValue(() => undefined),
  getWindowsBuildNumber: vi.fn().mockResolvedValue(null),
  takePendingOpen: vi.fn().mockResolvedValue(null),
  onOpenRequest: vi.fn().mockImplementation(async (_handler: (request: OpenRequest) => void) => () => undefined),
}));

const startSessionMock = vi.mocked(startSession);
const closeSessionMock = vi.mocked(closeSession);
const confirmMock = vi.fn<(message?: string) => boolean>();
const promptMock = vi.fn<(message?: string, defaultValue?: string) => string | null>();

async function renderWithPane(cwd = "") {
  render(<App />);
  await screen.findAllByRole("option", { name: /Ubuntu/u });
  if (cwd) fireEvent.change(screen.getByPlaceholderText(/Open path/u), { target: { value: cwd } });
  fireEvent.click(screen.getByRole("button", { name: "+ Terminal" }));
  const pane = await screen.findByLabelText("Ubuntu 터미널 팬") as HTMLDivElement;
  const tab = await screen.findByLabelText("Ubuntu 터미널 탭") as HTMLDivElement;
  return { pane, tab };
}

beforeEach(() => {
  mocks.nextSession = 0;
  mocks.terminalFocus.mockReset();
  startSessionMock.mockReset().mockImplementation(async () => `session-${++mocks.nextSession}`);
  closeSessionMock.mockReset().mockResolvedValue(undefined);
  confirmMock.mockReset().mockReturnValue(false);
  promptMock.mockReset().mockReturnValue(null);
  Object.defineProperty(window, "confirm", { configurable: true, value: confirmMock });
  Object.defineProperty(window, "prompt", { configurable: true, value: promptMock });
});

afterEach(() => cleanup());

describe("WSL Desktop pane and tab context menus", () => {
  it("우클릭한 exact pane을 선택하고 #262 경계를 포함한 정확한 메뉴를 표시한다", async () => {
    const { pane } = await renderWithPane();

    fireEvent.contextMenu(pane, { clientX: 20, clientY: 24 });

    expect(pane.className).toContain("pane-focused");
    for (const label of ["복사", "붙여넣기", "검색", "세로 분할", "가로 분할", "cwd 복사", "팬 닫기"]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeInTheDocument();
    }
    for (const label of ["복사", "붙여넣기", "검색", "cwd 복사"]) {
      expect(screen.getByRole("menuitem", { name: label })).toHaveAttribute("aria-disabled", "true");
    }
    expect(screen.getByRole("menuitem", { name: "팬 닫기" })).toHaveClass("danger");
  });

  it("활성 pane과 다른 exact pane의 distro·cwd로 세로 분할하고 layout을 전환한다", async () => {
    const firstCwd = "/mnt/c/projects/first";
    const { pane } = await renderWithPane(firstCwd);
    fireEvent.change(screen.getByPlaceholderText(/Open path/u), {
      target: { value: "/mnt/c/projects/second" },
    });
    fireEvent.click(screen.getByRole("button", { name: "+ Terminal" }));
    await screen.findByTitle("Close terminal session-2");

    fireEvent.contextMenu(pane);
    fireEvent.click(screen.getByRole("menuitem", { name: "세로 분할" }));

    await waitFor(() => expect(startSessionMock).toHaveBeenNthCalledWith(3, "Ubuntu", firstCwd));
    await waitFor(() => expect(screen.getAllByLabelText("Ubuntu 터미널 팬")).toHaveLength(3));
    const canvas = document.querySelector(".panes") as HTMLElement;
    expect(canvas.style.gridTemplateColumns).toContain("repeat(3");
  });

  it("pane 닫기는 confirmation 전 backend를 호출하지 않고 취소 뒤 terminal focus를 복원한다", async () => {
    const { pane } = await renderWithPane();
    pane.focus();

    fireEvent.keyDown(pane, { key: "F10", code: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "팬 닫기" }));

    expect(confirmMock).toHaveBeenCalledTimes(1);
    expect(closeSessionMock).not.toHaveBeenCalled();
    await waitFor(() => expect(mocks.terminalFocus).toHaveBeenCalledWith("session-1"));
    expect(document.activeElement).toBe(pane);

    confirmMock.mockReturnValueOnce(true);
    fireEvent.keyDown(pane, { key: "ContextMenu", code: "ContextMenu" });
    fireEvent.click(screen.getByRole("menuitem", { name: "팬 닫기" }));
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
    await waitFor(() => expect(document.activeElement).toBe(tab));

    promptMock.mockReturnValueOnce("작업 탭");
    fireEvent.keyDown(tab, { key: "ContextMenu", code: "ContextMenu" });
    fireEvent.click(screen.getByRole("menuitem", { name: "이름 변경" }));
    const renamed = await screen.findByLabelText("작업 탭 터미널 탭") as HTMLDivElement;

    fireEvent.contextMenu(renamed);
    fireEvent.click(screen.getByRole("menuitem", { name: "레이아웃 전환" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "가로 분할" }));
    expect(screen.getByRole("button", { name: "rows" })).toHaveClass("active");
  });

  it("다른 탭 닫기는 target tab을 먼저 활성화하고 승인된 다른 session만 닫는다", async () => {
    const { tab } = await renderWithPane();
    fireEvent.click(screen.getByTitle("New tab (Ctrl+Shift+T)"));
    await screen.findByLabelText("Ubuntu 2 터미널 탭");

    fireEvent.contextMenu(tab);
    expect(tab).toHaveAttribute("aria-current", "true");
    fireEvent.click(screen.getByRole("menuitem", { name: "다른 탭 닫기" }));
    expect(closeSessionMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.contextMenu(tab);
    fireEvent.click(screen.getByRole("menuitem", { name: "다른 탭 닫기" }));
    await waitFor(() => expect(closeSessionMock).toHaveBeenCalledWith("session-2"));
    expect(closeSessionMock).not.toHaveBeenCalledWith("session-1");
    await waitFor(() => expect(screen.queryByLabelText("Ubuntu 2 터미널 탭")).not.toBeInTheDocument());
  });

  it("단일 탭 닫기는 기존 button과 context menu 모두 같은 confirmation을 거친다", async () => {
    const { tab } = await renderWithPane();

    fireEvent.click(screen.getByTitle("Close tab"));
    expect(confirmMock).toHaveBeenCalledTimes(1);
    expect(closeSessionMock).not.toHaveBeenCalled();
    expect(tab).toBeInTheDocument();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.contextMenu(tab);
    fireEvent.click(screen.getByRole("menuitem", { name: "닫기" }));
    await waitFor(() => expect(closeSessionMock).toHaveBeenCalledWith("session-1"));
    await waitFor(() => expect(screen.queryByLabelText("Ubuntu 터미널 탭")).not.toBeInTheDocument());
  });

  it("다른 탭 닫기의 부분 실패는 성공한 session만 제거하고 실패한 팬을 유지한다", async () => {
    const raw = "C:\\secret\\partial-close credential-raw";
    const { tab } = await renderWithPane();
    fireEvent.click(screen.getByTitle("New tab (Ctrl+Shift+T)"));
    await screen.findByLabelText("Ubuntu 2 터미널 탭");
    fireEvent.click(screen.getByRole("button", { name: "+ Terminal" }));
    await screen.findByTitle("Close terminal session-3");
    closeSessionMock.mockImplementation(async (id) => {
      if (id === "session-3") throw new Error(raw);
    });
    confirmMock.mockReturnValueOnce(true);

    fireEvent.contextMenu(tab);
    fireEvent.click(screen.getByRole("menuitem", { name: "다른 탭 닫기" }));

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
    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(screen.getByTitle("Close terminal session-1"));
    expect(await screen.findByText("터미널 팬을 닫지 못했습니다.")).toBeInTheDocument();
    expect(document.body.textContent).not.toContain(raw);
  });
});
