import { useEffect, useRef, type CSSProperties } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { broadcast, resizeSession, writeSession } from "../api";
import { matchShortcut, type ShortcutAction } from "../lib/shortcuts";

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
  registerWrite: (id: string, fn: (data: string) => void) => void;
  unregisterWrite: (id: string) => void;
  onClose: () => void;
  onFocusPane: () => void;
  onShortcut: (action: ShortcutAction) => void;
  /** PaneCanvas가 display:none(비활성) 또는 order(활성 탭 안에서의 시각적 순서)를 준다. */
  style?: CSSProperties;
}

const THEME = {
  background: "#111418",
  foreground: "#e6e9ef",
  cursor: "#4f8cff",
  selectionBackground: "#264f78",
};

const RESIZE_DEBOUNCE_MS = 100;

export default function TermPane({
  sessionId,
  title,
  active,
  isFocusedPane,
  broadcastOn,
  broadcastTargetIds,
  registerWrite,
  unregisterWrite,
  onClose,
  onFocusPane,
  onShortcut,
  style,
}: TermPaneProps) {
  const ref = useRef<HTMLDivElement>(null);

  // 매 렌더마다 최신 값을 반영하는 ref들 — mount effect(아래)의 의존성 배열에는 넣지
  // 않는다. 넣으면 이 prop들이 바뀔 때마다 xterm이 재생성되어 스크롤백을 잃는다.
  const broadcastRef = useRef(broadcastOn);
  broadcastRef.current = broadcastOn;
  const broadcastTargetsRef = useRef(broadcastTargetIds);
  broadcastTargetsRef.current = broadcastTargetIds;
  const onShortcutRef = useRef(onShortcut);
  onShortcutRef.current = onShortcut;

  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const lastSizeRef = useRef<{ rows: number; cols: number } | null>(null);
  const resizeTimerRef = useRef<number | undefined>(undefined);
  const fitAndSendResizeRef = useRef<() => void>(() => undefined);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const term = new Terminal({
      fontSize: 13,
      fontFamily: '"Cascadia Code", Consolas, monospace',
      theme: THEME,
      cursorBlink: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    termRef.current = term;
    fitRef.current = fit;

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
      if (rows <= 0 || cols <= 0) return;
      const last = lastSizeRef.current;
      if (last && last.rows === rows && last.cols === cols) return;
      lastSizeRef.current = { rows, cols };
      void resizeSession(sessionId, rows, cols);
    };
    fitAndSendResizeRef.current = fitAndSendResize;
    fitAndSendResize();

    // Windows Terminal 호환 단축키를 셸로 보내기 전에 가로챈다. keydown/keyup 모두
    // 불리므로 keydown만 처리한다. 매칭되면 stopPropagation으로 window 레벨
    // keydown 리스너(App.tsx)까지 버블링되지 않게 막아 중복 실행을 막는다.
    term.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown") return true;
      const action = matchShortcut(event);
      if (!action) return true;
      event.preventDefault();
      event.stopPropagation();
      onShortcutRef.current(action);
      return false;
    });

    const dataDisposable = term.onData((data) => {
      if (broadcastRef.current) {
        void broadcast(broadcastTargetsRef.current, data);
      } else {
        void writeSession(sessionId, data);
      }
    });

    registerWrite(sessionId, (data: string) => term.write(data));

    const ro = new ResizeObserver(() => {
      window.clearTimeout(resizeTimerRef.current);
      resizeTimerRef.current = window.setTimeout(fitAndSendResize, RESIZE_DEBOUNCE_MS);
    });
    ro.observe(el);

    // 초기 프롬프트
    term.write("\x1b[2J\x1b[H");

    return () => {
      window.clearTimeout(resizeTimerRef.current);
      ro.disconnect();
      dataDisposable.dispose();
      unregisterWrite(sessionId);
      termRef.current = null;
      fitRef.current = null;
      term.dispose();
    };
  }, [sessionId, registerWrite, unregisterWrite]);

  // 탭이 다시 보일 때(active: false → true) 실제 크기로 다시 맞춘다. ResizeObserver가
  // display:none → 보임 전환에서 항상 안정적으로 발화한다고 보장할 수 없으므로,
  // 여기서 명시적으로 한 번 더 호출한다.
  useEffect(() => {
    if (!active) return;
    const raf = requestAnimationFrame(() => fitAndSendResizeRef.current());
    return () => cancelAnimationFrame(raf);
  }, [active]);

  return (
    <div className={`pane ${isFocusedPane ? "pane-focused" : ""}`} style={style} onMouseDownCapture={onFocusPane}>
      <div
        className="pane-head"
        draggable
        onDragStart={(e) => {
          e.dataTransfer.setData("application/x-wsld-pane", sessionId);
          e.dataTransfer.effectAllowed = "move";
        }}
      >
        <span className="pane-title">{title}</span>
        <button className="pane-close" title="Close terminal" onClick={onClose}>
          ✕
        </button>
      </div>
      <div className="term-wrap" ref={ref} />
    </div>
  );
}
