import { buildTerminalPalette } from "./lib/terminalPalette";
import { TerminalToolbar } from "./components/TerminalToolbar";
import { useWorkspaceLaunch } from "./hooks/useWorkspaceLaunch";
import { usePolling } from "@devbox/hooks";
import { isProductHosted } from "../transport";
import { ContextMenu, useContextMenu, type ContextMenuEntry } from "@devbox/context-menu";
import { isImeComposing } from "@devbox/a11y";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  closeSession,
  configureQuickSummon,
  detectMultiplexers,
  dockerAction,
  getDashboardSnapshot,
  getWindowsBuildNumber,
  listWorkspaceProfiles,
  listDistros,
  onOpenRequest,
  onTerminalClosed,
  onTerminalOutput,
  openWslFileInLogLens,
  openWslJournalInLogLens,
  takePendingOpen,
  type QuickSummonStatus,
} from "./api";
import AppDialog, { useAppDialog, type AskDialog } from "./components/AppDialog";
import DistroPanel from "./components/DistroPanel";
import SettingsPanel from "./components/SettingsPanel";
import ShortcutReference from "./components/ShortcutReference";
import ActionPalette from "./components/ActionPalette";
import PaneCanvas from "./components/PaneCanvas";
import type { TerminalPaneCapabilities, TerminalPaneHandle } from "./components/TermPane";
import TabBar from "./components/TabBar";
import WorkspacePanel from "./components/WorkspacePanel";
import { routeOpenRequest } from "./lib/applink";
import { buildPaneContextMenu, buildTabContextMenu, normalizeTabName } from "./lib/contextMenu";
import { matchShortcut, type ShortcutAction } from "./lib/shortcuts";
import { nextPaneIndex, type FocusDirection } from "./lib/paneGeometry";
import { normalizePaneSizing } from "./lib/paneSizing";
import { MAX_BROADCAST_TARGETS, nextBroadcastTargets } from "./lib/broadcastSafety";
import {
  loadCopyOnSelect,
  loadPinned,
  loadPinnedCwd,
  loadRecentPaths,
  loadTerminalFontSize,
  saveCopyOnSelect,
  savePinned,
  savePinnedCwd,
  saveTerminalFontSize,
} from "./lib/storage";
import { loadLastWorkspace, normalizeProfile, saveLastWorkspace, workspaceFromRuntime } from "./lib/workspace";
import { clampTerminalFontSize } from "./lib/terminalUx";
import type {
  ContainerInfo,
  DashboardSnapshot,
  DistroInfo,
  Layout,
  MultiplexerAvailability,
  OpenRequest,
  Pane,
  Tab,
  WorkspaceProfile,
} from "./types";
import type { DashboardFreshness } from "./lib/resourceDisplay";
import { isSnapshotActionable, isSnapshotExpired } from "./lib/snapshotState";
import { fontFamilyFor, loadSettings, saveSettings, TERMINAL_THEMES, type TerminalSettings } from "./lib/settings";
import "./App.css";

const LAYOUT_LABELS: Readonly<Record<Layout, string>> = {
  grid: "격자",
  cols: "세로 분할",
  rows: "가로 분할",
};

const DASHBOARD_ERROR_MESSAGE = "WSL resource snapshot을 갱신하지 못했습니다. 마지막 정상 상태를 유지합니다.";
const RESTORE_FAILURE_MESSAGE = "터미널을 복원하지 못했습니다. 설정을 확인한 뒤 다시 시도하세요.";

function paneIdentity(pane: Pane): string {
  return pane.sessionId ?? pane.key;
}

export default function App() {
  const [distros, setDistros] = useState<DistroInfo[]>([]);
  const [selected, setSelected] = useState("");
  const [containers, setContainers] = useState<ContainerInfo[]>([]);
  const [dockerMissing, setDockerMissing] = useState(false);
  useEffect(() => {
    const failed = () =>
      setError("터미널 설정을 저장하지 못했습니다. 다른 창의 변경을 확인한 뒤 설정을 다시 열어 주세요.");
    window.addEventListener("terminal-preference-save-failed", failed);
    return () => window.removeEventListener("terminal-preference-save-failed", failed);
  }, []);
  const [dashboardSnapshot, setDashboardSnapshot] = useState<DashboardSnapshot | null>(null);
  const [dashboardState, setDashboardState] = useState<DashboardFreshness>("loading");
  // Recomputed by the freshness tick below so an in-flight refresh that outlives the TTL
  // still fails closed, without treating "a refresh started" as "unsafe".
  const [snapshotExpired, setSnapshotExpired] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [logLensBusy, setLogLensBusy] = useState<string | null>(null);
  const [settings, setSettings] = useState<TerminalSettings>(loadSettings);
  const panelOpen = settings.sidePanelOpen;
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [quickSummonStatus, setQuickSummonStatus] = useState<QuickSummonStatus | null>(null);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [zoomedPaneId, setZoomedPaneId] = useState<string | null>(null);
  const [pinned, setPinned] = useState<boolean>(loadPinned);
  const [cwd, setCwd] = useState<string>(() => (loadPinned() ? loadPinnedCwd() : ""));
  const [recentPaths, setRecentPaths] = useState<string[]>(loadRecentPaths);
  const [panes, setPanes] = useState<Pane[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string>("");
  const [activePaneId, setActivePaneId] = useState<string | null>(null);
  const [broadcastOn, setBroadcastOn] = useState(false);
  const [broadcastTargetIds, setBroadcastTargetIds] = useState<Set<string>>(() => new Set());
  const [broadcastPickerOpen, setBroadcastPickerOpen] = useState(false);
  const [startCommand, setStartCommand] = useState("");
  const multiplexer = settings.multiplexer;
  const [muxAvailability, setMuxAvailability] = useState<MultiplexerAvailability[]>([
    { kind: "native", status: "available", version: null, source: null },
    { kind: "tmux", status: "missing", version: null, source: null },
    { kind: "zellij", status: "missing", version: null, source: null },
  ]);
  const [muxScanning, setMuxScanning] = useState(false);
  const [profiles, setProfiles] = useState<WorkspaceProfile[]>([]);
  const [profilesLoaded, setProfilesLoaded] = useState(false);
  const [workspaceReady, setWorkspaceReady] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [copyOnSelect, setCopyOnSelect] = useState(loadCopyOnSelect);
  const [terminalFontSize, setTerminalFontSize] = useState(loadTerminalFontSize);
  const [error, setError] = useState<string | null>(null);
  const [contextActionBusy, setContextActionBusy] = useState(false);
  const contextActionBusyRef = useRef(false);
  const [contextPane, setContextPane] = useState<Pane | null>(null);
  const [contextPaneCapabilities, setContextPaneCapabilities] = useState<TerminalPaneCapabilities>({
    hasSelection: false,
    hasCwd: false,
  });
  const [contextTab, setContextTab] = useState<Tab | null>(null);
  const { ask, pending: pendingDialog, answer: answerDialog } = useAppDialog();
  const shortcutBlockersRef = useRef({
    pendingDialog,
    paletteOpen,
    settingsOpen,
    shortcutsOpen,
  });
  shortcutBlockersRef.current = { pendingDialog, paletteOpen, settingsOpen, shortcutsOpen };
  const askRef = useRef<AskDialog>(ask);
  askRef.current = ask;
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const dashboardStateRef = useRef(dashboardState);
  dashboardStateRef.current = dashboardState;
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  // Hosts the user chose not to be asked about again. Session-scoped on purpose: a
  // remembered host must not outlive the window that granted it.
  const trustedLinkHosts = useRef(new Set<string>());
  // TermPane must not create xterm until this one-time lookup resolves, so
  // every terminal receives its final ConPTY build-number option.
  const [windowsBuildNumber, setWindowsBuildNumber] = useState<number | null | undefined>(undefined);
  // Flips true once the first shared dashboard snapshot resolves. Gates applink handling
  // (below) so a `path` target has a real default distro to open into,
  // rather than racing the empty initial `selected` state.
  const [distrosLoaded, setDistrosLoaded] = useState(false);
  const writes = useRef(new Map<string, (data: string) => void>());
  const paneFocus = useRef(new Map<string, () => void>());
  const terminalHandles = useRef(new Map<string, TerminalPaneHandle>());
  const restoreStarted = useRef(false);
  const panesRef = useRef(panes);
  panesRef.current = panes;
  const workspaceLoadingRef = useRef(false);
  const workspaceRestoreGeneration = useRef(0);
  const layoutSaveTimer = useRef<number | undefined>(undefined);
  const dashboardRequestRef = useRef<Promise<void> | null>(null);
  const dashboardRefreshQueuedRef = useRef(false);
  const dashboardRequestSequence = useRef(0);
  const muxRequestSequence = useRef(0);
  const dashboardClockRef = useRef<number>(Date.now());
  const dashboardSnapshotRef = useRef<DashboardSnapshot | null>(null);
  const dashboardMountedRef = useRef(true);
  dashboardSnapshotRef.current = dashboardSnapshot;
  const mountedRef = useRef(true);
  const logLensGeneration = useRef(0);
  const busyRef = useRef<string | null>(null);
  const logLensBusyRef = useRef<string | null>(null);
  const dashboardOperationToken = useRef(0);
  const logLensOperationToken = useRef(0);
  const quickSummonRequestSequence = useRef(0);
  const quickSummonQueue = useRef<Promise<void>>(Promise.resolve());

  const setContextBusy = (value: boolean) => {
    contextActionBusyRef.current = value;
    setContextActionBusy(value);
  };

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      workspaceRestoreGeneration.current += 1;
      logLensGeneration.current += 1;
      busyRef.current = null;
      logLensBusyRef.current = null;
      contextActionBusyRef.current = false;
    };
  }, []);

  // onTerminalClosed 구독은 마운트 시 한 번만 걸린다(아래 effect, deps []). 그 콜백이
  // dropPane을 부를 때 tabs/activeTabId/activePaneId를 직접 클로저로 참조하면 마운트
  // 시점의 낡은 값에 고정된다. 매 렌더마다 최신 값을 채우는 ref로 우회한다.
  const stateRef = useRef({ tabs, activeTabId, activePaneId });
  stateRef.current = { tabs, activeTabId, activePaneId };

  const registerWrite = useCallback((id: string, fn: (data: string) => void) => {
    writes.current.set(id, fn);
  }, []);
  const unregisterWrite = useCallback((id: string) => {
    writes.current.delete(id);
  }, []);
  const registerFocus = useCallback((id: string, focus: () => void) => {
    paneFocus.current.set(id, focus);
  }, []);
  const unregisterFocus = useCallback((id: string) => {
    paneFocus.current.delete(id);
  }, []);
  const registerTerminalHandle = useCallback((id: string, handle: TerminalPaneHandle) => {
    terminalHandles.current.set(id, handle);
  }, []);
  const unregisterTerminalHandle = useCallback((id: string) => {
    terminalHandles.current.delete(id);
  }, []);
  /** 링크 host 확인. 사용자가 기억을 선택한 host는 이 창이 살아 있는 동안만 다시 묻지 않는다. */
  const confirmLinkHost = useCallback(async (host: string): Promise<boolean> => {
    if (trustedLinkHosts.current.has(host)) return true;
    const answer = await askRef.current({
      kind: "confirm",
      title: `'${host}' 링크를 기본 브라우저에서 열까요?`,
      confirmLabel: "열기",
      rememberLabel: "이 창에서는 이 host를 다시 묻지 않기",
    });
    if (!answer.confirmed) return false;
    if (answer.remember) trustedLinkHosts.current.add(host);
    return true;
  }, []);

  const updateSettings = useCallback((patch: Partial<TerminalSettings>) => {
    setSettings((previous) => {
      const next = { ...previous, ...patch };
      saveSettings(next);
      return next;
    });
  }, []);

  useEffect(() => {
    const sequence = ++quickSummonRequestSequence.current;
    setQuickSummonStatus(null);
    const config = {
      shortcutEnabled: settings.quickSummonEnabled,
      shortcut: settings.quickSummonShortcut,
      keepInTray: settings.keepInTray,
    };
    const request = quickSummonQueue.current.catch(() => undefined).then(() => configureQuickSummon(config));
    // Keep native mutations in the same order as the user's setting changes.
    // Only the latest response is rendered, but every queued configuration is
    // applied before the one that supersedes it.
    quickSummonQueue.current = request.then(
      () => undefined,
      () => undefined,
    );
    void request
      .then((status) => {
        if (mountedRef.current && quickSummonRequestSequence.current === sequence) {
          setQuickSummonStatus(status);
        }
      })
      .catch(() => {
        if (mountedRef.current && quickSummonRequestSequence.current === sequence) {
          setQuickSummonStatus({
            shortcutRegistered: false,
            activeShortcut: null,
            trayEnabled: false,
            closeBehavior: "exit",
            issues: ["backendUnavailable"],
          });
        }
      });
    return () => {
      if (quickSummonRequestSequence.current === sequence) {
        quickSummonRequestSequence.current += 1;
      }
    };
  }, [settings.keepInTray, settings.quickSummonEnabled, settings.quickSummonShortcut]);

  const updateTerminalFontSize = useCallback((value: number) => {
    const next = clampTerminalFontSize(value);
    setTerminalFontSize(next);
    saveTerminalFontSize(next);
  }, []);
  const updatePaneMetadata = useCallback((id: string, metadata: { title?: string; cwd?: string }) => {
    setPanes((previous) => {
      let changed = false;
      const next = previous.map((pane) => {
        if (pane.sessionId !== id) return pane;
        const titleChanged = metadata.title !== undefined && metadata.title !== pane.title;
        const cwdChanged = metadata.cwd !== undefined && metadata.cwd !== pane.cwd;
        if (!titleChanged && !cwdChanged) return pane;
        changed = true;
        return { ...pane, ...metadata };
      });
      return changed ? next : previous;
    });
  }, []);

  // 사용자가 이름을 붙이지 않은 탭은 현재 활성 팬의 OSC 0/2 제목을 따른다. 수동 rename은
  // customTitle로 고정해 이후 shell title sequence가 사용자 이름을 덮어쓰지 못하게 한다.
  useEffect(() => {
    if (!activePaneId) return;
    const paneTitle = panes.find((pane) => pane.sessionId === activePaneId)?.title;
    if (!paneTitle) return;
    setTabs((previous) => {
      let changed = false;
      const next = previous.map((tab) => {
        if (tab.customTitle || !tab.paneIds.includes(activePaneId) || tab.title === paneTitle) return tab;
        changed = true;
        return { ...tab, title: paneTitle };
      });
      return changed ? next : previous;
    });
  }, [activePaneId, panes]);

  useEffect(() => {
    let disposed = false;
    void listWorkspaceProfiles()
      .then((items) => {
        if (disposed) return;
        setProfiles(items.map(normalizeProfile).filter((item): item is WorkspaceProfile => item !== null));
      })
      .catch(() => {
        if (!disposed) setError("터미널 프로필 목록을 읽지 못했습니다.");
      })
      .finally(() => {
        if (!disposed) setProfilesLoaded(true);
      });
    return () => {
      disposed = true;
    };
  }, []);

  const refreshMultiplexers = useCallback(async (distro = selectedRef.current): Promise<void> => {
    if (!distro) return;
    const sequence = ++muxRequestSequence.current;
    setMuxScanning(true);
    try {
      const availability = await detectMultiplexers(distro);
      if (!mountedRef.current || sequence !== muxRequestSequence.current || selectedRef.current !== distro) return;
      setMuxAvailability(availability);
    } catch {
      if (!mountedRef.current || sequence !== muxRequestSequence.current || selectedRef.current !== distro) return;
      setMuxAvailability([
        { kind: "native", status: "available", version: null, source: null },
        { kind: "tmux", status: "error", version: null, source: null },
        { kind: "zellij", status: "error", version: null, source: null },
      ]);
    } finally {
      if (mountedRef.current && sequence === muxRequestSequence.current && selectedRef.current === distro) {
        setMuxScanning(false);
      }
    }
  }, []);

  useEffect(() => {
    if (!selected) return;
    void refreshMultiplexers(selected);
  }, [refreshMultiplexers, selected]);

  useEffect(() => {
    void getWindowsBuildNumber()
      .then(setWindowsBuildNumber)
      .catch(() => setWindowsBuildNumber(null));
  }, []);

  const refreshDashboard = useCallback((force = false): Promise<void> => {
    if (logLensBusyRef.current !== null) return Promise.resolve();
    // A refresh is a shared single-flight operation. Keeping one promise here also means the
    // manual button, lifecycle triggers and the periodic freshness guard cannot race their
    // responses and regress the resource/session generation shown by the UI.
    if (dashboardRequestRef.current) {
      // A lifecycle mutation that happens while an older collection is in flight needs one
      // follow-up generation. Coalesce all such requests into one queued refresh rather than
      // letting each caller start its own native collection.
      if (force) dashboardRefreshQueuedRef.current = true;
      return dashboardRequestRef.current;
    }
    const sequence = ++dashboardRequestSequence.current;
    setDashboardState(dashboardSnapshotRef.current ? "refreshing" : "loading");
    const request = getDashboardSnapshot()
      .then((next) => {
        if (!dashboardMountedRef.current || sequence !== dashboardRequestSequence.current) return;
        dashboardSnapshotRef.current = next;
        setDashboardSnapshot(next);
        setDistros(
          next.distros.map(({ name, version, default: isDefault, state }) => ({
            name,
            version,
            default: isDefault,
            state,
          })),
        );
        const fallback = next.distros.find((distro) => distro.default)?.name ?? next.distros[0]?.name ?? "";
        setSelected((previous) => (next.distros.some((distro) => distro.name === previous) ? previous : fallback));
        setError((current) => (current === DASHBOARD_ERROR_MESSAGE ? null : current));
        setDashboardState("fresh");
        setDistrosLoaded(true);
      })
      .catch(async () => {
        if (!dashboardMountedRef.current || sequence !== dashboardRequestSequence.current) return;
        // Keep the last good snapshot and its Docker/session/resource data. The terminal
        // transport remains usable; only broadcast is fail-closed by the shared status below.
        // Keep an error state even when the last snapshot is still younger than its normal TTL;
        // a failed poll must never silently re-enable broadcast on the next freshness tick.
        setDashboardState("error");
        setError(DASHBOARD_ERROR_MESSAGE);
        if (isProductHosted() && !dashboardSnapshotRef.current) {
          // CPU/Docker telemetry does not authorize a PTY. A companion already
          // owns an explicitly opened profile; hydrate its target names from a
          // fresh read-only distro list while native launch rechecks each binding.
          // Keep the failed dashboard state so broadcast/default-shell guessing
          // remains disabled. Never use an old cached list after this read fails.
          try {
            const current = await listDistros();
            if (!dashboardMountedRef.current || sequence !== dashboardRequestSequence.current) return;
            setDistros(current);
            setSelected((previous) =>
              current.some((distro) => distro.name === previous)
                ? previous
                : (current.find((distro) => distro.default)?.name ?? current[0]?.name ?? ""),
            );
            setDistrosLoaded(true);
          } catch {
            /* No fresh target list: saved-profile restoration stays blocked. */
          }
        }
      })
      .finally(() => {
        if (dashboardMountedRef.current && sequence === dashboardRequestSequence.current) {
          dashboardRequestRef.current = null;
          if (dashboardRefreshQueuedRef.current) {
            dashboardRefreshQueuedRef.current = false;
            void refreshDashboard().catch(() => undefined);
          }
        }
      });
    dashboardRequestRef.current = request;
    return request;
  }, []);

  useEffect(() => {
    // React StrictMode replays effect setup/cleanup in development. Restore this instance's
    // mounted flag in setup so the replay cannot make every later snapshot look unmounted.
    dashboardMountedRef.current = true;
    return () => {
      dashboardMountedRef.current = false;
      dashboardRequestSequence.current += 1;
      dashboardRequestRef.current = null;
    };
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: existing dependency list; review in P1-15
  useEffect(() => {
    void refreshDashboard().catch(() => undefined);
    // The callback is intentionally stable: its single-flight state lives in refs and must not
    // be retriggered every time a new successful snapshot is committed.
  }, []);

  useEffect(() => {
    const entry = dashboardSnapshot?.distros.find((distro) => distro.name === selected);
    if (!entry) {
      setContainers([]);
      setDockerMissing(false);
      return;
    }
    setContainers(entry.containers);
    setDockerMissing(entry.dockerAvailability === "missing");
  }, [dashboardSnapshot, selected]);

  const updateFreshness = useCallback(() => {
    if (!dashboardSnapshot) return;
    dashboardClockRef.current = Date.now();
    const expired = isSnapshotExpired(dashboardSnapshot, dashboardClockRef.current);
    setSnapshotExpired(expired);
    if (dashboardRequestRef.current) return;
    setDashboardState((current) => (current === "error" ? current : expired ? "stale" : "fresh"));
  }, [dashboardSnapshot]);
  useEffect(updateFreshness, [updateFreshness]);
  usePolling(updateFreshness, { intervalMs: 1_000, active: Boolean(dashboardSnapshot), immediate: false });
  usePolling(
    async () => {
      await refreshDashboard().catch(() => undefined);
    },
    {
      intervalMs: Math.min(60_000, Math.max(5_000, dashboardSnapshot?.staleAfterMs ?? 5_000)),
      active: Boolean(dashboardSnapshot),
      immediate: false,
    },
  );

  // One rule for every snapshot-gated control. An in-flight collection is not by itself a
  // reason to close them; a failed or expired one is.
  const snapshotActionable = isSnapshotActionable(dashboardState, dashboardSnapshot !== null, snapshotExpired);

  const onDockerAction = async (id: string, action: "start" | "stop" | "restart") => {
    if (busyRef.current !== null || logLensBusyRef.current !== null) return;
    const snapshot = dashboardSnapshot?.distros.find((distro) => distro.name === selected);
    if (
      !snapshot ||
      !snapshotActionable ||
      snapshot.dockerAvailability !== "available" ||
      !snapshot.containers.some((container) => container.id === id)
    ) {
      setError("최신 Docker snapshot이 준비될 때까지 상태를 변경할 수 없습니다.");
      return;
    }
    const token = ++dashboardOperationToken.current;
    const operation = `${id}:${action}`;
    busyRef.current = operation;
    setBusy(operation);
    try {
      await dockerAction(selected, id, action);
      await refreshDashboard(true);
    } catch {
      setError("Docker 상태를 안전하게 변경하지 못했습니다.");
    } finally {
      if (dashboardOperationToken.current === token) {
        busyRef.current = null;
        if (mountedRef.current) setBusy(null);
      }
    }
  };

  const selectDistro = (name: string) => {
    if (logLensBusyRef.current !== null) return;
    setSelected(name);
  };

  const openJournalInLogLens = async (name: string): Promise<void> => {
    if (busyRef.current !== null || logLensBusyRef.current !== null || workspaceLoadingRef.current || contextActionBusy)
      return;
    const confirmed = await ask({
      kind: "confirm",
      title: `'${name}'의 WSL journal을 Log Lens에서 열까요?`,
      lines: ["읽기 전용으로만 열립니다.", "로그 원문·명령·자격 증명은 handoff에 포함되지 않습니다."],
      confirmLabel: "Log Lens에서 열기",
    });
    if (!confirmed.confirmed) return;
    const generation = ++logLensGeneration.current;
    const token = ++logLensOperationToken.current;
    const operation = `log-lens-journal:${name}`;
    logLensBusyRef.current = operation;
    setLogLensBusy(operation);
    setError(null);
    void openWslJournalInLogLens(name, null)
      .then(() => {
        if (mountedRef.current && token === logLensOperationToken.current && generation === logLensGeneration.current) {
          setError(null);
        }
      })
      .catch(() => {
        if (mountedRef.current && token === logLensOperationToken.current && generation === logLensGeneration.current) {
          setError("Log Lens journal handoff를 시작하지 못했습니다.");
        }
      })
      .finally(() => {
        if (token === logLensOperationToken.current && generation === logLensGeneration.current) {
          logLensBusyRef.current = null;
          if (mountedRef.current) setLogLensBusy(null);
        }
      });
  };

  const openFileInLogLens = async (name: string): Promise<void> => {
    if (busyRef.current !== null || logLensBusyRef.current !== null || workspaceLoadingRef.current || contextActionBusy)
      return;
    const entered = await ask({
      kind: "prompt",
      title: "Log Lens에서 열 WSL 파일",
      lines: ["열려는 파일의 절대 경로를 입력하세요."],
      inputLabel: "WSL 파일 절대 경로",
      placeholder: "/var/log/app.log",
      confirmLabel: "계속",
    });
    if (!entered.confirmed) return;
    const wslPath = entered.value.trim();
    if (!wslPath) {
      setError("WSL 파일 경로를 입력해야 합니다.");
      return;
    }
    const confirmed = await ask({
      kind: "confirm",
      title: `'${name}'의 선택한 WSL 파일을 Log Lens에서 열까요?`,
      lines: ["읽기 전용으로만 열립니다.", "경로는 검증된 WSL adapter 설정으로만 한 번 전달됩니다."],
      confirmLabel: "Log Lens에서 열기",
    });
    if (!confirmed.confirmed) return;
    const generation = ++logLensGeneration.current;
    const token = ++logLensOperationToken.current;
    const operation = `log-lens-file:${name}`;
    logLensBusyRef.current = operation;
    setLogLensBusy(operation);
    setError(null);
    void openWslFileInLogLens(name, wslPath)
      .then(() => {
        if (mountedRef.current && token === logLensOperationToken.current && generation === logLensGeneration.current) {
          setError(null);
        }
      })
      .catch(() => {
        if (mountedRef.current && token === logLensOperationToken.current && generation === logLensGeneration.current) {
          setError("Log Lens file handoff를 시작하지 못했습니다.");
        }
      })
      .finally(() => {
        if (token === logLensOperationToken.current && generation === logLensGeneration.current) {
          logLensBusyRef.current = null;
          if (mountedRef.current) setLogLensBusy(null);
        }
      });
  };

  const openDistroTerminal = (name: string) => {
    if (!workspaceReady || workspaceLoading || logLensBusyRef.current !== null) return;
    setSelected(name);
    void startInTab(null, name);
  };

  // 팬 하나(연결된 session id 또는 실패 placeholder key)를 제거한다. 마지막 팬이면 탭도 닫는다.
  // stateRef를 통해서만 tabs/activeTabId/activePaneId를 읽는다 — 위 주석 참고.
  const dropPane = useCallback((paneId: string) => {
    const { tabs: curTabs, activeTabId: curActiveTabId, activePaneId: curActivePaneId } = stateRef.current;
    setPanes((prev) => {
      const next = prev.filter((pane) => paneIdentity(pane) !== paneId);
      panesRef.current = next;
      return next;
    });

    const ownerIdx = curTabs.findIndex((t) => t.paneIds.includes(paneId));
    if (ownerIdx === -1) {
      setActivePaneId((prev) => (prev === paneId ? null : prev));
      return;
    }
    const owner = curTabs[ownerIdx];
    const remaining = owner.paneIds.filter((id) => id !== paneId);
    const tabClosed = remaining.length === 0;

    const nextTabs = tabClosed
      ? curTabs.filter((t) => t.id !== owner.id)
      : curTabs.map((tab) =>
          tab.id === owner.id
            ? {
                ...tab,
                paneIds: remaining,
                sizing: normalizePaneSizing(undefined, tab.layout, remaining.length),
              }
            : tab,
        );
    setTabs(nextTabs);

    if (tabClosed && curActiveTabId === owner.id) {
      const fallback = nextTabs[Math.min(ownerIdx, nextTabs.length - 1)] ?? null;
      setActiveTabId(fallback ? fallback.id : "");
      setActivePaneId(fallback ? (fallback.paneIds[fallback.paneIds.length - 1] ?? null) : null);
    } else if (curActivePaneId === paneId) {
      setActivePaneId(remaining[remaining.length - 1] ?? null);
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    let stopOutput: (() => void) | undefined;
    let stopClosed: (() => void) | undefined;
    void onTerminalOutput(({ session_id, data }) => {
      if (disposed) return;
      writes.current.get(session_id)?.(data);
    })
      .then((stop) => {
        if (disposed) stop();
        else stopOutput = stop;
      })
      .catch(() => undefined);
    void onTerminalClosed(({ session_id }) => {
      if (disposed) return;
      // 백엔드가 세션 리소스를 정리한 뒤 보내는 이벤트다. 여기서는 UI 상태만
      // 제거하고 close_session은 호출하지 않는다.
      dropPane(session_id);
      writes.current.delete(session_id);
    })
      .then((stop) => {
        if (disposed) stop();
        else stopClosed = stop;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      stopOutput?.();
      stopClosed?.();
    };
  }, [dropPane]);

  // Inbound cross-app open requests (§3). Redefined every render so it always
  // closes over the latest tabs/activeTabId/selected/startInTab — the
  // devbox://open listener below is set up once and lives for the app's
  // lifetime, so without this a relaunch long after mount would act on
  // stale state (same hazard the stateRef comment above documents for
  // onTerminalClosed).
  const handleOpenRequest = (request: OpenRequest) => {
    const action = routeOpenRequest(request);
    switch (action.kind) {
      case "openTerminal":
        // repo-manager는 저장소의 Windows 경로를 보내는데(연동 설계 §0.1) 이
        // 터미널은 WSL이다. 경로는 `--cd`의 별도 argv 값으로 그대로 전달되므로
        // 입력 경로를 프론트에서 변환하지 않는다.
        void startInTab(tabs.length === 0 ? null : activeTabId, selected, action.path);
        break;
      case "openProfile":
        void openProfileById(action.id);
        break;
      case "noop":
        console.info(`applink: ${action.reason}`);
        break;
    }
  };
  const handleOpenRequestRef = useRef(handleOpenRequest);
  handleOpenRequestRef.current = handleOpenRequest;

  // Cold start pulls take_pending_open once; a relaunch of this same running
  // instance arrives as the devbox://open event. Both converge on
  // handleOpenRequest so the two paths behave identically. Gated on
  // distro/profile/layout hydration이 끝나야 cold/hot profile 요청도 같은 상태를 본다.
  useEffect(() => {
    if (!distrosLoaded || !profilesLoaded || !workspaceReady) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const consumePendingOpen = () => {
      void takePendingOpen()
        .then((request) => {
          if (!disposed && request) handleOpenRequestRef.current(request);
        })
        .catch(() => undefined);
    };
    let coldStartConsumed = false;
    const consumeColdStart = () => {
      if (disposed || coldStartConsumed) return;
      coldStartConsumed = true;
      consumePendingOpen();
    };

    void onOpenRequest(() => consumePendingOpen())
      .then((stop) => {
        if (disposed) stop();
        else {
          unlisten = stop;
          consumeColdStart();
        }
      })
      .catch(() => {
        consumeColdStart();
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [distrosLoaded, profilesLoaded, workspaceReady]);

  // 핀이 켜져 있는 동안은 cwd가 바뀔 때마다 localStorage에 저장한다 (핀을 막 켠
  // 순간도 pinned가 deps에 있어 여기서 함께 처리된다).
  useEffect(() => {
    if (pinned) savePinnedCwd(cwd);
  }, [pinned, cwd]);

  const togglePinned = () => {
    setPinned((prev) => {
      const next = !prev;
      savePinned(next);
      return next;
    });
  };
  const {
    workspaceLoading,
    startInTab,
    openProfileById,
    startInTabRef,
    launchWorkspaceRef,
    saveCurrentProfile,
    openProfile,
    requestDeleteProfile,
    retryWorkspacePane,
  } = useWorkspaceLaunch({
    workspaceLoadingRef,
    logLensBusyRef,
    setError,
    cwd,
    startCommand,
    ask,
    multiplexer,
    workspaceRestoreGeneration,
    panesRef,
    stateRef,
    tabs,
    setPanes,
    setTabs,
    setActiveTabId,
    setActivePaneId,
    layoutSaveTimer,
    setRecentPaths,
    pinned,
    setCwd,
    refreshDashboard,
    mountedRef,
    setSelected,
    RESTORE_FAILURE_MESSAGE,
    contextActionBusyRef,
    setBroadcastOn,
    setBroadcastTargetIds,
    setBroadcastPickerOpen,
    setContextBusy,
    profiles,
    panes,
    activeTabId,
    activePaneId,
    setProfiles,
  });

  useEffect(() => {
    if (!distrosLoaded || restoreStarted.current) return;
    restoreStarted.current = true;
    const saved = loadLastWorkspace();
    if (!saved) {
      // Nothing to restore. Open one terminal in the default distro so the app that exists to
      // hold terminals does not start empty. Skipped when the distro collection failed, so a
      // failed hydration never starts a shell against a guessed distro.
      if (settingsRef.current.openTerminalOnStart && dashboardStateRef.current !== "error" && selectedRef.current) {
        void startInTabRef.current(null, selectedRef.current).finally(() => setWorkspaceReady(true));
        return;
      }
      setWorkspaceReady(true);
      return;
    }
    void launchWorkspaceRef
      .current(saved, { replaceExisting: false, label: "마지막 터미널 레이아웃" })
      .finally(() => setWorkspaceReady(true));
  }, [distrosLoaded, launchWorkspaceRef, startInTabRef]);

  useEffect(() => {
    if (!workspaceReady || workspaceLoading) return;
    window.clearTimeout(layoutSaveTimer.current);
    layoutSaveTimer.current = window.setTimeout(() => {
      void saveLastWorkspace(workspaceFromRuntime(tabs, panes, activeTabId, activePaneId)).catch(() => {
        setError("터미널 레이아웃을 저장하지 못했습니다. 실행 중인 터미널은 유지됩니다.");
      });
    }, 150);
    return () => window.clearTimeout(layoutSaveTimer.current);
  }, [activePaneId, activeTabId, panes, tabs, workspaceLoading, workspaceReady]);

  useEffect(() => {
    if (workspaceLoading) setPaletteOpen(false);
  }, [workspaceLoading]);

  // 분할이 활성 팬의 cwd를 물려받는 것과 같은 기대에 맞춘다. 툴바에 사용자가 직접 입력한
  // 경로가 있으면 그 값이 우선한다.
  const openNewTab = () => {
    const activePane = panes.find((pane) => pane.sessionId === activePaneId);
    const inherited = cwd.trim() ? undefined : activePane?.cwd;
    return startInTab(null, activePane?.distro ?? selected, inherited);
  };
  // 툴바 "+ Terminal"과 Ctrl+Shift+D는 같은 동작이다: 활성 탭이 있으면 분할 추가,
  // 없으면(앱을 막 띄운 직후) 새 탭을 만든다.
  const addPane = () => startInTab(tabs.length === 0 ? null : activeTabId, selected);

  const closePane = async (paneId: string): Promise<void> => {
    setError(null);
    const pane = panesRef.current.find((candidate) => paneIdentity(candidate) === paneId);
    if (!pane) return;
    if (pane.sessionId === null) {
      dropPane(paneId);
      return;
    }
    setContextBusy(true);
    try {
      await closeSession(pane.sessionId);
      dropPane(paneId);
      void refreshDashboard(true).catch(() => undefined);
    } catch {
      setError("터미널 팬을 닫지 못했습니다.");
    } finally {
      setContextBusy(false);
    }
  };

  const closeTabs = async (tabIds: readonly string[]): Promise<void> => {
    const ids = new Set(tabIds);
    const currentTabs = stateRef.current.tabs;
    const targets = currentTabs.filter((tab) => ids.has(tab.id));
    if (targets.length === 0) return;
    const targetPaneIds = new Set(targets.flatMap((tab) => tab.paneIds));
    const targetPanes = panesRef.current.filter((pane) => targetPaneIds.has(paneIdentity(pane)));
    const sessionIds = targetPanes.flatMap((pane) => (pane.sessionId ? [pane.sessionId] : []));
    const placeholderIds = targetPanes.flatMap((pane) => (pane.sessionId === null ? [pane.key] : []));
    setError(null);
    setContextBusy(true);
    try {
      const results = await Promise.allSettled(sessionIds.map((id) => closeSession(id)));
      const closedSessionIds = new Set(sessionIds.filter((_id, index) => results[index]?.status === "fulfilled"));
      const removedPaneIds = new Set([...closedSessionIds, ...placeholderIds]);
      const latestTabs = stateRef.current.tabs;
      const latestActiveTabId = stateRef.current.activeTabId;
      const latestActivePaneId = stateRef.current.activePaneId;
      const activeIndex = latestTabs.findIndex((tab) => tab.id === latestActiveTabId);
      const nextTabs =
        removedPaneIds.size === 0
          ? latestTabs
          : latestTabs
              .map((tab) => {
                const paneIds = tab.paneIds.filter((id) => !removedPaneIds.has(id));
                return paneIds.length === tab.paneIds.length
                  ? tab
                  : { ...tab, paneIds, sizing: normalizePaneSizing(undefined, tab.layout, paneIds.length) };
              })
              .filter((tab) => tab.paneIds.length > 0);

      if (removedPaneIds.size > 0) {
        // close_session 완료 이벤트가 먼저 도착했어도 멱등적이다. 닫기 중 팬이
        // 다른 탭으로 이동했거나 새 팬이 추가된 경우에도 성공한 session ID만 제거해
        // 최신 탭/팬 소유권을 보존한다.
        setPanes((previous) => {
          const next = previous.filter((pane) => !removedPaneIds.has(paneIdentity(pane)));
          panesRef.current = next;
          return next;
        });
        setTabs(nextTabs);
      }

      const activeTab = nextTabs.find((tab) => tab.id === latestActiveTabId);
      if (activeTab) {
        setActivePaneId(
          latestActivePaneId && activeTab.paneIds.includes(latestActivePaneId)
            ? latestActivePaneId
            : (activeTab.paneIds[activeTab.paneIds.length - 1] ?? null),
        );
      } else if (latestActiveTabId) {
        const fallback = nextTabs[Math.min(Math.max(activeIndex, 0), nextTabs.length - 1)] ?? null;
        setActiveTabId(fallback?.id ?? "");
        setActivePaneId(fallback?.paneIds[fallback.paneIds.length - 1] ?? null);
      }

      if (results.some((result) => result.status === "rejected")) {
        setError("터미널 탭을 모두 닫지 못했습니다.");
      }
      void refreshDashboard(true).catch(() => undefined);
    } finally {
      setContextBusy(false);
    }
  };

  const requestClosePane = async (paneId: string): Promise<void> => {
    const pane = panes.find((candidate) => paneIdentity(candidate) === paneId);
    if (!pane) return;
    if (settingsRef.current.confirmSinglePaneClose) {
      const confirmed = await ask({
        kind: "confirm",
        title: `'${pane.distro}' 터미널 팬을 닫을까요?`,
        lines:
          pane.sessionId === null
            ? ["실패한 복원 자리만 레이아웃에서 제거합니다."]
            : ["실행 중인 작업이 종료될 수 있습니다."],
        confirmLabel: "닫기",
        danger: true,
      });
      if (!confirmed.confirmed) return;
    }
    await closePane(paneId);
  };

  const requestCloseTab = async (tabId: string): Promise<void> => {
    const tab = tabs.find((candidate) => candidate.id === tabId);
    if (!tab) return;
    const confirmed = await ask({
      kind: "confirm",
      title: `'${tab.title}' 탭을 닫을까요?`,
      lines: [`터미널 ${tab.paneIds.length}개가 함께 닫히고 실행 중인 작업이 종료될 수 있습니다.`],
      confirmLabel: "닫기",
      danger: true,
    });
    if (!confirmed.confirmed) return;
    await closeTabs([tab.id]);
  };

  const requestCloseOtherTabs = async (tabId: string): Promise<void> => {
    const tab = tabs.find((candidate) => candidate.id === tabId);
    const otherTabs = tabs.filter((candidate) => candidate.id !== tabId);
    if (!tab || otherTabs.length === 0) return;
    const paneCount = otherTabs.reduce((total, candidate) => total + candidate.paneIds.length, 0);
    const confirmed = await ask({
      kind: "confirm",
      title: `'${tab.title}' 외 탭 ${otherTabs.length}개를 닫을까요?`,
      lines: [`터미널 ${paneCount}개가 함께 닫히고 실행 중인 작업이 종료될 수 있습니다.`],
      confirmLabel: "닫기",
      danger: true,
    });
    if (!confirmed.confirmed) return;
    await closeTabs(otherTabs.map((candidate) => candidate.id));
  };

  const activateTab = (tabId: string) => {
    setActiveTabId(tabId);
    const tab = tabs.find((t) => t.id === tabId);
    if (tab) {
      setActivePaneId((prev) =>
        prev && tab.paneIds.includes(prev) ? prev : (tab.paneIds[tab.paneIds.length - 1] ?? null),
      );
    }
  };

  const stepTab = (dir: 1 | -1) => {
    if (tabs.length === 0) return;
    const idx = tabs.findIndex((t) => t.id === activeTabId);
    const base = idx === -1 ? 0 : idx;
    const nextIdx = (base + dir + tabs.length) % tabs.length;
    activateTab(tabs[nextIdx].id);
  };

  const gotoTab = (index: number) => {
    const tab = tabs[index];
    if (tab) activateTab(tab.id);
  };

  const reorderTabs = (fromId: string, toId: string) => {
    setTabs((prev) => {
      const from = prev.find((t) => t.id === fromId);
      if (!from) return prev;
      const withoutFrom = prev.filter((t) => t.id !== fromId);
      const idx = withoutFrom.findIndex((t) => t.id === toId);
      if (idx === -1) return prev;
      const next = [...withoutFrom];
      next.splice(idx, 0, from);
      return next;
    });
  };

  const movePaneToTab = (paneId: string, targetTabId: string) => {
    const ownerIdx = tabs.findIndex((t) => t.paneIds.includes(paneId));
    if (ownerIdx === -1) return;
    const owner = tabs[ownerIdx];
    if (owner.id === targetTabId) return;

    const remaining = owner.paneIds.filter((id) => id !== paneId);
    const withoutOwnerPane =
      remaining.length === 0
        ? tabs.filter((t) => t.id !== owner.id)
        : tabs.map((tab) =>
            tab.id === owner.id
              ? {
                  ...tab,
                  paneIds: remaining,
                  sizing: normalizePaneSizing(undefined, tab.layout, remaining.length),
                }
              : tab,
          );
    const next = withoutOwnerPane.map((tab) => {
      if (tab.id !== targetTabId) return tab;
      const paneIds = [...tab.paneIds, paneId];
      return { ...tab, paneIds, sizing: normalizePaneSizing(undefined, tab.layout, paneIds.length) };
    });
    setTabs(next);
    setActiveTabId(targetTabId);
    setActivePaneId(paneId);
  };

  const setTabLayout = (tabId: string, layout: Layout) => {
    setTabs((prev) =>
      prev.map((tab) =>
        tab.id === tabId
          ? {
              ...tab,
              layout,
              sizing: normalizePaneSizing(undefined, layout, tab.paneIds.length),
            }
          : tab,
      ),
    );
  };

  const setTabSizing = useCallback((tabId: string, sizing: Tab["sizing"]) => {
    setTabs((previous) =>
      previous.map((tab) =>
        tab.id === tabId ? { ...tab, sizing: normalizePaneSizing(sizing, tab.layout, tab.paneIds.length) } : tab,
      ),
    );
  }, []);

  const setActiveTabLayout = (layout: Layout) => setTabLayout(activeTabId, layout);

  const focusPane = (direction: FocusDirection) => {
    const tab = tabs.find((t) => t.id === activeTabId);
    if (!tab || tab.paneIds.length === 0) return;
    const current = tab.paneIds.indexOf(activePaneId ?? "");
    if (current === -1) {
      setActivePaneId(tab.paneIds[0]);
      return;
    }
    // 화면상의 이웃으로만 옮긴다. 목록 순환이면 격자에서 오른쪽을 눌렀는데 아래 줄
    // 첫 팬으로 건너뛰는 일이 생긴다.
    const next = nextPaneIndex(tab.layout, tab.paneIds.length, current, direction);
    if (next !== null) setActivePaneId(tab.paneIds[next]);
  };

  const handleShortcut = (action: ShortcutAction) => {
    // Dialogs own the keyboard while open. This guard covers both the window listener and the
    // TermPane callback, including the frame before an in-app dialog moves focus away from xterm.
    const blockers = shortcutBlockersRef.current;
    if (blockers.pendingDialog || blockers.paletteOpen || blockers.settingsOpen || blockers.shortcutsOpen) return;
    if ((!workspaceReady || workspaceLoadingRef.current) && action.type !== "command-palette") return;
    switch (action.type) {
      case "new-tab":
        void openNewTab();
        break;
      case "new-pane":
        void addPane();
        break;
      case "command-palette":
        setPaletteOpen(true);
        break;
      case "close-pane":
        if (activePaneId && !contextActionBusy) void requestClosePane(activePaneId);
        break;
      case "next-tab":
        stepTab(1);
        break;
      case "prev-tab":
        stepTab(-1);
        break;
      case "goto-tab":
        gotoTab(action.index);
        break;
      case "focus-pane":
        focusPane(action.direction);
        break;
    }
  };

  const preparePaneContext = useCallback(
    (target: HTMLElement) => {
      const id = target.dataset.paneId;
      const pane = panes.find((candidate) => candidate.sessionId === id);
      const owner = tabs.find((tab) => id !== undefined && tab.paneIds.includes(id));
      if (!pane || !owner || !id) return;
      setContextPane(pane);
      setContextPaneCapabilities(
        terminalHandles.current.get(id)?.getCapabilities() ?? { hasSelection: false, hasCwd: false },
      );
      setActiveTabId(owner.id);
      setActivePaneId(id);
    },
    [panes, tabs],
  );
  const paneContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => preparePaneContext(target),
  });

  const prepareTabContext = useCallback(
    (target: HTMLElement) => {
      const id = target.dataset.tabId;
      const tab = tabs.find((candidate) => candidate.id === id);
      if (!tab) return;
      setContextTab(tab);
      setActiveTabId(tab.id);
      setActivePaneId((current) =>
        current && tab.paneIds.includes(current) ? current : (tab.paneIds[tab.paneIds.length - 1] ?? null),
      );
    },
    [tabs],
  );
  const tabContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareTabContext(target),
  });

  useEffect(() => {
    const id = contextPane?.sessionId;
    if (!id) return;
    const current = panes.find((pane) => pane.sessionId === id) ?? null;
    if (current) setContextPane(current);
    else {
      paneContextMenu.close();
      setContextPane(null);
    }
  }, [contextPane?.sessionId, paneContextMenu.close, panes]);

  useEffect(() => {
    const id = contextTab?.id;
    if (!id) return;
    const current = tabs.find((tab) => tab.id === id) ?? null;
    if (current) setContextTab(current);
    else {
      tabContextMenu.close();
      setContextTab(null);
    }
  }, [contextTab?.id, tabContextMenu.close, tabs]);

  const domainActionsBusy = contextActionBusy || workspaceLoading;
  const paneContextItems = useMemo<readonly ContextMenuEntry[]>(
    () =>
      buildPaneContextMenu({
        busy: domainActionsBusy,
        hasSelection: contextPaneCapabilities.hasSelection,
        hasCwd: contextPaneCapabilities.hasCwd,
        zoomed: zoomedPaneId !== null,
      }),
    [domainActionsBusy, contextPaneCapabilities.hasCwd, contextPaneCapabilities.hasSelection, zoomedPaneId],
  );
  const tabContextItems = useMemo<readonly ContextMenuEntry[]>(
    () => buildTabContextMenu(domainActionsBusy, tabs.length > 1),
    [domainActionsBusy, tabs.length],
  );

  const splitContextPane = (layout: "cols" | "rows") => {
    const pane = contextPane;
    const owner = tabs.find(
      (tab) => pane?.sessionId !== null && pane?.sessionId !== undefined && tab.paneIds.includes(pane.sessionId),
    );
    if (!pane || !owner || pane.sessionId === null) return;
    setContextBusy(true);
    void startInTab(owner.id, pane.distro, pane.cwd, "터미널 팬을 안전하게 분할하지 못했습니다.", {
      startCommand: null,
      multiplexer: pane.multiplexer,
    })
      .then((started) => {
        if (started) setTabLayout(owner.id, layout);
      })
      .finally(() => setContextBusy(false));
  };

  const renameContextTab = async (tab: Tab): Promise<void> => {
    const input = await ask({
      kind: "prompt",
      title: "탭 이름 변경",
      inputLabel: "탭 이름",
      defaultValue: tab.title,
      maxLength: 80,
      confirmLabel: "변경",
    });
    if (!input.confirmed) return;
    const name = normalizeTabName(input.value);
    if (!name) {
      setError("탭 이름은 비워둘 수 없습니다.");
      return;
    }
    setTabs((previous) =>
      previous.map((candidate) =>
        candidate.id === tab.id ? { ...candidate, title: name, customTitle: true } : candidate,
      ),
    );
  };

  const onPaneContextSelect = (id: string) => {
    if (workspaceLoadingRef.current) return;
    const pane = contextPane;
    if (!pane || pane.sessionId === null) return;
    const handle = terminalHandles.current.get(pane.sessionId);
    if (id === "copy") void handle?.copySelection();
    else if (id === "paste") void handle?.pasteClipboard();
    else if (id === "search") handle?.openSearch();
    else if (id === "copy-cwd") void handle?.copyCwd();
    else if (id === "select-all") handle?.selectAll();
    else if (id === "clear-scrollback") handle?.clearScrollback();
    else if (id === "scroll-bottom") handle?.scrollToBottom();
    else if (id === "zoom") setZoomedPaneId((current) => (current ? null : pane.sessionId));
    else if (id === "split-vertical") splitContextPane("cols");
    else if (id === "split-horizontal") splitContextPane("rows");
    else if (id === "close") void requestClosePane(pane.sessionId);
  };

  const onTabContextSelect = (id: string) => {
    if (workspaceLoadingRef.current) return;
    const tab = contextTab;
    if (!tab) return;
    if (id === "close") void requestCloseTab(tab.id);
    else if (id === "close-others") void requestCloseOtherTabs(tab.id);
    else if (id === "rename") void renameContextTab(tab);
    else if (id === "layout-grid") setTabLayout(tab.id, "grid");
    else if (id === "layout-cols") setTabLayout(tab.id, "cols");
    else if (id === "layout-rows") setTabLayout(tab.id, "rows");
  };

  const closePaneContextMenu = useCallback(() => {
    const paneId = contextPane?.sessionId;
    paneContextMenu.close();
    if (paneId) {
      window.setTimeout(() => paneFocus.current.get(paneId)?.(), 0);
    }
  }, [contextPane?.sessionId, paneContextMenu.close]);

  // 터미널 밖(탭 바, cwd 입력칸 등)에 포커스가 있을 때를 위한 전역 리스너.
  // handleShortcut이 tabs/activeTabId/cwd/selected/pinned 등 여러 상태를 참조하므로
  // deps 배열로 정확히 추적하는 대신, 매 렌더마다 재등록해 항상 최신 클로저를 쓴다
  // (단일 window 리스너 add/remove라 비용은 무시할 만하다).
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (isImeComposing(e)) return;
      const action = matchShortcut(e);
      if (!action) return;
      // 터미널에 포커스가 있으면 TermPane의 attachCustomKeyEventHandler가 이미
      // stopPropagation으로 처리했다 (여기 도달했다면 처리 안 된 경우에 대한 방어선).
      const el = document.activeElement;
      if (el instanceof HTMLElement && el.closest(".term-wrap")) return;
      e.preventDefault();
      handleShortcut(action);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  const activeTab = tabs.find((t) => t.id === activeTabId) ?? null;
  const activeLayout = activeTab?.layout ?? "grid";
  const activePaneIds = (activeTab?.paneIds ?? []).filter((id) => panes.some((pane) => pane.sessionId === id));
  const selectedBroadcastIds = activePaneIds.filter((id) => broadcastTargetIds.has(id));
  const broadcastReady = snapshotActionable && !workspaceLoading && !contextActionBusy && busy === null;

  useEffect(() => {
    if (!broadcastReady) setBroadcastOn(false);
  }, [broadcastReady]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: existing dependency list; review in P1-15
  useEffect(() => {
    const allowed = new Set(activePaneIds);
    const next = new Set([...broadcastTargetIds].filter((id) => allowed.has(id)));
    setBroadcastTargetIds(next);
    if (next.size < 2) setBroadcastOn(false);
    // 대상 변경은 active tab/pane identity 변화에만 반응한다. Set 자체는 deps에 넣으면
    // 이 effect가 만든 새 Set 때문에 다시 실행된다.
  }, [activeTabId, activePaneIds.join("|")]);

  const toggleBroadcastTarget = (id: string, checked: boolean) => {
    const next = nextBroadcastTargets(broadcastTargetIds, id, checked);
    if (!next) {
      setError(`동시 입력 대상은 최대 ${MAX_BROADCAST_TARGETS}개까지 선택할 수 있습니다.`);
      return;
    }
    setBroadcastTargetIds(next);
    if (next.size < 2) setBroadcastOn(false);
  };

  const splitActivePane = (layout: "cols" | "rows") => {
    const pane = panes.find((item) => item.sessionId === activePaneId);
    if (!pane || !activeTab) return;
    setContextBusy(true);
    void startInTab(activeTab.id, pane.distro, pane.cwd, "터미널 팬을 안전하게 분할하지 못했습니다.", {
      startCommand: null,
      multiplexer: pane.multiplexer,
    })
      .then((started) => {
        if (started) setTabLayout(activeTab.id, layout);
      })
      .finally(() => setContextBusy(false));
  };
  const { paletteActions } = buildTerminalPalette({
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
  });

  return (
    <div className="app">
      <TerminalToolbar
        panelOpen={panelOpen}
        updateSettings={updateSettings}
        logLensBusy={logLensBusy}
        selected={selected}
        selectDistro={selectDistro}
        distros={distros}
        cwd={cwd}
        setCwd={setCwd}
        addPane={addPane}
        startCommand={startCommand}
        setStartCommand={setStartCommand}
        recentPaths={recentPaths}
        pinned={pinned}
        togglePinned={togglePinned}
        contextActionBusy={contextActionBusy}
        workspaceLoading={workspaceLoading}
        workspaceReady={workspaceReady}
        setPaletteOpen={setPaletteOpen}
        setSettingsOpen={setSettingsOpen}
        setShortcutsOpen={setShortcutsOpen}
        broadcastReady={broadcastReady}
        broadcastOn={broadcastOn}
        selectedBroadcastIds={selectedBroadcastIds}
        setBroadcastOn={setBroadcastOn}
        broadcastPickerOpen={broadcastPickerOpen}
        activePaneIds={activePaneIds}
        setBroadcastPickerOpen={setBroadcastPickerOpen}
        activeLayout={activeLayout}
        domainActionsBusy={domainActionsBusy}
        setActiveTabLayout={setActiveTabLayout}
        LAYOUT_LABELS={LAYOUT_LABELS}
      />

      {broadcastPickerOpen && (
        <div id="broadcast-target-picker" className="broadcast-picker" role="group" aria-label="동시 입력 대상 팬 선택">
          <strong>동시 입력 대상</strong>
          <span className="dim">
            기본 꺼짐 · 최소 2개, 최대 {MAX_BROADCAST_TARGETS}개를 직접 선택해야 켤 수 있습니다.
          </span>
          {activePaneIds.map((id, index) => {
            const pane = panes.find((item) => item.sessionId === id);
            const checked = broadcastTargetIds.has(id);
            return (
              <label key={id}>
                <input
                  type="checkbox"
                  checked={checked}
                  disabled={!broadcastReady || (!checked && broadcastTargetIds.size >= MAX_BROADCAST_TARGETS)}
                  onChange={(event) => toggleBroadcastTarget(id, event.currentTarget.checked)}
                />
                {index + 1}. {pane?.title?.trim() || pane?.distro || "터미널"}
              </label>
            );
          })}
          {activePaneIds.length === 0 && <span className="dim">활성 탭에 팬이 없습니다.</span>}
        </div>
      )}

      <div className="main">
        {panelOpen && (
          <aside className="side-panel">
            <DistroPanel
              distros={distros}
              selectedDistro={selected}
              showDistroSelect={false}
              onSelectDistro={selectDistro}
              onOpenTerminal={openDistroTerminal}
              onOpenJournalInLogLens={(name) => void openJournalInLogLens(name)}
              onOpenFileInLogLens={(name) => void openFileInLogLens(name)}
              containers={containers}
              dockerMissing={dockerMissing}
              busy={busy}
              logLensBusy={logLensBusy}
              onAction={onDockerAction}
              onRefresh={() => {
                if (logLensBusyRef.current === null) {
                  void refreshDashboard().catch(() => undefined);
                  void refreshMultiplexers();
                }
              }}
              dashboardDistros={dashboardSnapshot?.distros}
              snapshotState={dashboardState}
              snapshotActionable={snapshotActionable}
            />
            <WorkspacePanel
              profiles={profiles}
              muxAvailability={muxAvailability}
              busy={workspaceLoading || contextActionBusy || logLensBusy !== null}
              onSaveCurrent={() => void saveCurrentProfile()}
              onOpen={(profile) => void openProfile(profile)}
              onDelete={(profile) => void requestDeleteProfile(profile)}
            />
          </aside>
        )}

        <div className="terminal-area">
          {error && (
            <div className="error" role="alert">
              <span>{error}</span>
              <button
                type="button"
                className="error-dismiss"
                aria-label="오류 메시지 닫기"
                onClick={() => setError(null)}
              >
                ✕
              </button>
            </div>
          )}

          <TabBar
            tabs={tabs}
            activeTabId={activeTabId}
            onActivate={activateTab}
            onClose={(tabId) => void requestCloseTab(tabId)}
            onRename={(tabId) => {
              const tab = tabs.find((candidate) => candidate.id === tabId);
              if (tab) void renameContextTab(tab);
            }}
            onReorder={reorderTabs}
            onDropPane={movePaneToTab}
            onNewTab={() => void openNewTab()}
            contextMenuTriggerProps={tabContextMenu.triggerProps}
            actionsDisabled={contextActionBusy || workspaceLoading || !selected}
          />

          {windowsBuildNumber !== undefined && (
            <PaneCanvas
              tabs={tabs}
              panes={panes}
              activeTabId={activeTabId}
              activePaneId={activePaneId}
              broadcastOn={broadcastOn && broadcastReady}
              broadcastTargetIds={selectedBroadcastIds}
              copyOnSelect={copyOnSelect}
              fontSize={terminalFontSize}
              fontFamily={fontFamilyFor(settings.fontId)}
              theme={TERMINAL_THEMES[settings.theme]}
              cursorStyle={settings.cursorStyle}
              cursorBlink={settings.cursorBlink}
              scrollbackLines={settings.scrollbackLines}
              registerWrite={registerWrite}
              unregisterWrite={unregisterWrite}
              registerFocus={registerFocus}
              unregisterFocus={unregisterFocus}
              registerTerminalHandle={registerTerminalHandle}
              unregisterTerminalHandle={unregisterTerminalHandle}
              onClosePane={(id) => void requestClosePane(id)}
              onRetryPane={(key) => void retryWorkspacePane(key)}
              ask={ask}
              onConfirmLinkHost={confirmLinkHost}
              onFocusPane={(id) => {
                setActivePaneId(id);
                const owner = tabs.find((t) => t.paneIds.includes(id));
                if (owner) setActiveTabId(owner.id);
              }}
              onShortcut={handleShortcut}
              onSizingChange={setTabSizing}
              onFontSizeChange={updateTerminalFontSize}
              onMetadataChange={updatePaneMetadata}
              onTerminalError={setError}
              onBroadcastFailure={() => setBroadcastOn(false)}
              windowsBuildNumber={windowsBuildNumber}
              contextMenuTriggerProps={paneContextMenu.triggerProps}
              actionsDisabled={contextActionBusy || workspaceLoading}
              zoomedPaneId={zoomedPaneId}
            />
          )}
        </div>
      </div>
      <ContextMenu
        open={paneContextMenu.open}
        anchor={paneContextMenu.anchor}
        restoreFocusTo={paneContextMenu.restoreFocusTo}
        items={paneContextItems}
        onSelect={onPaneContextSelect}
        onClose={closePaneContextMenu}
        ariaLabel="터미널 팬 메뉴"
      />
      <ContextMenu
        open={tabContextMenu.open}
        anchor={tabContextMenu.anchor}
        restoreFocusTo={tabContextMenu.restoreFocusTo}
        items={tabContextItems}
        onSelect={onTabContextSelect}
        onClose={tabContextMenu.close}
        ariaLabel="터미널 탭 메뉴"
      />
      <ActionPalette open={paletteOpen} actions={paletteActions} onClose={() => setPaletteOpen(false)} />
      <ShortcutReference open={shortcutsOpen} onClose={() => setShortcutsOpen(false)} />
      <SettingsPanel
        open={settingsOpen}
        settings={settings}
        quickSummonStatus={quickSummonStatus}
        onChange={updateSettings}
        onClose={() => setSettingsOpen(false)}
        muxAvailability={muxAvailability}
        muxScanning={muxScanning}
        onRefreshMux={() => void refreshMultiplexers()}
        copyOnSelect={copyOnSelect}
        onCopyOnSelectChange={(enabled) => {
          setCopyOnSelect(enabled);
          saveCopyOnSelect(enabled);
        }}
        fontSize={terminalFontSize}
        onFontSizeChange={updateTerminalFontSize}
        distro={selected}
        ask={ask}
        onError={setError}
      />
      <AppDialog pending={pendingDialog} onAnswer={answerDialog} />
    </div>
  );
}
