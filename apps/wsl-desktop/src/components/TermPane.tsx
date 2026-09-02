import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import type { ContextMenuTriggerProps } from "@devbox/context-menu";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon, type ISearchResultChangeEvent } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import {
  attachSession,
  broadcast,
  openTerminalLink,
  readClipboardText,
  resizeSession,
  writeSession,
} from "../api";
import { matchShortcut, type ShortcutAction } from "../lib/shortcuts";
import {
  DEFAULT_TERMINAL_FONT_SIZE,
  MAX_TERMINAL_PASTE_CHARACTERS,
  MAX_TERMINAL_SEARCH_CHARACTERS,
  clampTerminalFontSize,
  hasMultilinePaste,
  matchTerminalKey,
  normalizeTerminalLink,
  normalizeTerminalTitle,
  parseOsc7Cwd,
  pasteLineCount,
  type TerminalKeyAction,
} from "../lib/terminalUx";
import { assessBroadcastInput } from "../lib/broadcastSafety";
import type { AskDialog } from "./AppDialog";
import type { CursorStyle, TerminalTheme } from "../lib/settings";
import type { MultiplexerKind } from "../types";

export interface TerminalPaneCapabilities {
  hasSelection: boolean;
  hasCwd: boolean;
}

export interface TerminalPaneHandle {
  getCapabilities: () => TerminalPaneCapabilities;
  copySelection: () => Promise<void>;
  pasteClipboard: () => Promise<void>;
  openSearch: () => void;
  copyCwd: () => Promise<void>;
  clearScrollback: () => void;
  scrollToBottom: () => void;
  selectAll: () => void;
}

interface TermPaneProps {
  sessionId: string;
  title: string;
  /** 이 팬이 속한 탭이 활성 탭인가. false면 PaneCanvas가 style로 display:none을 준다. */
  active: boolean;
  /** 이 팬이 activePaneId인가 (Ctrl+Shift+W 등 "활성 팬" 단축키의 대상). */
  isFocusedPane: boolean;
  broadcastOn: boolean;
  /** broadcast 대상 세션 id 목록 (활성 탭의 paneIds). */
  broadcastTargetIds: string[];
  /** 이 팬이 지금 무장된 동시 입력의 대상인지. 화면에 그대로 표시한다. */
  isBroadcastTarget: boolean;
  /** 새 native/mux 세션에만 한 번 보내며, 기존 mux 재연결에는 전달하지 않는다. */
  initialCommand?: string;
  copyOnSelect: boolean;
  fontSize: number;
  fontFamily: string;
  theme: TerminalTheme;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
  scrollbackLines: number;
  /** 실제로 시작된 유지 방식. 요청과 다를 수 있으므로 팬에 그대로 표시한다. */
  multiplexer: MultiplexerKind;
  /** 기존 multiplexer 세션에 다시 붙었는지. */
  resumed: boolean;
  registerWrite: (id: string, fn: (data: string) => void) => void;
  unregisterWrite: (id: string) => void;
  registerFocus: (id: string, fn: () => void) => void;
  unregisterFocus: (id: string) => void;
  registerTerminalHandle: (id: string, handle: TerminalPaneHandle) => void;
  unregisterTerminalHandle: (id: string) => void;
  onClose: () => void;
  onFocusPane: () => void;
  onShortcut: (action: ShortcutAction) => void;
  onFontSizeChange: (fontSize: number) => void;
  onMetadataChange: (id: string, metadata: { title?: string; cwd?: string }) => void;
  onTerminalError: (message: string) => void;
  /** Disable broadcast at the owner when the backend rejects a stale target set. */
  onBroadcastFailure?: () => void;
  /** Windows build number for xterm's ConPTY soft-wrap heuristics, or null off Windows. */
  windowsBuildNumber: number | null;
  contextMenuTriggerProps: ContextMenuTriggerProps;
  actionsDisabled: boolean;
  /** In-app confirm/prompt. Replaces the native dialogs so focus, theme and IME match. */
  ask: AskDialog;
  /** Resolves the per-session host trust decision for an outbound link. */
  onConfirmLinkHost: (host: string) => Promise<boolean>;
  /** PaneCanvas가 display:none(비활성) 또는 order(활성 탭 안에서의 시각적 순서)를 준다. */
  style?: CSSProperties;
}

const SEARCH_DECORATIONS = {
  matchBackground: "#5c4b16",
  matchBorder: "#d29922",
  matchOverviewRuler: "#d29922",
  activeMatchBackground: "#264f78",
  activeMatchBorder: "#4f8cff",
  activeMatchColorOverviewRuler: "#4f8cff",
};

const RESIZE_DEBOUNCE_MS = 100;
const BELL_VISIBLE_MS = 4000;
const SELECTION_COPY_DEBOUNCE_MS = 120;

// FitAddon.proposeDimensions() clamps to a minimum of 1 row / 2 cols — it never
// produces 0, so a `rows <= 0 || cols <= 0` guard can never fire. These are the
// real usability floor: below this the shell would redraw into a near-nothing
// viewport and destroy whatever was on screen. See design §2.3.
const MIN_ROWS = 4;
const MIN_COLS = 20;

// Bounded so a confirmation left open cannot grow an unbounded input backlog.
const MAX_QUEUED_INPUT_CHUNKS = 256;

export default function TermPane({
  sessionId,
  title,
  active,
  isFocusedPane,
  broadcastOn,
  broadcastTargetIds,
  isBroadcastTarget,
  initialCommand,
  copyOnSelect,
  fontSize,
  fontFamily,
  theme,
  cursorStyle,
  cursorBlink,
  scrollbackLines,
  multiplexer,
  resumed,
  registerWrite,
  unregisterWrite,
  registerFocus,
  unregisterFocus,
  registerTerminalHandle,
  unregisterTerminalHandle,
  onClose,
  onFocusPane,
  onShortcut,
  onFontSizeChange,
  onMetadataChange,
  onTerminalError,
  onBroadcastFailure,
  windowsBuildNumber,
  contextMenuTriggerProps,
  actionsDisabled,
  ask,
  onConfirmLinkHost,
  style,
}: TermPaneProps) {
  const ref = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const { onContextMenu, ...menuTriggerProps } = contextMenuTriggerProps;
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResult, setSearchResult] = useState<ISearchResultChangeEvent>({
    resultIndex: -1,
    resultCount: 0,
  });
  const [bellAt, setBellAt] = useState<number | null>(null);
  const [searchOptions, setSearchOptions] = useState({
    caseSensitive: false,
    wholeWord: false,
    regex: false,
  });

  // 매 렌더마다 최신 값을 반영하는 ref들 — mount effect(아래)의 의존성 배열에는 넣지
  // 않는다. 넣으면 이 prop들이 바뀔 때마다 xterm이 재생성되어 스크롤백을 잃는다.
  const broadcastRef = useRef(broadcastOn);
  broadcastRef.current = broadcastOn;
  const broadcastTargetsRef = useRef(broadcastTargetIds);
  broadcastTargetsRef.current = broadcastTargetIds;
  const copyOnSelectRef = useRef(copyOnSelect);
  copyOnSelectRef.current = copyOnSelect;
  const fontSizeRef = useRef(fontSize);
  fontSizeRef.current = fontSize;
  const appearanceRef = useRef({ fontFamily, theme, cursorStyle, cursorBlink, scrollbackLines });
  appearanceRef.current = { fontFamily, theme, cursorStyle, cursorBlink, scrollbackLines };
  const onShortcutRef = useRef(onShortcut);
  onShortcutRef.current = onShortcut;
  const onFontSizeChangeRef = useRef(onFontSizeChange);
  onFontSizeChangeRef.current = onFontSizeChange;
  const onMetadataChangeRef = useRef(onMetadataChange);
  onMetadataChangeRef.current = onMetadataChange;
  const onTerminalErrorRef = useRef(onTerminalError);
  onTerminalErrorRef.current = onTerminalError;
  const onBroadcastFailureRef = useRef(onBroadcastFailure);
  onBroadcastFailureRef.current = onBroadcastFailure;
  const askRef = useRef(ask);
  askRef.current = ask;
  const onConfirmLinkHostRef = useRef(onConfirmLinkHost);
  onConfirmLinkHostRef.current = onConfirmLinkHost;

  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const searchAddonRef = useRef<SearchAddon | null>(null);
  const webglAddonRef = useRef<{ dispose: () => void } | null>(null);
  const cwdRef = useRef<string | null>(null);
  const lastAutoCopiedRef = useRef<string | null>(null);
  const selectionCopyTimerRef = useRef<number | undefined>(undefined);
  const lastSizeRef = useRef<{ rows: number; cols: number } | null>(null);
  const resizeTimerRef = useRef<number | undefined>(undefined);
  const fitAndSendResizeRef = useRef<() => void>(() => undefined);
  const seqRef = useRef(0);
  const broadcastPendingCommandRef = useRef("");
  const appliedFontSizeRef = useRef(clampTerminalFontSize(fontSize));
  // The in-app confirmation resolves asynchronously, so later keystrokes must not overtake
  // the one being confirmed. While a confirmation is open, input queues here in arrival
  // order and is replayed through the same dispatch path once it resolves.
  const bellTimerRef = useRef<number | undefined>(undefined);
  const confirmOpenRef = useRef(false);
  const queuedInputRef = useRef<string[]>([]);

  useEffect(() => {
    broadcastPendingCommandRef.current = "";
    // A target or mode change invalidates anything still waiting behind a confirmation.
    queuedInputRef.current = [];
  }, [broadcastOn, broadcastTargetIds.join("|")]);

  const copyText = useCallback(async (text: string, failureMessage: string) => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      onTerminalErrorRef.current(failureMessage);
    }
  }, []);

  const copySelection = useCallback(async () => {
    const selection = termRef.current?.getSelection() ?? "";
    if (!selection) return;
    await copyText(selection, "선택한 텍스트를 클립보드에 복사하지 못했습니다.");
  }, [copyText]);

  const pasteClipboard = useCallback(async () => {
    try {
      const text = await readClipboardText();
      if (!text) return;
      if (text.length > MAX_TERMINAL_PASTE_CHARACTERS) {
        onTerminalErrorRef.current("붙여넣을 내용이 1,000,000자를 초과합니다.");
        return;
      }
      if (hasMultilinePaste(text)) {
        const approved = await askRef.current({
          kind: "confirm",
          title: `${pasteLineCount(text)}줄을 터미널에 붙여넣을까요?`,
          lines: ["각 줄이 명령으로 실행될 수 있습니다."],
          confirmLabel: "붙여넣기",
          danger: true,
        });
        if (!approved.confirmed) {
          termRef.current?.focus();
          return;
        }
      }
      termRef.current?.paste(text);
      termRef.current?.focus();
    } catch {
      onTerminalErrorRef.current("클립보드 내용을 터미널에 붙여넣지 못했습니다.");
    }
  }, []);

  const openSearch = useCallback(() => {
    setSearchOpen(true);
    window.requestAnimationFrame(() => {
      searchInputRef.current?.focus();
      searchInputRef.current?.select();
    });
  }, []);

  const closeSearch = useCallback(() => {
    searchAddonRef.current?.clearDecorations();
    setSearchOpen(false);
    setSearchQuery("");
    setSearchResult({ resultIndex: -1, resultCount: 0 });
    window.requestAnimationFrame(() => termRef.current?.focus());
  }, []);

  const copyCwd = useCallback(async () => {
    if (!cwdRef.current) return;
    await copyText(cwdRef.current, "현재 경로를 클립보드에 복사하지 못했습니다.");
  }, [copyText]);

  const clearScrollback = useCallback(() => {
    const term = termRef.current;
    if (!term) return;
    // 화면에 보이는 줄은 남기고 위쪽 스크롤백만 버린다. 셸의 `clear`는 버퍼를 비우지 않는다.
    term.clear();
    term.focus();
  }, []);

  const scrollToBottom = useCallback(() => {
    termRef.current?.scrollToBottom();
    termRef.current?.focus();
  }, []);

  const selectAll = useCallback(() => {
    termRef.current?.selectAll();
  }, []);

  const getCapabilities = useCallback<TerminalPaneHandle["getCapabilities"]>(() => ({
    hasSelection: Boolean(termRef.current?.hasSelection()),
    hasCwd: cwdRef.current !== null,
  }), []);

  const openLinkRef = useRef<(input: string) => Promise<void>>(async () => undefined);
  openLinkRef.current = async (input: string) => {
    const url = normalizeTerminalLink(input);
    if (!url) {
      onTerminalErrorRef.current("지원하지 않는 링크 형식입니다.");
      return;
    }
    const host = new URL(url).hostname;
    if (!(await onConfirmLinkHostRef.current(host))) return;
    try {
      await openTerminalLink(url);
    } catch {
      onTerminalErrorRef.current("터미널 링크를 기본 브라우저에서 열지 못했습니다.");
    }
  };

  const terminalKeyActionRef = useRef<(action: TerminalKeyAction) => void>(() => undefined);
  terminalKeyActionRef.current = (action) => {
    switch (action) {
      case "copy":
        void copySelection();
        break;
      case "paste":
        void pasteClipboard();
        break;
      case "search":
        openSearch();
        break;
      case "font-increase":
        onFontSizeChangeRef.current(clampTerminalFontSize(fontSizeRef.current + 1));
        break;
      case "font-decrease":
        onFontSizeChangeRef.current(clampTerminalFontSize(fontSizeRef.current - 1));
        break;
      case "font-reset":
        onFontSizeChangeRef.current(DEFAULT_TERMINAL_FONT_SIZE);
        break;
    }
  };

  useEffect(() => {
    registerTerminalHandle(sessionId, {
      getCapabilities,
      copySelection,
      pasteClipboard,
      openSearch,
      copyCwd,
      clearScrollback,
      scrollToBottom,
      selectAll,
    });
    return () => unregisterTerminalHandle(sessionId);
  }, [
    clearScrollback,
    copyCwd,
    copySelection,
    getCapabilities,
    openSearch,
    pasteClipboard,
    registerTerminalHandle,
    scrollToBottom,
    selectAll,
    sessionId,
    unregisterTerminalHandle,
  ]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const initialFontSize = clampTerminalFontSize(fontSizeRef.current);
    appliedFontSizeRef.current = initialFontSize;
    const appearance = appearanceRef.current;
    const term = new Terminal({
      fontSize: initialFontSize,
      fontFamily: appearance.fontFamily,
      theme: appearance.theme,
      cursorStyle: appearance.cursorStyle,
      cursorBlink: appearance.cursorBlink,
      scrollback: appearance.scrollbackLines, // xterm 기본값(1000)보다 크게
      allowProposedApi: true, // Unicode11Addon 전제
      linkHandler: {
        activate: (event, text) => {
          event.preventDefault();
          void openLinkRef.current(text);
        },
      },
      // ConPTY는 오른쪽 여백에서 하드 랩할 때 랩 플래그를 주지 않는다. 이 옵션 없이는
      // 긴 줄이 전부 독립적인 하드 줄로 저장되고, cols가 바뀔 때마다 기존 출력이
      // 리플로우되지 않아 망가진다. Windows 빌드 번호는 소프트랩 휴리스틱에 필요하며,
      // 비-Windows에서는 null이라 backend만 지정한다.
      windowsPty: {
        backend: "conpty",
        ...(windowsBuildNumber === null ? {} : { buildNumber: windowsBuildNumber }),
      },
    });
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    const fit = new FitAddon();
    term.loadAddon(fit);
    const search = new SearchAddon();
    term.loadAddon(search);
    term.loadAddon(new WebLinksAddon((event, uri) => {
      event.preventDefault();
      void openLinkRef.current(uri);
    }));
    term.open(el);
    termRef.current = term;
    fitRef.current = fit;
    searchAddonRef.current = search;

    // WebGL 렌더러는 대량 출력과 전체 화면 TUI에서 기본 DOM 렌더러보다 훨씬 빠르다.
    // 첫 화면에는 필요 없으므로 별도 chunk로 나중에 불러오고, chunk 로딩·컨텍스트 생성·
    // 나중의 컨텍스트 손실 중 무엇이 실패하든 조용히 DOM 렌더러로 남는다 — 터미널 자체는
    // 어느 경우에도 계속 동작해야 하므로 실패를 오류로 올리지 않는다.
    let torndown = false;
    void import("@xterm/addon-webgl")
      .then(({ WebglAddon }) => {
        if (torndown || termRef.current !== term) return;
        const webgl = new WebglAddon();
        webgl.onContextLoss(() => {
          webgl.dispose();
          if (webglAddonRef.current === webgl) webglAddonRef.current = null;
        });
        term.loadAddon(webgl);
        webglAddonRef.current = webgl;
      })
      .catch(() => {
        webglAddonRef.current = null;
      });

    // fit() 후 실제 rows/cols를 PTY에 전달한다. display:none인 동안(비활성 탭)에는
    // fit()이 0 크기를 계산하므로 건너뛴다 — 그 상태에서도 lastSizeRef는 마지막 실측
    // 크기를 유지하므로, 다시 보일 때 같은 크기면 재전송하지 않는다.
    const fitAndSendResize = () => {
      try {
        fit.fit();
      } catch {
        return;
      }
      const { rows, cols } = term;
      // FitAddon은 최소 1행 2열로 clamp하므로 `rows <= 0 || cols <= 0`는 절대 발동하지
      // 않는 죽은 코드였다 — 그 크기의 resize가 그대로 ConPTY까지 전달되어 셸이 다시
      // 그리면 화면 내용이 영구 파괴된다. 실사용 바닥값으로 교체한다. 바닥값 미만이면
      // 전송하지 않고 lastSizeRef도 갱신하지 않는다 — 팬이 다시 커졌을 때 재전송되어야
      // 하기 때문이다.
      if (rows < MIN_ROWS || cols < MIN_COLS) return;
      const last = lastSizeRef.current;
      if (last && last.rows === rows && last.cols === cols) return;
      // ack 후 커밋: lastSizeRef를 IPC 전에 낙관적으로 기록하지 않는다. resize가
      // 유실·거부되면 다음 fitAndSendResize가 재시도해야 하므로, 성공 응답이 와야만
      // (그리고 그 사이 더 최신 resize가 나가지 않았을 때만) 커밋한다.
      const seq = ++seqRef.current;
      resizeSession(sessionId, rows, cols)
        .then(() => {
          if (seq === seqRef.current) lastSizeRef.current = { rows, cols };
        })
        .catch(() => {
          /* 커밋하지 않음 → 다음 fit이 재시도한다 */
        });
    };
    fitAndSendResizeRef.current = fitAndSendResize;
    fitAndSendResize();

    // 터미널 로컬 UX 단축키를 먼저 처리한 뒤 앱 단축키를 처리한다. 매칭되면
    // window 레벨 리스너까지 버블링되지 않게 막아 중복 실행을 방지한다.
    term.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown") return true;
      const terminalAction = matchTerminalKey(event);
      if (terminalAction) {
        event.preventDefault();
        event.stopPropagation();
        terminalKeyActionRef.current(terminalAction);
        return false;
      }
      const action = matchShortcut(event);
      if (!action) return true;
      event.preventDefault();
      event.stopPropagation();
      onShortcutRef.current(action);
      return false;
    });

    const sendBroadcast = (targets: string[], data: string) => {
      void broadcast(targets, data).catch(() => {
        broadcastPendingCommandRef.current = "";
        onBroadcastFailureRef.current?.();
        onTerminalErrorRef.current("broadcast 입력을 모든 대상 터미널에 전달하지 못했습니다.");
      });
    };

    const flushQueuedInput = () => {
      while (!confirmOpenRef.current && queuedInputRef.current.length > 0) {
        dispatchInput(queuedInputRef.current.shift() as string);
      }
    };

    const dispatchInput = (data: string) => {
      const targets = broadcastTargetsRef.current;
      if (!broadcastRef.current || targets.length < 2) {
        broadcastPendingCommandRef.current = "";
        void writeSession(sessionId, data).catch(() => {
          onTerminalErrorRef.current("터미널 입력을 전달하지 못했습니다.");
        });
        return;
      }
      const assessment = assessBroadcastInput(data, broadcastPendingCommandRef.current, targets.length);
      if (!assessment.confirmation) {
        broadcastPendingCommandRef.current = assessment.nextPendingCommand;
        sendBroadcast(targets, data);
        return;
      }
      confirmOpenRef.current = true;
      void askRef.current({
        kind: "confirm",
        title: assessment.confirmation,
        confirmLabel: "보내기",
        danger: true,
      }).then((approved) => {
        confirmOpenRef.current = false;
        if (approved.confirmed) {
          broadcastPendingCommandRef.current = assessment.nextPendingCommand;
          sendBroadcast(targets, data);
        }
        flushQueuedInput();
      });
    };

    const dataDisposable = term.onData((data) => {
      // Hold input in arrival order behind an open confirmation. The dialog takes focus so
      // this is rare, but a keystroke landing in the frame before it renders must not be
      // delivered ahead of the chunk still awaiting approval.
      if (confirmOpenRef.current) {
        if (queuedInputRef.current.length < MAX_QUEUED_INPUT_CHUNKS) queuedInputRef.current.push(data);
        return;
      }
      dispatchInput(data);
    });
    const selectionDisposable = term.onSelectionChange(() => {
      window.clearTimeout(selectionCopyTimerRef.current);
      const selection = term.getSelection();
      if (!selection) {
        lastAutoCopiedRef.current = null;
        return;
      }
      if (!copyOnSelectRef.current || selection === lastAutoCopiedRef.current) return;
      selectionCopyTimerRef.current = window.setTimeout(() => {
        const settledSelection = term.getSelection();
        if (!copyOnSelectRef.current || !settledSelection || settledSelection !== selection) return;
        lastAutoCopiedRef.current = settledSelection;
        void copyText(settledSelection, "선택한 텍스트를 클립보드에 복사하지 못했습니다.");
      }, SELECTION_COPY_DEBOUNCE_MS);
    });
    const titleDisposable = term.onTitleChange((nextTitle) => {
      const normalized = normalizeTerminalTitle(nextTitle);
      if (normalized) onMetadataChangeRef.current(sessionId, { title: normalized });
    });
    const osc7Disposable = term.parser.registerOscHandler(7, (payload) => {
      const nextCwd = parseOsc7Cwd(payload);
      if (nextCwd) {
        cwdRef.current = nextCwd;
        onMetadataChangeRef.current(sessionId, { cwd: nextCwd });
      }
      return true;
    });
    const searchResultDisposable = search.onDidChangeResults((result) => setSearchResult(result));
    // 벨은 소리 대신 팬 머리글을 잠깐 표시한다. 비활성 탭에서도 남아 있어 무엇이 울렸는지
    // 놓치지 않는다.
    const bellDisposable = term.onBell(() => {
      setBellAt(Date.now());
      window.clearTimeout(bellTimerRef.current);
      bellTimerRef.current = window.setTimeout(() => setBellAt(null), BELL_VISIBLE_MS);
    });

    registerWrite(sessionId, (data: string) => term.write(data));
    // registerWrite 직후, 마운트당 정확히 한 번. 리더 스레드는 start_session이 아니라
    // 이 호출로 spawn된다 — 그 사이 출력은 ConPTY 내부 버퍼가 보관하므로 프론트
    // 핸들러가 등록되기 전에 나온 바이트가 유실되지 않는다.
    void attachSession(sessionId);
    if (initialCommand) {
      void writeSession(sessionId, `${initialCommand}\r`).catch(() => {
        onTerminalErrorRef.current("프로필 시작 명령을 터미널에 전달하지 못했습니다.");
      });
    }

    const ro = new ResizeObserver(() => {
      window.clearTimeout(resizeTimerRef.current);
      resizeTimerRef.current = window.setTimeout(fitAndSendResize, RESIZE_DEBOUNCE_MS);
    });
    ro.observe(el);

    // 초기 프롬프트
    term.write("\x1b[2J\x1b[H");

    return () => {
      torndown = true;
      window.clearTimeout(selectionCopyTimerRef.current);
      window.clearTimeout(resizeTimerRef.current);
      confirmOpenRef.current = false;
      queuedInputRef.current = [];
      ro.disconnect();
      dataDisposable.dispose();
      selectionDisposable.dispose();
      titleDisposable.dispose();
      osc7Disposable.dispose();
      searchResultDisposable.dispose();
      bellDisposable.dispose();
      window.clearTimeout(bellTimerRef.current);
      unregisterWrite(sessionId);
      cwdRef.current = null;
      termRef.current = null;
      fitRef.current = null;
      searchAddonRef.current = null;
      // xterm보다 먼저 정리해야 addon이 이미 사라진 renderer에 접근하지 않는다.
      webglAddonRef.current?.dispose();
      webglAddonRef.current = null;
      term.dispose();
    };
  }, [initialCommand, sessionId, registerWrite, unregisterWrite]);

  // focus registry callback identity가 바뀌어도 xterm 자체를 재생성하지 않는다. terminal
  // 인스턴스는 mount effect가 소유하고 이 effect는 최신 registry에 focus handle만 연결한다.
  useEffect(() => {
    registerFocus(sessionId, () => termRef.current?.focus());
    return () => unregisterFocus(sessionId);
  }, [registerFocus, sessionId, unregisterFocus]);

  // 탭이 다시 보일 때(active: false → true) 실제 크기로 다시 맞춘다. ResizeObserver가
  // display:none → 보임 전환에서 항상 안정적으로 발화한다고 보장할 수 없으므로,
  // 여기서 명시적으로 한 번 더 호출한다.
  useEffect(() => {
    if (!active) return;
    // 대기 중인 ResizeObserver 디바운스(100ms)가 이 rAF와 경합해 순서가 뒤바뀐 resize를
    // 낼 수 있다 — 먼저 취소해 두 경로가 절대 겹치지 않게 한다.
    window.clearTimeout(resizeTimerRef.current);
    const raf = requestAnimationFrame(() => fitAndSendResizeRef.current());
    return () => cancelAnimationFrame(raf);
  }, [active]);

  // 옵션만 갱신해 스크롤백과 PTY 연결을 유지한다. 글꼴 변화 뒤의 새 rows/cols는 기존
  // ack-after-commit resize 경로로만 전달한다.
  useEffect(() => {
    const next = clampTerminalFontSize(fontSize);
    const term = termRef.current;
    if (!term || next === appliedFontSizeRef.current) return;
    appliedFontSizeRef.current = next;
    term.options.fontSize = next;
    window.clearTimeout(resizeTimerRef.current);
    const raf = requestAnimationFrame(() => fitAndSendResizeRef.current());
    return () => cancelAnimationFrame(raf);
  }, [fontSize]);

  // 글꼴·테마·커서·스크롤백은 옵션만 갱신한다. xterm을 재생성하면 스크롤백과 PTY 연결이
  // 함께 사라지므로, 크기가 달라지는 글꼴만 기존 ack-after-commit resize 경로를 다시 탄다.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    const changesCellSize = term.options.fontFamily !== fontFamily;
    term.options.fontFamily = fontFamily;
    term.options.theme = theme;
    term.options.cursorStyle = cursorStyle;
    term.options.cursorBlink = cursorBlink;
    term.options.scrollback = scrollbackLines;
    if (!changesCellSize) return;
    window.clearTimeout(resizeTimerRef.current);
    const raf = requestAnimationFrame(() => fitAndSendResizeRef.current());
    return () => cancelAnimationFrame(raf);
  }, [cursorBlink, cursorStyle, fontFamily, scrollbackLines, theme]);

  // 활성 팬이 바뀌면(클릭 또는 Alt+Arrow 이동) xterm에 키보드 포커스를 준다.
  // 포커스 이동 없이 activePaneId만 바뀌면 입력이 이전 팬에 남는다.
  useEffect(() => {
    if (active && isFocusedPane && !searchOpen) {
      termRef.current?.focus();
    }
  }, [active, isFocusedPane, searchOpen]);

  const runSearch = (
    direction: "next" | "previous",
    query = searchQuery,
    modifiers = searchOptions,
  ) => {
    const addon = searchAddonRef.current;
    if (!addon) return;
    const boundedQuery = query.slice(0, MAX_TERMINAL_SEARCH_CHARACTERS);
    if (!boundedQuery) {
      addon.clearDecorations();
      setSearchResult({ resultIndex: -1, resultCount: 0 });
      return;
    }
    // decorations를 주면 addon이 현재 일치뿐 아니라 버퍼 전체의 일치를 표시한다.
    const options = {
      decorations: SEARCH_DECORATIONS,
      incremental: direction === "next",
      caseSensitive: modifiers.caseSensitive,
      wholeWord: modifiers.wholeWord,
      regex: modifiers.regex,
    };
    if (direction === "next") addon.findNext(boundedQuery, options);
    else addon.findPrevious(boundedQuery, options);
  };

  const toggleSearchOption = (key: keyof typeof searchOptions) => {
    const next = { ...searchOptions, [key]: !searchOptions[key] };
    setSearchOptions(next);
    runSearch("next", searchQuery, next);
  };

  return (
    <div
      className={`pane ${isFocusedPane ? "pane-focused" : ""} ${isBroadcastTarget ? "pane-broadcast-target" : ""}`}
      style={style}
      // 팬은 터미널과 그 컨트롤을 묶은 영역이다. role 없는 div에는 컨텍스트 메뉴 trigger가
      // 붙이는 aria-haspopup/aria-expanded를 쓸 수 없다.
      role="group"
      tabIndex={-1}
      data-pane-id={sessionId}
      aria-label={isBroadcastTarget ? `${title} 터미널 팬 · 동시 입력 대상` : `${title} 터미널 팬`}
      onMouseDownCapture={onFocusPane}
      // xterm 내부 handler가 bubble을 중단해도 pane menu가 먼저 열리도록 capture에서 받는다.
      onContextMenuCapture={onContextMenu}
      {...menuTriggerProps}
    >
      <div
        className="pane-head"
        draggable
        onDragStart={(event) => {
          event.dataTransfer.setData("application/x-wsld-pane", sessionId);
          event.dataTransfer.effectAllowed = "move";
        }}
      >
        <span className="pane-title" title={title}>{title}</span>
        {multiplexer !== "native" && (
          <span className="pane-badge" title={`이 팬은 ${multiplexer} 세션으로 실행 중입니다`}>
            {multiplexer}
          </span>
        )}
        {bellAt !== null && (
          <span className="pane-badge bell" role="status" title="터미널이 벨 문자를 보냈습니다">
            벨
          </span>
        )}
        {isBroadcastTarget && (
          <span className="pane-badge broadcast" title="동시 입력이 이 팬에도 전달됩니다">
            동시 입력
          </span>
        )}
        {resumed && (
          <span className="pane-badge resumed" title="기존 세션에 다시 연결했습니다. 시작 명령은 다시 실행하지 않았습니다.">
            재연결됨
          </span>
        )}
        <button className="pane-close" title="터미널 닫기" disabled={actionsDisabled} onClick={onClose}>
          ✕
        </button>
      </div>
      {searchOpen && (
        <div className="pane-search" role="search" aria-label="터미널 출력 검색">
          <input
            ref={searchInputRef}
            aria-label="검색어"
            maxLength={MAX_TERMINAL_SEARCH_CHARACTERS}
            value={searchQuery}
            onChange={(event) => {
              const query = event.currentTarget.value.slice(0, MAX_TERMINAL_SEARCH_CHARACTERS);
              setSearchQuery(query);
              runSearch("next", query);
            }}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                closeSearch();
              } else if (event.key === "Enter") {
                event.preventDefault();
                runSearch(event.shiftKey ? "previous" : "next");
              }
            }}
            placeholder="터미널 출력 검색"
          />
          <button
            type="button"
            className={`search-option ${searchOptions.caseSensitive ? "active" : ""}`}
            aria-pressed={searchOptions.caseSensitive}
            title="대/소문자 구분"
            onClick={() => toggleSearchOption("caseSensitive")}
          >Aa</button>
          <button
            type="button"
            className={`search-option ${searchOptions.wholeWord ? "active" : ""}`}
            aria-pressed={searchOptions.wholeWord}
            title="단어 단위로 일치"
            onClick={() => toggleSearchOption("wholeWord")}
          >ab|</button>
          <button
            type="button"
            className={`search-option ${searchOptions.regex ? "active" : ""}`}
            aria-pressed={searchOptions.regex}
            title="정규식으로 검색"
            onClick={() => toggleSearchOption("regex")}
          >.*</button>
          <span className="search-count" aria-live="polite">
            {searchResult.resultCount > 0 ? `${searchResult.resultIndex + 1}/${searchResult.resultCount}` : "0/0"}
          </span>
          <button type="button" title="이전 결과 (Shift+Enter)" onClick={() => runSearch("previous")}>↑</button>
          <button type="button" title="다음 결과 (Enter)" onClick={() => runSearch("next")}>↓</button>
          <button type="button" title="검색 닫기 (Esc)" onClick={closeSearch}>✕</button>
        </div>
      )}
      <div
        className="term-wrap"
        ref={ref}
        onMouseDown={(event) => {
          if (event.button === 1) event.preventDefault();
        }}
        onAuxClick={(event) => {
          if (event.button !== 1) return;
          event.preventDefault();
          void pasteClipboard();
        }}
      />
    </div>
  );
}
