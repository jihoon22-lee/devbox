import type { CSSProperties } from "react";
import TermPane from "./TermPane";
import type { Pane, Tab } from "../types";
import type { ShortcutAction } from "../lib/shortcuts";

interface PaneCanvasProps {
  tabs: Tab[];
  panes: Pane[];
  activeTabId: string;
  activePaneId: string | null;
  broadcastOn: boolean;
  registerWrite: (id: string, fn: (data: string) => void) => void;
  unregisterWrite: (id: string) => void;
  onClosePane: (id: string) => void;
  onFocusPane: (id: string) => void;
  onShortcut: (action: ShortcutAction) => void;
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
  registerWrite,
  unregisterWrite,
  onClosePane,
  onFocusPane,
  onShortcut,
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
            gridTemplateColumns: `repeat(${gridCols}, 1fr)`,
            gridTemplateRows: `repeat(${gridRows}, 1fr)`,
          };

  return (
    <div className="panes" style={gridStyle}>
      {panes.map((pane) => {
        const order = activePaneIds.indexOf(pane.id);
        const active = order !== -1;
        return (
          <TermPane
            key={pane.id}
            sessionId={pane.id}
            title={pane.distro}
            active={active}
            isFocusedPane={pane.id === activePaneId}
            broadcastOn={broadcastOn}
            broadcastTargetIds={activePaneIds}
            registerWrite={registerWrite}
            unregisterWrite={unregisterWrite}
            onClose={() => onClosePane(pane.id)}
            onFocusPane={() => onFocusPane(pane.id)}
            onShortcut={onShortcut}
            style={active ? { order } : { display: "none" }}
          />
        );
      })}
      {activePaneIds.length === 0 && (
        <div className="empty">
          No terminals. Select a distro and click "+ Terminal".
          <div className="dim">(터미널은 Windows에서 실행해야 동작합니다)</div>
        </div>
      )}
    </div>
  );
}
