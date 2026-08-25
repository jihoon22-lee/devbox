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
  /** 새 native/mux 세션에만 한 번 보내며, 기존 mux 재연결에는 전달하지 않는다. */
  initialCommand?: string;
  copyOnSelect: boolean;
  fontSize: number;
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
  /** Windows build number for xterm's ConPTY soft-wrap heuristics, or null off Windows. */
  windowsBuildNumber: number | null;
  contextMenuTriggerProps: ContextMenuTriggerProps;
  actionsDisabled: boolean;
  /** PaneCanvas가 display:none(비활성) 또는 order(활성 탭 안에서의 시각적 순서)를 준다. */
  style?: CSSProperties;
}

const THEME = {
  background: "#111418",
  foreground: "#e6e9ef",
  cursor: "#4f8cff",
  selectionBackground: "#264f78",
};

const SEARCH_DECORATIONS = {
  matchBackground: "#5c4b16",
  matchBorder: "#d29922",
  matchOverviewRuler: "#d29922",
  activeMatchBackground: "#264f78",
  activeMatchBorder: "#4f8cff",
  activeMatchColorOverviewRuler: "#4f8cff",
};

const RESIZE_DEBOUNCE_MS = 100;
const SELECTION_COPY_DEBOUNCE_MS = 120;

// FitAddon.proposeDimensions() clamps to a minimum of 1 row / 2 cols — it never
// produces 0, so a `rows <= 0 || cols <= 0` guard can never fire. These are the
// real usability floor: below this the shell would redraw into a near-nothing
// viewport and destroy whatever was on screen. See design §2.3.
const MIN_ROWS = 4;
const MIN_COLS = 20;

export default function TermPane({
  sessionId,
  title,
  active,
  isFocusedPane,
  broadcastOn,
  broadcastTargetIds,
  initialCommand,
  copyOnSelect,
  fontSize,
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
  windowsBuildNumber,
  contextMenuTriggerProps,
  actionsDisabled,
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
  const onShortcutRef = useRef(onShortcut);
  onShortcutRef.current = onShortcut;
  const onFontSizeChangeRef = useRef(onFontSizeChange);
  onFontSizeChangeRef.current = onFontSizeChange;
  const onMetadataChangeRef = useRef(onMetadataChange);
  onMetadataChangeRef.current = onMetadataChange;
  const onTerminalErrorRef = useRef(onTerminalError);
  onTerminalErrorRef.current = onTerminalError;

  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const searchAddonRef = useRef<SearchAddon | null>(null);
  const cwdRef = useRef<string | null>(null);
  const lastAutoCopiedRef = useRef<string | null>(null);
  const selectionCopyTimerRef = useRef<number | undefined>(undefined);
  const lastSizeRef = useRef<{ rows: number; cols: number } | null>(null);
  const resizeTimerRef = useRef<number | undefined>(undefined);
  const fitAndSendResizeRef = useRef<() => void>(() => undefined);
  const seqRef = useRef(0);
  const broadcastPendingCommandRef = useRef("");
  const appliedFontSizeRef = useRef(clampTerminalFontSize(fontSize));

  useEffect(() => {
    broadcastPendingCommandRef.current = "";
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
      if (
        hasMultilinePaste(text)
        && !window.confirm(`${pasteLineCount(text)}줄을 터미널에 붙여넣을까요? 명령이 실행될 수 있습니다.`)
      ) {
        termRef.current?.focus();
        return;
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
    if (!window.confirm(`'${host}' 링크를 기본 브라우저에서 열까요?`)) return;
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
    });
    return () => unregisterTerminalHandle(sessionId);
  }, [copyCwd, copySelection, getCapabilities, openSearch, pasteClipboard, registerTerminalHandle, sessionId, unregisterTerminalHandle]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const initialFontSize = clampTerminalFontSize(fontSizeRef.current);
    appliedFontSizeRef.current = initialFontSize;
    const term = new Terminal({
      fontSize: initialFontSize,
      fontFamily: '"Cascadia Code", Consolas, monospace',
      theme: THEME,
      cursorBlink: true,
      scrollback: 10000, // xterm 기본값(1000)보다 크게
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

    const dataDisposable = term.onData((data) => {
      const targets = broadcastTargetsRef.current;
      if (broadcastRef.current && targets.length >= 2) {
        const assessment = assessBroadcastInput(data, broadcastPendingCommandRef.current, targets.length);
        if (assessment.confirmation && !window.confirm(assessment.confirmation)) return;
        broadcastPendingCommandRef.current = assessment.nextPendingCommand;
        void broadcast(targets, data).catch(() => {
          onTerminalErrorRef.current("broadcast 입력을 모든 대상 터미널에 전달하지 못했습니다.");
        });
      } else {
        broadcastPendingCommandRef.current = "";
        void writeSession(sessionId, data).catch(() => {
          onTerminalErrorRef.current("터미널 입력을 전달하지 못했습니다.");
        });
      }
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
      window.clearTimeout(selectionCopyTimerRef.current);
      window.clearTimeout(resizeTimerRef.current);
      ro.disconnect();
      dataDisposable.dispose();
      selectionDisposable.dispose();
      titleDisposable.dispose();
      osc7Disposable.dispose();
      searchResultDisposable.dispose();
      unregisterWrite(sessionId);
      cwdRef.current = null;
      termRef.current = null;
      fitRef.current = null;
      searchAddonRef.current = null;
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

  // 활성 팬이 바뀌면(클릭 또는 Alt+Arrow 이동) xterm에 키보드 포커스를 준다.
  // 포커스 이동 없이 activePaneId만 바뀌면 입력이 이전 팬에 남는다.
  useEffect(() => {
    if (active && isFocusedPane && !searchOpen) {
      termRef.current?.focus();
    }
  }, [active, isFocusedPane, searchOpen]);

  const runSearch = (direction: "next" | "previous", query = searchQuery) => {
    const addon = searchAddonRef.current;
    if (!addon) return;
    const boundedQuery = query.slice(0, MAX_TERMINAL_SEARCH_CHARACTERS);
    if (!boundedQuery) {
      addon.clearDecorations();
      setSearchResult({ resultIndex: -1, resultCount: 0 });
      return;
    }
    const options = { decorations: SEARCH_DECORATIONS, incremental: direction === "next" };
    if (direction === "next") addon.findNext(boundedQuery, options);
    else addon.findPrevious(boundedQuery, options);
  };

  return (
    <div
      className={`pane ${isFocusedPane ? "pane-focused" : ""}`}
      style={style}
      tabIndex={-1}
      data-pane-id={sessionId}
      aria-label={`${title} 터미널 팬`}
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
