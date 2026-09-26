import { type PaletteAction } from "../components/ActionPalette";
import { DEFAULT_TERMINAL_FONT_SIZE } from "./terminalUx";
import type * as React from "react";

interface Props {
  openNewTab: () => Promise<boolean>;
  addPane: () => Promise<boolean>;
  activeTab: import("../types").Tab | null;
  renameContextTab: (tab: import("../types").Tab) => Promise<void>;
  activeTabId: string;
  requestCloseTab: (tabId: string) => Promise<void>;
  LAYOUT_LABELS: Readonly<Record<import("../types").Layout, string>>;
  setActiveTabLayout: (layout: import("../types").Layout) => void;
  splitActivePane: (layout: "cols" | "rows") => void;
  activePaneId: string | null;
  terminalHandles: React.RefObject<Map<string, import("../components/TermPane").TerminalPaneHandle>>;
  updateTerminalFontSize: (value: number) => void;
  setSettingsOpen: React.Dispatch<React.SetStateAction<boolean>>;
  requestClosePane: (paneId: string) => Promise<void>;
  zoomedPaneId: string | null;
  setZoomedPaneId: React.Dispatch<React.SetStateAction<string | null>>;
  panelOpen: boolean;
  updateSettings: (patch: Partial<import("./settings").TerminalSettings>) => void;
  logLensBusyRef: React.RefObject<string | null>;
  refreshDashboard: (force?: boolean) => Promise<void>;
  refreshMultiplexers: (distro?: string) => Promise<void>;
  saveCurrentProfile: () => Promise<void>;
  setShortcutsOpen: React.Dispatch<React.SetStateAction<boolean>>;
  distros: import("../types").DistroInfo[];
  openDistroTerminal: (name: string) => void;
  profiles: import("../types").WorkspaceProfile[];
  openProfile: (profile: import("../types").WorkspaceProfile) => Promise<boolean>;
}

export function buildTerminalPalette({
  openNewTab,
  addPane,
  activeTab,
  renameContextTab,
  activeTabId,
  requestCloseTab,
  LAYOUT_LABELS,
  setActiveTabLayout,
  splitActivePane,
  activePaneId,
  terminalHandles,
  updateTerminalFontSize,
  setSettingsOpen,
  requestClosePane,
  zoomedPaneId,
  setZoomedPaneId,
  panelOpen,
  updateSettings,
  logLensBusyRef,
  refreshDashboard,
  refreshMultiplexers,
  saveCurrentProfile,
  setShortcutsOpen,
  distros,
  openDistroTerminal,
  profiles,
  openProfile,
}: Props) {
  const paletteActions: PaletteAction[] = [
    {
      id: "new-tab",
      label: "탭: 새 탭",
      description: "활성 팬의 배포판·cwd를 물려받는다",
      keys: "Ctrl+Shift+T",
      run: () => void openNewTab(),
    },
    {
      id: "new-pane",
      label: "팬: 활성 탭에 추가",
      keys: "Ctrl+Shift+D",
      run: () => void addPane(),
    },
    {
      id: "rename-tab",
      label: "탭: 이름 변경",
      run: () => {
        if (activeTab) void renameContextTab(activeTab);
      },
    },
    {
      id: "close-tab",
      label: "탭: 닫기",
      description: "탭의 모든 터미널이 함께 닫힌다",
      danger: true,
      run: () => {
        if (activeTabId) void requestCloseTab(activeTabId);
      },
    },
    ...(["grid", "cols", "rows"] as const).map(
      (layout): PaletteAction => ({
        id: `layout-${layout}`,
        label: `레이아웃: ${LAYOUT_LABELS[layout]}`,
        run: () => setActiveTabLayout(layout),
      }),
    ),
    {
      id: "split-vertical",
      label: "팬: 세로 분할",
      description: "활성 팬과 같은 배포판/cwd로 오른쪽에 추가",
      run: () => splitActivePane("cols"),
    },
    {
      id: "split-horizontal",
      label: "팬: 가로 분할",
      description: "활성 팬과 같은 배포판/cwd로 아래에 추가",
      run: () => splitActivePane("rows"),
    },
    {
      id: "search",
      label: "팬: 출력 검색",
      description: "활성 팬의 스크롤백 검색",
      keys: "Ctrl+Shift+F",
      run: () => activePaneId && terminalHandles.current.get(activePaneId)?.openSearch(),
    },
    {
      id: "copy",
      label: "팬: 선택 복사",
      keys: "Ctrl+Shift+C",
      run: () => {
        if (activePaneId) void terminalHandles.current.get(activePaneId)?.copySelection();
      },
    },
    {
      id: "paste",
      label: "팬: 붙여넣기",
      keys: "Ctrl+Shift+V",
      run: () => {
        if (activePaneId) void terminalHandles.current.get(activePaneId)?.pasteClipboard();
      },
    },
    {
      id: "copy-cwd",
      label: "팬: cwd 복사",
      description: "활성 팬이 보고한 현재 경로 복사",
      run: () => {
        if (activePaneId) void terminalHandles.current.get(activePaneId)?.copyCwd();
      },
    },
    {
      id: "font-reset",
      label: "글꼴 크기 기본값",
      keys: "Ctrl+0",
      run: () => updateTerminalFontSize(DEFAULT_TERMINAL_FONT_SIZE),
    },
    {
      id: "settings",
      label: "설정 열기",
      description: "글꼴·테마·커서·스크롤백과 확인 동작",
      run: () => setSettingsOpen(true),
    },
    {
      id: "close-pane",
      label: "팬: 닫기",
      description: "실행 중인 작업이 종료될 수 있음",
      keys: "Ctrl+Shift+W",
      danger: true,
      run: () => {
        if (activePaneId) void requestClosePane(activePaneId);
      },
    },
    {
      id: "zoom-pane",
      label: zoomedPaneId ? "팬: 확대 해제" : "팬: 확대",
      description: "활성 팬만 탭 전체에 표시",
      run: () => setZoomedPaneId((current) => (current ? null : activePaneId)),
    },
    {
      id: "clear-scrollback",
      label: "팬: 스크롤백 비우기",
      description: "화면에 보이는 줄만 남기고 버퍼를 비운다",
      run: () => {
        if (activePaneId) terminalHandles.current.get(activePaneId)?.clearScrollback();
      },
    },
    {
      id: "scroll-bottom",
      label: "팬: 맨 아래로 이동",
      run: () => {
        if (activePaneId) terminalHandles.current.get(activePaneId)?.scrollToBottom();
      },
    },
    {
      id: "select-all",
      label: "팬: 전체 선택",
      run: () => {
        if (activePaneId) terminalHandles.current.get(activePaneId)?.selectAll();
      },
    },
    {
      id: "toggle-panel",
      label: `사이드 패널 ${panelOpen ? "숨기기" : "보이기"}`,
      run: () => updateSettings({ sidePanelOpen: !panelOpen }),
    },
    {
      id: "refresh-snapshot",
      label: "WSL 상태와 멀티플렉서 새로 고침",
      run: () => {
        if (logLensBusyRef.current === null) {
          void refreshDashboard(true).catch(() => undefined);
          void refreshMultiplexers();
        }
      },
    },
    {
      id: "save-profile",
      label: "현재 레이아웃을 프로필로 저장",
      run: () => void saveCurrentProfile(),
    },
    {
      id: "shortcuts",
      label: "키보드 단축키 보기",
      run: () => setShortcutsOpen(true),
    },
    ...distros.map(
      (distro): PaletteAction => ({
        id: `open-distro-${distro.name}`,
        label: `터미널 열기: ${distro.name}`,
        description: distro.default ? "기본 배포판" : undefined,
        run: () => openDistroTerminal(distro.name),
      }),
    ),
    ...profiles.map(
      (profile): PaletteAction => ({
        id: `profile-${profile.id}`,
        label: `프로필 전환: ${profile.name}`,
        description: `${profile.tabs.length}개 탭 · ${profile.panes.length}개 팬`,
        run: () => void openProfile(profile),
      }),
    ),
  ];
  return { paletteActions };
}
