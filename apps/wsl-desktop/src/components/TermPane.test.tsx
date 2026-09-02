import type { ComponentProps } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import TermPane from "./TermPane";
import { DEFAULT_SETTINGS, TERMINAL_THEMES, fontFamilyFor } from "../lib/settings";
import {
  MAX_TERMINAL_PASTE_CHARACTERS,
  MAX_TERMINAL_SEARCH_CHARACTERS,
} from "../lib/terminalUx";

/**
 * jsdom에는 캔버스 텍스트 측정이 없어 실제 xterm을 그릴 수 없다(PaneCanvas.test.tsx의
 * 주석 참고 — 그쪽은 TermPane 자체를 통째로 모킹해 우회한다). 여기서는 반대로 TermPane의
 * resize 바닥값 로직(§2.3)만 검증하려는 것이므로, xterm 표면(Terminal/FitAddon/
 * Unicode11Addon)을 rows/cols를 직접 조작할 수 있는 최소 스텁으로 대체하고 실제
 * TermPane을 렌더링한다.
 */
const {
  createdTerminals,
  createdSearchAddons,
  createdWebLinksAddons,
  fitSizes,
  observerState,
  focusSpy,
  pasteSpy,
  FakeTerminal,
  FakeFitAddon,
  FakeSearchAddon,
  FakeUnicode11Addon,
  FakeWebLinksAddon,
} = vi.hoisted(() => {
  type TerminalOptions = {
    fontSize?: number;
    windowsPty?: { backend: string; buildNumber?: number };
    linkHandler?: { activate: (event: MouseEvent, text: string) => void };
  };
  type TerminalSize = { rows: number; cols: number };
  type Disposable = { dispose: () => void };
  type SearchResult = { resultIndex: number; resultCount: number };
  const createdTerminals: FakeTerminal[] = [];
  const createdSearchAddons: FakeSearchAddon[] = [];
  const createdWebLinksAddons: FakeWebLinksAddon[] = [];
  const fitSizes: TerminalSize[] = [];
  const observerState: { callback?: () => void } = {};
  const focusSpy = vi.fn();
  const pasteSpy = vi.fn<(text: string) => void>();

  class FakeTerminal {
    rows = 2;
    cols = 5;
    unicode = { activeVersion: "" };
    selection = "";
    keyHandler?: (event: KeyboardEvent) => boolean;
    selectionHandler?: () => void;
    dataHandler?: (data: string) => void;
    titleHandler?: (title: string) => void;
    osc7Handler?: (payload: string) => boolean;
    parser = {
      registerOscHandler: (identifier: number, handler: (payload: string) => boolean): Disposable => {
        if (identifier === 7) this.osc7Handler = handler;
        return { dispose: () => undefined };
      },
    };
    constructor(public options: TerminalOptions) {
      createdTerminals.push(this);
    }
    loadAddon(addon: { activate?: (terminal: FakeTerminal) => void }) {
      addon.activate?.(this);
    }
    open() {}
    attachCustomKeyEventHandler(handler: (event: KeyboardEvent) => boolean) {
      this.keyHandler = handler;
    }
    onData(handler: (data: string) => void) {
      this.dataHandler = handler;
      return { dispose: () => undefined };
    }
    onSelectionChange(handler: () => void) {
      this.selectionHandler = handler;
      return { dispose: () => undefined };
    }
    onTitleChange(handler: (title: string) => void) {
      this.titleHandler = handler;
      return { dispose: () => undefined };
    }
    getSelection() {
      return this.selection;
    }
    hasSelection() {
      return this.selection.length > 0;
    }
    paste(text: string) {
      pasteSpy(text);
    }
    write() {}
    focus() {
      focusSpy();
    }
    dispose() {}
  }

  class FakeFitAddon {
    fit() {
      const nextSize = fitSizes.shift();
      if (!nextSize) return;
      const term = createdTerminals[createdTerminals.length - 1];
      term.rows = nextSize.rows;
      term.cols = nextSize.cols;
    }
  }

  class FakeUnicode11Addon {}

  class FakeSearchAddon {
    resultHandler?: (result: SearchResult) => void;
    findNext = vi.fn<(term: string) => boolean>().mockReturnValue(true);
    findPrevious = vi.fn<(term: string) => boolean>().mockReturnValue(true);
    clearDecorations = vi.fn();
    constructor() {
      createdSearchAddons.push(this);
    }
    activate() {}
    onDidChangeResults(handler: (result: SearchResult) => void) {
      this.resultHandler = handler;
      return { dispose: () => undefined };
    }
  }

  class FakeWebLinksAddon {
    constructor(public handler?: (event: MouseEvent, uri: string) => void) {
      createdWebLinksAddons.push(this);
    }
    activate() {}
  }

  return {
    createdTerminals,
    createdSearchAddons,
    createdWebLinksAddons,
    fitSizes,
    observerState,
    focusSpy,
    pasteSpy,
    FakeTerminal,
    FakeFitAddon,
    FakeSearchAddon,
    FakeUnicode11Addon,
    FakeWebLinksAddon,
  };
});

vi.mock("@xterm/xterm", () => ({ Terminal: FakeTerminal }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: FakeFitAddon }));
vi.mock("@xterm/addon-search", () => ({ SearchAddon: FakeSearchAddon }));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: FakeUnicode11Addon }));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: FakeWebLinksAddon }));

const { mockResizeSession, mockReadClipboardText, mockOpenTerminalLink, mockBroadcast, mockWriteSession } = vi.hoisted(() => ({
  mockResizeSession: vi.fn().mockResolvedValue(undefined),
  mockReadClipboardText: vi.fn().mockResolvedValue(""),
  mockOpenTerminalLink: vi.fn().mockResolvedValue(undefined),
  mockBroadcast: vi.fn().mockResolvedValue(undefined),
  mockWriteSession: vi.fn().mockResolvedValue(undefined),
}));
const stableRegisterFocus = vi.fn<(id: string, focus: () => void) => void>();
const stableUnregisterFocus = vi.fn<(id: string) => void>();
const askMock = vi.fn().mockResolvedValue({ confirmed: false, value: "", remember: false });
const confirmLinkHostMock = vi.fn<(host: string) => Promise<boolean>>().mockResolvedValue(true);
const approve = () => askMock.mockResolvedValue({ confirmed: true, value: "", remember: false });
const reject = () => askMock.mockResolvedValue({ confirmed: false, value: "", remember: false });

vi.mock("../api", () => ({
  attachSession: vi.fn().mockResolvedValue(undefined),
  broadcast: mockBroadcast,
  writeSession: mockWriteSession,
  resizeSession: mockResizeSession,
  readClipboardText: mockReadClipboardText,
  openTerminalLink: mockOpenTerminalLink,
}));

function baseProps(overrides: Partial<ComponentProps<typeof TermPane>> = {}): ComponentProps<typeof TermPane> {
  return {
    sessionId: "s1",
    title: "Ubuntu",
    active: true,
    isFocusedPane: false,
    broadcastOn: false,
    broadcastTargetIds: [],
    isBroadcastTarget: false,
    copyOnSelect: true,
    fontSize: 13,
    fontFamily: fontFamilyFor(DEFAULT_SETTINGS.fontId),
    theme: TERMINAL_THEMES[DEFAULT_SETTINGS.theme],
    cursorStyle: DEFAULT_SETTINGS.cursorStyle,
    cursorBlink: DEFAULT_SETTINGS.cursorBlink,
    scrollbackLines: DEFAULT_SETTINGS.scrollbackLines,
    multiplexer: "native" as const,
    resumed: false,
    registerWrite: vi.fn(),
    unregisterWrite: vi.fn(),
    registerFocus: stableRegisterFocus,
    unregisterFocus: stableUnregisterFocus,
    registerTerminalHandle: vi.fn(),
    unregisterTerminalHandle: vi.fn(),
    onClose: vi.fn(),
    onFocusPane: vi.fn(),
    onShortcut: vi.fn(),
    onFontSizeChange: vi.fn(),
    onMetadataChange: vi.fn(),
    onTerminalError: vi.fn(),
    windowsBuildNumber: null,
    contextMenuTriggerProps: {},
    actionsDisabled: false,
    ask: askMock,
    onConfirmLinkHost: confirmLinkHostMock,
    ...overrides,
  };
}

beforeEach(() => {
  createdTerminals.length = 0;
  createdSearchAddons.length = 0;
  createdWebLinksAddons.length = 0;
  fitSizes.length = 0;
  observerState.callback = undefined;
  focusSpy.mockClear();
  pasteSpy.mockClear();
  mockResizeSession.mockClear();
  mockReadClipboardText.mockReset().mockResolvedValue("");
  mockOpenTerminalLink.mockReset().mockResolvedValue(undefined);
  mockBroadcast.mockReset().mockResolvedValue(undefined);
  mockWriteSession.mockReset().mockResolvedValue(undefined);
  stableRegisterFocus.mockClear();
  stableUnregisterFocus.mockClear();
  askMock.mockReset();
  reject();
  confirmLinkHostMock.mockReset().mockResolvedValue(true);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: vi.fn().mockResolvedValue(undefined) },
  });
  // TermPane 마운트는 항상 `new ResizeObserver(...)`를 호출한다. jsdom은 이를 구현하지
  // 않으므로(ReferenceError) 최소 스텁으로 대체한다. 이 스위트는 ResizeObserver의
  // 디바운스 경로가 아니라 mount 직접 호출 + active(rAF) 경로만으로 fitAndSendResize를
  // 구동하므로 observe/disconnect는 아무 일도 안 해도 된다.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) {
        observerState.callback = callback;
      }

      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  // active: false → true 전환의 rAF 트리거를 동기적으로 즉시 실행되게 만들어, 테스트가
  // 실제 프레임을 기다리지 않고도 결정적으로 재확인(fitAndSendResize)을 유발할 수 있게 한다.
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
    cb(0);
    return 0;
  });
  vi.stubGlobal("cancelAnimationFrame", () => undefined);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("TermPane — resize 바닥값 (§2.3)", () => {
  it("context menu trigger와 terminal focus 복구 함수를 pane root에 연결한다", () => {
    const onContextMenu = vi.fn();
    const onKeyDown = vi.fn();
    const registerFocus = vi.fn();
    const { container } = render(<TermPane {...baseProps({
      registerFocus,
      contextMenuTriggerProps: { onContextMenu, onKeyDown },
    })} />);
    const pane = container.querySelector(".pane") as HTMLDivElement;

    fireEvent.contextMenu(pane);
    fireEvent.keyDown(pane, { key: "F10", shiftKey: true });
    expect(onContextMenu).toHaveBeenCalledTimes(1);
    expect(onKeyDown).toHaveBeenCalledTimes(1);
    expect(registerFocus).toHaveBeenCalledWith("s1", expect.any(Function));

    const focus = registerFocus.mock.calls[0][1] as () => void;
    focus();
    expect(focusSpy).toHaveBeenCalledTimes(1);
  });

  it("passes the Windows build number to xterm ConPTY options", () => {
    const props = { ...baseProps(), windowsBuildNumber: 22631 } as ComponentProps<typeof TermPane>;
    render(<TermPane {...props} />);

    expect(createdTerminals[0].options.windowsPty).toEqual({ backend: "conpty", buildNumber: 22631 });
  });

  it("바닥값(4행 20열) 미만이면 마운트 시 resizeSession을 호출하지 않는다", () => {
    // FakeTerminal 기본값은 2행 5열 — 둘 다 바닥값 미만이다.
    render(<TermPane {...baseProps()} />);
    expect(mockResizeSession).not.toHaveBeenCalled();
  });

  it("바닥값 미만을 거치는 동안 이전 유효 크기가 보존돼, 그 크기로 복귀하면 재전송하지 않는다", async () => {
    // registerWrite/unregisterWrite를 모든 렌더에서 같은 참조로 유지해야 한다 — 바뀌면
    // 마운트 effect(의존성 [sessionId, registerWrite, unregisterWrite])가 매번 다시
    // 돌아 Terminal이 재생성되며 아래 term 조작이 새 인스턴스를 놓치게 된다.
    const registerWrite = vi.fn();
    const unregisterWrite = vi.fn();
    const props = (active: boolean) => baseProps({ registerWrite, unregisterWrite, active });

    const { rerender } = render(<TermPane {...props(false)} />);
    const term = createdTerminals[0];
    expect(mockResizeSession).not.toHaveBeenCalled();

    // 1) 유효 크기(24행 80열)로 자란다 → 전송되고, .then()에서 lastSizeRef에 커밋된다.
    term.rows = 24;
    term.cols = 80;
    rerender(<TermPane {...props(true)} />);
    await Promise.resolve();
    await Promise.resolve();
    expect(mockResizeSession).toHaveBeenCalledTimes(1);
    expect(mockResizeSession).toHaveBeenLastCalledWith("s1", 24, 80);

    // 2) 바닥값 미만(2행 5열)으로 줄어든다 → 전송하지 않는다. 아래 3)에서 이 구간 동안
    //    lastSizeRef가 (실수로) 갱신되지 않았음을 간접적으로 확인한다.
    rerender(<TermPane {...props(false)} />);
    term.rows = 2;
    term.cols = 5;
    rerender(<TermPane {...props(true)} />);
    expect(mockResizeSession).toHaveBeenCalledTimes(1);

    // 3) 원래의 유효 크기(24행 80열)로 복귀한다. lastSizeRef가 2)에서 그대로였다면
    //    (24,80)은 이미 전송된 적이 있으므로 재전송하지 않아야 한다 — 만약 2)에서
    //    바닥값 미만 크기를 lastSizeRef에 잘못 커밋했다면 여기서 다시 호출돼 실패한다.
    rerender(<TermPane {...props(false)} />);
    term.rows = 24;
    term.cols = 80;
    rerender(<TermPane {...props(true)} />);
    expect(mockResizeSession).toHaveBeenCalledTimes(1);
  });

  it("retries the same dimensions when resizeSession rejects", async () => {
    vi.useFakeTimers();
    const registerWrite = vi.fn();
    const unregisterWrite = vi.fn();
    render(<TermPane {...baseProps({ active: false, registerWrite, unregisterWrite })} />);
    const term = createdTerminals[0];
    term.rows = 24;
    term.cols = 80;
    mockResizeSession.mockRejectedValueOnce(new Error("resize rejected"));

    expect(observerState.callback).toBeDefined();
    observerState.callback!();
    vi.advanceTimersByTime(100);
    expect(mockResizeSession).toHaveBeenCalledTimes(1);
    expect(mockResizeSession).toHaveBeenNthCalledWith(1, "s1", 24, 80);

    // Let the rejection handler run without committing the rejected size.
    await Promise.resolve();
    await Promise.resolve();

    observerState.callback!();
    vi.advanceTimersByTime(100);
    expect(mockResizeSession).toHaveBeenCalledTimes(2);
    expect(mockResizeSession).toHaveBeenNthCalledWith(2, "s1", 24, 80);
  });

  it("cancels a pending ResizeObserver resize before the activation resize", async () => {
    vi.useFakeTimers();
    const registerWrite = vi.fn();
    const unregisterWrite = vi.fn();
    // Mount fit, activation fit, and a stale timer fit respectively. The third size
    // must never reach resizeSession when activation cancels the pending timer.
    fitSizes.push({ rows: 2, cols: 5 }, { rows: 24, cols: 80 }, { rows: 40, cols: 120 });
    const { rerender } = render(<TermPane {...baseProps({ active: false, registerWrite, unregisterWrite })} />);

    expect(observerState.callback).toBeDefined();
    observerState.callback!();
    const clearTimeoutSpy = vi.spyOn(window, "clearTimeout");
    const clearCallsBeforeActivation = clearTimeoutSpy.mock.invocationCallOrder.length;

    rerender(<TermPane {...baseProps({ active: true, registerWrite, unregisterWrite })} />);
    vi.runOnlyPendingTimers();
    await Promise.resolve();
    await Promise.resolve();

    expect(mockResizeSession).toHaveBeenCalledTimes(1);
    expect(mockResizeSession).toHaveBeenNthCalledWith(1, "s1", 24, 80);
    const activationClearOrders = clearTimeoutSpy.mock.invocationCallOrder.slice(clearCallsBeforeActivation);
    expect(activationClearOrders).not.toHaveLength(0);
    expect(activationClearOrders[0]).toBeLessThan(mockResizeSession.mock.invocationCallOrder[0]);

    vi.advanceTimersByTime(100);
    await Promise.resolve();
    expect(mockResizeSession).toHaveBeenCalledTimes(1);
    clearTimeoutSpy.mockRestore();
  });
});

describe("TermPane — clipboard, OSC, search, link와 font UX (#262)", () => {
  it("selection이 120ms 유지될 때만 자동 복사하고 clipboard 실패를 고정 메시지로 격리한다", async () => {
    vi.useFakeTimers();
    const onTerminalError = vi.fn();
    const raw = "C:\\secret\\clipboard credential-raw";
    const writeText = vi.mocked(navigator.clipboard.writeText);
    writeText.mockRejectedValueOnce(new Error(raw));
    render(<TermPane {...baseProps({ onTerminalError })} />);
    const term = createdTerminals[0];

    term.selection = "first";
    term.selectionHandler?.();
    term.selection = "settled";
    term.selectionHandler?.();
    await act(async () => {
      vi.advanceTimersByTime(120);
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(writeText).toHaveBeenCalledTimes(1);
    expect(writeText).toHaveBeenCalledWith("settled");
    expect(onTerminalError).toHaveBeenCalledWith(
      "선택한 텍스트를 클립보드에 복사하지 못했습니다.",
    );
    expect(onTerminalError).not.toHaveBeenCalledWith(expect.stringContaining(raw));
  });

  it("exact pane handle이 복사·다중행 확인 붙여넣기·OSC cwd 복사·검색을 제공한다", async () => {
    const registerTerminalHandle = vi.fn();
    const onTerminalError = vi.fn();
    approve();
    mockReadClipboardText.mockResolvedValue("echo one\r\necho two");
    const { container } = render(<TermPane {...baseProps({ registerTerminalHandle, onTerminalError })} />);
    const term = createdTerminals[0];
    const handle = registerTerminalHandle.mock.calls[0][1];

    term.selection = "selected output";
    expect(handle.getCapabilities()).toEqual({ hasSelection: true, hasCwd: false });
    await handle.copySelection();
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("selected output");

    await handle.pasteClipboard();
    expect(askMock).toHaveBeenCalledWith(expect.objectContaining({
      kind: "confirm",
      title: "2줄을 터미널에 붙여넣을까요?",
    }));
    expect(pasteSpy).toHaveBeenCalledWith("echo one\r\necho two");

    const raw = "C:\\secret\\clipboard-read credential-raw";
    mockReadClipboardText.mockRejectedValue(new Error(raw));
    await handle.pasteClipboard();
    expect(onTerminalError).toHaveBeenCalledWith("클립보드 내용을 터미널에 붙여넣지 못했습니다.");
    expect(onTerminalError).not.toHaveBeenCalledWith(expect.stringContaining(raw));

    mockReadClipboardText.mockResolvedValue("x".repeat(MAX_TERMINAL_PASTE_CHARACTERS + 1));
    await handle.pasteClipboard();
    expect(pasteSpy).toHaveBeenCalledTimes(1);
    expect(onTerminalError).toHaveBeenCalledWith("붙여넣을 내용이 1,000,000자를 초과합니다.");

    mockReadClipboardText.mockResolvedValue("middle paste");
    fireEvent(
      container.querySelector(".term-wrap") as HTMLDivElement,
      new MouseEvent("auxclick", { bubbles: true, button: 1 }),
    );
    await waitFor(() => expect(pasteSpy).toHaveBeenLastCalledWith("middle paste"));

    act(() => {
      term.osc7Handler?.("file://wsl-host/mnt/c/My%20Repo");
      handle.openSearch();
    });
    expect(handle.getCapabilities()).toEqual({ hasSelection: true, hasCwd: true });
    await handle.copyCwd();
    expect(navigator.clipboard.writeText).toHaveBeenLastCalledWith("/mnt/c/My Repo");
    expect(await screen.findByRole("search", { name: "터미널 출력 검색" })).toBeInTheDocument();

    fireEvent.change(screen.getByRole("textbox", { name: "검색어" }), { target: { value: "error" } });
    expect(createdSearchAddons[0].findNext).toHaveBeenCalledWith("error", expect.any(Object));
    fireEvent.change(screen.getByRole("textbox", { name: "검색어" }), {
      target: { value: "q".repeat(MAX_TERMINAL_SEARCH_CHARACTERS + 1) },
    });
    expect(createdSearchAddons[0].findNext).toHaveBeenLastCalledWith(
      "q".repeat(MAX_TERMINAL_SEARCH_CHARACTERS),
      expect.any(Object),
    );
    fireEvent.click(screen.getByTitle("이전 결과 (Shift+Enter)"));
    expect(createdSearchAddons[0].findPrevious).toHaveBeenCalledWith(
      "q".repeat(MAX_TERMINAL_SEARCH_CHARACTERS),
      expect.any(Object),
    );
    act(() => createdSearchAddons[0].resultHandler?.({ resultIndex: 1, resultCount: 3 }));
    expect(screen.getByText("2/3")).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("textbox", { name: "검색어" }), { key: "Escape" });
    expect(screen.queryByRole("search", { name: "터미널 출력 검색" })).not.toBeInTheDocument();
  });

  it("Ctrl+Shift+C/V/F는 로컬 처리하지만 bare Ctrl+C는 PTY 입력 경로에 남긴다", async () => {
    const registerTerminalHandle = vi.fn();
    render(<TermPane {...baseProps({ registerTerminalHandle })} />);
    const term = createdTerminals[0];
    term.selection = "copy me";
    const event = (key: string, code: string, shiftKey: boolean) => ({
      type: "keydown",
      ctrlKey: true,
      shiftKey,
      altKey: false,
      metaKey: false,
      key,
      code,
      preventDefault: vi.fn(),
      stopPropagation: vi.fn(),
    }) as unknown as KeyboardEvent;

    expect(term.keyHandler?.(event("c", "KeyC", false))).toBe(true);
    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
    expect(term.keyHandler?.(event("c", "KeyC", true))).toBe(false);
    await waitFor(() => expect(navigator.clipboard.writeText).toHaveBeenCalledWith("copy me"));

    mockReadClipboardText.mockResolvedValue("keyboard paste");
    expect(term.keyHandler?.(event("v", "KeyV", true))).toBe(false);
    await waitFor(() => expect(pasteSpy).toHaveBeenCalledWith("keyboard paste"));

    const searchEvent = event("f", "KeyF", true);
    act(() => { term.keyHandler?.(searchEvent); });
    expect(await screen.findByRole("search", { name: "터미널 출력 검색" })).toBeInTheDocument();
  });

  it("OSC 0/2 제목과 유효한 OSC 7만 metadata에 반영한다", () => {
    const onMetadataChange = vi.fn();
    render(<TermPane {...baseProps({ onMetadataChange })} />);
    const term = createdTerminals[0];

    act(() => {
      term.titleHandler?.(" build\u0000\r\nlogs ");
      term.osc7Handler?.("https://example.com/secret");
      term.osc7Handler?.("file://wsl-host/home/me/project");
    });

    expect(onMetadataChange).toHaveBeenCalledWith("s1", { title: "build logs" });
    expect(onMetadataChange).toHaveBeenCalledWith("s1", { cwd: "/home/me/project" });
    expect(onMetadataChange).toHaveBeenCalledTimes(2);
  });

  it("HTTP(S) 링크만 host 확인 뒤 열고 font 변경은 terminal을 재마운트하지 않는다", async () => {
    const onTerminalError = vi.fn();
    const registerWrite = vi.fn();
    const unregisterWrite = vi.fn();
    const props = (nextFontSize: number) => baseProps({
      fontSize: nextFontSize,
      onTerminalError,
      registerWrite,
      unregisterWrite,
    });
    const { rerender } = render(<TermPane {...props(13)} />);
    const term = createdTerminals[0];

    const preventDefault = vi.fn();
    term.options.linkHandler?.activate({ preventDefault } as unknown as MouseEvent, "javascript:alert(1)");
    expect(onTerminalError).toHaveBeenCalledWith("지원하지 않는 링크 형식입니다.");
    expect(mockOpenTerminalLink).not.toHaveBeenCalled();

    createdWebLinksAddons[0].handler?.(
      { preventDefault } as unknown as MouseEvent,
      "https://example.com/docs",
    );
    await waitFor(() => expect(mockOpenTerminalLink).toHaveBeenCalledWith("https://example.com/docs"));
    expect(confirmLinkHostMock).toHaveBeenCalledWith("example.com");

    rerender(<TermPane {...props(16)} />);
    expect(createdTerminals).toHaveLength(1);
    expect(term.options.fontSize).toBe(16);
  });
});

describe("TermPane — profile command와 safe broadcast (#263)", () => {
  it("새 세션의 시작 명령은 한 번만 보내고 rerender에서 재실행하지 않는다", async () => {
    const registerWrite = vi.fn();
    const unregisterWrite = vi.fn();
    const props = baseProps({ initialCommand: "pnpm dev", registerWrite, unregisterWrite });
    const { rerender } = render(<TermPane {...props} />);
    await waitFor(() => expect(mockWriteSession).toHaveBeenCalledWith("s1", "pnpm dev\r"));

    rerender(<TermPane {...props} fontSize={14} />);
    expect(mockWriteSession).toHaveBeenCalledTimes(1);
  });

  it("명시적으로 선택한 대상에만 broadcast하고 위험 Enter 취소 시 실행을 막는다", async () => {
    reject();
    render(<TermPane {...baseProps({ broadcastOn: true, broadcastTargetIds: ["s1", "s3"] })} />);
    const term = createdTerminals[0];

    act(() => term.dataHandler?.("sudo rm -rf ./cache"));
    expect(mockBroadcast).toHaveBeenCalledWith(["s1", "s3"], "sudo rm -rf ./cache");
    act(() => term.dataHandler?.("\r"));
    await waitFor(() => expect(askMock).toHaveBeenCalledWith(expect.objectContaining({
      title: "2개 터미널에 위험할 수 있는 명령을 동시에 보낼까요?",
    })));
    expect(mockBroadcast).toHaveBeenCalledTimes(1);
    act(() => term.dataHandler?.("\r"));
    await waitFor(() => expect(askMock).toHaveBeenCalledTimes(2));
    expect(mockBroadcast).toHaveBeenCalledTimes(1);
  });

  it("확인이 열려 있는 동안 들어온 입력은 순서를 지켜 확인 뒤에 전달된다", async () => {
    let release: ((answer: { confirmed: boolean; value: string; remember: boolean }) => void) | undefined;
    askMock.mockImplementationOnce(() => new Promise((resolve) => { release = resolve; }));
    render(<TermPane {...baseProps({ broadcastOn: true, broadcastTargetIds: ["s1", "s2"] })} />);
    const term = createdTerminals[0];

    act(() => term.dataHandler?.("rm -rf ./cache"));
    expect(mockBroadcast).toHaveBeenCalledTimes(1);
    act(() => term.dataHandler?.("\r"));
    await waitFor(() => expect(release).toBeDefined());

    // Arrives in the frame before the dialog takes focus: it must wait its turn.
    act(() => term.dataHandler?.("echo after"));
    expect(mockBroadcast).toHaveBeenCalledTimes(1);

    approve();
    act(() => release?.({ confirmed: true, value: "", remember: false }));
    await waitFor(() => expect(mockBroadcast).toHaveBeenCalledTimes(3));
    expect(mockBroadcast.mock.calls.map((call) => call[1])).toEqual([
      "rm -rf ./cache",
      "\r",
      "echo after",
    ]);
  });

  it("multiline broadcast는 대상 수 확인 뒤 취소하고 raw command를 확인문에 넣지 않는다", async () => {
    reject();
    render(<TermPane {...baseProps({ broadcastOn: true, broadcastTargetIds: ["s1", "s2", "s4"] })} />);
    const term = createdTerminals[0];

    act(() => term.dataHandler?.("echo raw-one\necho raw-two\n"));
    await waitFor(() => expect(askMock).toHaveBeenCalledTimes(1));
    expect(mockBroadcast).not.toHaveBeenCalled();
    const request = askMock.mock.calls[0]?.[0] as { title: string };
    expect(request.title).toContain("3개");
    expect(JSON.stringify(request)).not.toContain("raw-one");
  });

  it("대상 2개 미만이면 broadcast mode가 켜져 있어도 현재 팬에만 입력한다", () => {
    render(<TermPane {...baseProps({ broadcastOn: true, broadcastTargetIds: ["s1"] })} />);
    act(() => createdTerminals[0].dataHandler?.("echo local"));
    expect(mockBroadcast).not.toHaveBeenCalled();
    expect(mockWriteSession).toHaveBeenCalledWith("s1", "echo local");
  });

  it("backend가 broadcast 대상을 거부하면 owner에 fail-closed를 알리고 raw 오류를 숨긴다", async () => {
    const onBroadcastFailure = vi.fn();
    const onTerminalError = vi.fn();
    const raw = "C:\\secret\\stale-session credential-raw";
    mockBroadcast.mockRejectedValueOnce(new Error(raw));
    render(<TermPane {...baseProps({
      broadcastOn: true,
      broadcastTargetIds: ["s1", "s2"],
      onBroadcastFailure,
      onTerminalError,
    })} />);

    act(() => createdTerminals[0].dataHandler?.("echo safe"));
    await waitFor(() => expect(onBroadcastFailure).toHaveBeenCalledTimes(1));
    expect(onTerminalError).toHaveBeenCalledWith("broadcast 입력을 모든 대상 터미널에 전달하지 못했습니다.");
    expect(onTerminalError).not.toHaveBeenCalledWith(expect.stringContaining(raw));
  });
});
