import { isImeComposing } from "@devbox/a11y";
import { isRestoreOnly } from "../lib/workspace";
import type { Layout } from "../types";
import type * as React from "react";

interface Props {
  panelOpen: boolean;
  updateSettings: (patch: Partial<import("../lib/settings").TerminalSettings>) => void;
  logLensBusy: string | null;
  selected: string;
  selectDistro: (name: string) => void;
  distros: import("../types").DistroInfo[];
  cwd: string;
  setCwd: React.Dispatch<React.SetStateAction<string>>;
  addPane: () => Promise<boolean>;
  startCommand: string;
  setStartCommand: React.Dispatch<React.SetStateAction<string>>;
  recentPaths: string[];
  pinned: boolean;
  togglePinned: () => void;
  contextActionBusy: boolean;
  workspaceLoading: boolean;
  workspaceReady: boolean;
  setPaletteOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setSettingsOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setShortcutsOpen: React.Dispatch<React.SetStateAction<boolean>>;
  broadcastReady: boolean;
  broadcastOn: boolean;
  selectedBroadcastIds: string[];
  setBroadcastOn: React.Dispatch<React.SetStateAction<boolean>>;
  broadcastPickerOpen: boolean;
  activePaneIds: string[];
  setBroadcastPickerOpen: React.Dispatch<React.SetStateAction<boolean>>;
  activeLayout: import("../types").Layout;
  domainActionsBusy: boolean;
  setActiveTabLayout: (layout: import("../types").Layout) => void;
  LAYOUT_LABELS: Readonly<Record<import("../types").Layout, string>>;
}

export function TerminalToolbar({
  panelOpen,
  updateSettings,
  logLensBusy,
  selected,
  selectDistro,
  distros,
  cwd,
  setCwd,
  addPane,
  startCommand,
  setStartCommand,
  recentPaths,
  pinned,
  togglePinned,
  contextActionBusy,
  workspaceLoading,
  workspaceReady,
  setPaletteOpen,
  setSettingsOpen,
  setShortcutsOpen,
  broadcastReady,
  broadcastOn,
  selectedBroadcastIds,
  setBroadcastOn,
  broadcastPickerOpen,
  activePaneIds,
  setBroadcastPickerOpen,
  activeLayout,
  domainActionsBusy,
  setActiveTabLayout,
  LAYOUT_LABELS,
}: Props) {
  return (
    <header className="toolbar">
      <h1 className="title">WSL Desktop</h1>
      <button
        className={`btn panel-toggle ${panelOpen ? "active" : ""}`}
        title="사이드 패널 토글 (배포판/Docker/프로젝트)"
        onClick={() => updateSettings({ sidePanelOpen: !panelOpen })}
      >
        ☰
      </button>
      <select
        aria-label="현재 WSL 배포판"
        disabled={logLensBusy !== null}
        value={selected}
        onChange={(e) => selectDistro(e.currentTarget.value)}
      >
        {distros.map((d) => (
          <option key={d.name} value={d.name}>
            {d.name} {d.default ? "(기본)" : ""}
          </option>
        ))}
      </select>
      <input
        className="cwd"
        list="cwd-recent"
        placeholder="경로 열기 (선택, 예: /mnt/c/projects)"
        value={cwd}
        onChange={(e) => setCwd(e.currentTarget.value)}
        onKeyDown={(event) => {
          if (!isImeComposing(event) && event.key === "Enter") void addPane();
        }}
      />
      <input
        className="start-command"
        placeholder={
          isRestoreOnly() ? "상태 복원 창에서는 시작 명령을 보내지 않습니다" : "시작 명령 (선택, 프로필에 저장)"
        }
        disabled={isRestoreOnly()}
        value={startCommand}
        maxLength={4096}
        onChange={(event) => setStartCommand(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (!isImeComposing(event) && event.key === "Enter") void addPane();
        }}
      />
      <datalist id="cwd-recent">
        {recentPaths.map((p) => (
          <option key={p} value={p} />
        ))}
      </datalist>
      <button
        className={`btn pin-btn ${pinned ? "active" : ""}`}
        title={pinned ? "경로 고정됨 — 클릭하면 해제" : "경로 고정 — 켜면 열어도 입력칸이 비워지지 않습니다"}
        onClick={togglePinned}
      >
        📌
      </button>
      <button
        className="btn"
        disabled={contextActionBusy || workspaceLoading || logLensBusy !== null || !workspaceReady || !selected}
        onClick={() => void addPane()}
      >
        + 터미널
      </button>
      <button className="btn" title="명령 팔레트 (Ctrl+Shift+P)" onClick={() => setPaletteOpen(true)}>
        명령…
      </button>
      <button className="btn" title="설정" onClick={() => setSettingsOpen(true)}>
        설정
      </button>
      <button className="btn" title="키보드 단축키" onClick={() => setShortcutsOpen(true)}>
        단축키
      </button>
      <span className="spacer" />
      <label
        className="toggle"
        title={
          broadcastReady
            ? "선택한 팬에 동시 입력을 보냅니다"
            : "최신 WSL snapshot이 준비될 때까지 동시 입력을 사용할 수 없습니다"
        }
      >
        <input
          type="checkbox"
          aria-label="동시 입력 활성화"
          checked={broadcastOn}
          disabled={selectedBroadcastIds.length < 2 || !broadcastReady}
          onChange={(event) => setBroadcastOn(event.currentTarget.checked)}
        />
        동시 입력 {broadcastOn ? "켜짐" : "꺼짐"}
      </label>
      <button
        type="button"
        className={`btn compact ${broadcastPickerOpen ? "active" : ""}`}
        aria-label={`동시 입력 대상 선택 (${selectedBroadcastIds.length}/${activePaneIds.length})`}
        aria-expanded={broadcastPickerOpen}
        aria-controls="broadcast-target-picker"
        onClick={() => setBroadcastPickerOpen((open) => !open)}
      >
        대상 {selectedBroadcastIds.length}/{activePaneIds.length}
      </button>
      <select
        aria-label="탭 레이아웃"
        className="layout-select"
        value={activeLayout}
        disabled={domainActionsBusy}
        onChange={(event) => setActiveTabLayout(event.currentTarget.value as Layout)}
      >
        {(["grid", "cols", "rows"] as const).map((option) => (
          <option key={option} value={option}>
            {LAYOUT_LABELS[option]}
          </option>
        ))}
      </select>
    </header>
  );
}
