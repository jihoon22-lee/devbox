import type { CSSProperties } from "react";
import type { ContextMenuTriggerProps } from "@devbox/context-menu";
import type { AskDialog } from "./AppDialog";
import type { CursorStyle, TerminalTheme } from "../lib/settings";
import TermPane, { type TerminalPaneHandle } from "./TermPane";
import type { Pane, Tab } from "../types";
import type { ShortcutAction } from "../lib/shortcuts";

interface PaneCanvasProps {
  tabs: Tab[];
  panes: Pane[];
  activeTabId: string;
  activePaneId: string | null;
  broadcastOn: boolean;
  broadcastTargetIds: string[];
  copyOnSelect: boolean;
  fontSize: number;
  fontFamily: string;
  theme: TerminalTheme;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
  scrollbackLines: number;
  registerWrite: (id: string, fn: (data: string) => void) => void;
  unregisterWrite: (id: string) => void;
  registerFocus: (id: string, fn: () => void) => void;
  unregisterFocus: (id: string) => void;
  registerTerminalHandle: (id: string, handle: TerminalPaneHandle) => void;
  unregisterTerminalHandle: (id: string) => void;
  onClosePane: (id: string) => void;
  onFocusPane: (id: string) => void;
  onShortcut: (action: ShortcutAction) => void;
  onFontSizeChange: (fontSize: number) => void;
  onMetadataChange: (id: string, metadata: { title?: string; cwd?: string }) => void;
  onTerminalError: (message: string) => void;
  onBroadcastFailure?: () => void;
  windowsBuildNumber: number | null;
  contextMenuTriggerProps: ContextMenuTriggerProps;
  actionsDisabled: boolean;
  ask: AskDialog;
  onConfirmLinkHost: (host: string) => Promise<boolean>;
}

/**
 * 모든 팬(TermPane)을 `panes` 배열 순서 그대로, 항상 같은 부모(.panes) 아래
 * 마운트한 채로 둔다. React portal은 쓰지 않는다 — portal은 DOM 출력 위치만 바꿀
 * 뿐 React 엘리먼트 트리상의 "부모"(그 portal을 반환한 JSX 위치)는 그대로다.
 * 팬을 활성 탭용 `.map()`에서 비활성 탭용 `.map()`으로 옮기면, 같은 key라도
 * "부모 children 배열"이 바뀌는 셈이라 React가 이전 부모에서는 그 key를 더 이상
 * 찾지 못해 언마운트하고 새 부모에서는 처음 보는 key로 취급해 새로 마운트한다
 * (portal의 containerInfo가 바뀌는 것도 별도로 같은 결과를 유발한다). 이 구현을
 * 브라우저에서 실측해 실제로 재마운트됨을 먼저 확인한 뒤 이 방식으로 고쳤다.
 *
 * 대신 비활성 탭의 팬은 CSS display:none으로 숨기고, 보이는 팬들의 화면 순서는
 * CSS order로만 준다. `panes` 배열 자체의 순서는 절대 바뀌지 않는다(App.tsx는
 * 추가는 append, 제거는 filter만 쓴다) — React가 fiber를 옮길 일이 구조적으로
 * 없어야 재마운트도 없다.
 */
export default function PaneCanvas({
  tabs,
  panes,
  activeTabId,
  activePaneId,
  broadcastOn,
  broadcastTargetIds,
  copyOnSelect,
  fontSize,
  fontFamily,
  theme,
  cursorStyle,
  cursorBlink,
  scrollbackLines,
  registerWrite,
  unregisterWrite,
  registerFocus,
  unregisterFocus,
  registerTerminalHandle,
  unregisterTerminalHandle,
  onClosePane,
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
}: PaneCanvasProps) {
  const activeTab = tabs.find((t) => t.id === activeTabId) ?? null;
  const activePaneIds = activeTab?.paneIds ?? [];

  const gridCols = activePaneIds.length === 0 ? 1 : Math.ceil(Math.sqrt(activePaneIds.length));
  const gridRows = Math.ceil(activePaneIds.length / gridCols);
  const layout = activeTab?.layout ?? "grid";
  const gridStyle: CSSProperties =
    layout === "cols"
      ? { gridTemplateColumns: `repeat(${Math.max(1, activePaneIds.length)}, 1fr)` }
      : layout === "rows"
        ? { gridTemplateRows: `repeat(${Math.max(1, activePaneIds.length)}, 1fr)` }
        : {
            // activePaneIds.length === 0일 때 gridRows도 0이 되어 `repeat(0, 1fr)`은 무효
            // CSS라 이전 렌더의 gridTemplateRows가 그대로 남는다. cols/rows 분기엔 있던
            // Math.max(1, …) 가드가 #189에서 이 grid 분기에만 누락됐었다.
            gridTemplateColumns: `repeat(${Math.max(1, gridCols)}, 1fr)`,
            gridTemplateRows: `repeat(${Math.max(1, gridRows)}, 1fr)`,
          };

  return (
    <div className="panes" style={gridStyle}>
      {panes.map((pane) => {
        // 세션이 아직 붙지 않은 팬(sessionId === null)은 TermPane을 마운트할 것이 없다
        // — 지금 이 릴리스에서는 startSession 성공 후에만 팬이 생기므로 실질적으로는
        // 항상 non-null이지만, 타입이 허용하는 미래(레이아웃 복원)를 위해 가드해 둔다.
        // panes 배열 자체는 건드리지 않으므로(그냥 렌더를 건너뜀) 순서 불변식은 유지된다.
        if (pane.sessionId === null) return null;
        const sessionId = pane.sessionId;
        const order = activePaneIds.indexOf(sessionId);
        const active = order !== -1;
        return (
          <TermPane
            key={pane.key}
            sessionId={sessionId}
            title={pane.title?.trim() || pane.distro}
            active={active}
            isFocusedPane={sessionId === activePaneId}
            broadcastOn={broadcastOn}
            broadcastTargetIds={broadcastTargetIds}
            isBroadcastTarget={broadcastOn && broadcastTargetIds.includes(sessionId)}
            initialCommand={pane.initialCommand}
            copyOnSelect={copyOnSelect}
            fontSize={fontSize}
            fontFamily={fontFamily}
            theme={theme}
            cursorStyle={cursorStyle}
            cursorBlink={cursorBlink}
            scrollbackLines={scrollbackLines}
            multiplexer={pane.multiplexer}
            resumed={pane.resumed === true}
            registerWrite={registerWrite}
            unregisterWrite={unregisterWrite}
            registerFocus={registerFocus}
            unregisterFocus={unregisterFocus}
            registerTerminalHandle={registerTerminalHandle}
            unregisterTerminalHandle={unregisterTerminalHandle}
            onClose={() => onClosePane(sessionId)}
            onFocusPane={() => onFocusPane(sessionId)}
            onShortcut={onShortcut}
            onFontSizeChange={onFontSizeChange}
            onMetadataChange={onMetadataChange}
            onTerminalError={onTerminalError}
            onBroadcastFailure={onBroadcastFailure}
            windowsBuildNumber={windowsBuildNumber}
            contextMenuTriggerProps={contextMenuTriggerProps}
            actionsDisabled={actionsDisabled}
            ask={ask}
            onConfirmLinkHost={onConfirmLinkHost}
            style={active ? { order } : { display: "none" }}
          />
        );
      })}
      {activePaneIds.length === 0 && (
        <div className="empty">
          터미널이 없습니다. 배포판을 선택하고 "+ 터미널"을 클릭하세요.
          <div className="dim">(터미널은 Windows에서 실행해야 동작합니다)</div>
        </div>
      )}
    </div>
  );
}
