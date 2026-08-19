import { useEffect, useRef, type CSSProperties } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import "@xterm/xterm/css/xterm.css";
import { attachSession, broadcast, resizeSession, writeSession } from "../api";
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
  /** Windows build number for xterm's ConPTY soft-wrap heuristics, or null off Windows. */
  windowsBuildNumber: number | null;
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
  registerWrite,
  unregisterWrite,
  onClose,
  onFocusPane,
  onShortcut,
  windowsBuildNumber,
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
  const seqRef = useRef(0);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const term = new Terminal({
      fontSize: 13,
      fontFamily: '"Cascadia Code", Consolas, monospace',
      theme: THEME,
      cursorBlink: true,
      scrollback: 10000, // xterm 기본값(1000)보다 크게
      allowProposedApi: true, // Unicode11Addon 전제
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
    // registerWrite 직후, 마운트당 정확히 한 번. 리더 스레드는 start_session이 아니라
    // 이 호출로 spawn된다 — 그 사이 출력은 ConPTY 내부 버퍼가 보관하므로 프론트
    // 핸들러가 등록되기 전에 나온 바이트가 유실되지 않는다.
    void attachSession(sessionId);

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
    // 대기 중인 ResizeObserver 디바운스(100ms)가 이 rAF와 경합해 순서가 뒤바뀐 resize를
    // 낼 수 있다 — 먼저 취소해 두 경로가 절대 겹치지 않게 한다.
    window.clearTimeout(resizeTimerRef.current);
    const raf = requestAnimationFrame(() => fitAndSendResizeRef.current());
    return () => cancelAnimationFrame(raf);
  }, [active]);

  // 활성 팬이 바뀌면(클릭 또는 Alt+Arrow 이동) xterm에 키보드 포커스를 준다.
  // 포커스 이동 없이 activePaneId만 바뀌면 입력이 이전 팬에 남는다.
  useEffect(() => {
    if (active && isFocusedPane) {
      termRef.current?.focus();
    }
  }, [active, isFocusedPane]);

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
