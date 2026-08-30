import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { isImeComposing } from "@devbox/a11y";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  closeSession,
  deleteWorkspaceProfile,
  detectMultiplexers,
  dockerAction,
  getDashboardSnapshot,
  getWindowsBuildNumber,
  listWorkspaceProfiles,
  onOpenRequest,
  onTerminalClosed,
  onTerminalOutput,
  openWslFileInLogLens,
  openWslJournalInLogLens,
  startSession,
  saveWorkspaceProfile,
  takePendingOpen,
} from "./api";
import DistroPanel from "./components/DistroPanel";
import ActionPalette, { type PaletteAction } from "./components/ActionPalette";
import PaneCanvas from "./components/PaneCanvas";
import type { TerminalPaneCapabilities, TerminalPaneHandle } from "./components/TermPane";
import TabBar from "./components/TabBar";
import WorkspacePanel from "./components/WorkspacePanel";
import { routeOpenRequest } from "./lib/applink";
import { makeId } from "./lib/id";
import { buildPaneContextMenu, buildTabContextMenu, normalizeTabName } from "./lib/contextMenu";
import { matchShortcut, type ShortcutAction } from "./lib/shortcuts";
import {
  MAX_BROADCAST_TARGETS,
  nextBroadcastTargets,
} from "./lib/broadcastSafety";
import {
  loadCopyOnSelect,
  loadPinned,
  loadPinnedCwd,
  loadRecentPaths,
  loadTerminalFontSize,
  pushRecentPath,
  saveCopyOnSelect,
  savePinned,
  savePinnedCwd,
  saveTerminalFontSize,
} from "./lib/storage";
import { nextTabTitle } from "./lib/tabTitle";
import {
  isSafeWorkspacePath,
  loadLastWorkspace,
  normalizeProfile,
  saveLastWorkspace,
  startCommandError,
  workspaceFromRuntime,
} from "./lib/workspace";
import {
  DEFAULT_TERMINAL_FONT_SIZE,
  clampTerminalFontSize,
} from "./lib/terminalUx";
import type {
  ContainerInfo,
  DashboardSnapshot,
  DistroInfo,
  Layout,
  MultiplexerAvailability,
  MultiplexerKind,
  OpenRequest,
  Pane,
  Tab,
  WorkspaceDefinition,
  WorkspaceProfile,
} from "./types";
import type { DashboardFreshness } from "./lib/resourceDisplay";
import "./App.css";

const DASHBOARD_ERROR_MESSAGE = "WSL resource snapshot을 갱신하지 못했습니다. 마지막 정상 상태를 유지합니다.";

export default function App() {
  const [distros, setDistros] = useState<DistroInfo[]>([]);
  const [selected, setSelected] = useState("");
  const [containers, setContainers] = useState<ContainerInfo[]>([]);
  const [dockerMissing, setDockerMissing] = useState(false);
  const [dashboardSnapshot, setDashboardSnapshot] = useState<DashboardSnapshot | null>(null);
  const [dashboardState, setDashboardState] = useState<DashboardFreshness>("loading");
  const [busy, setBusy] = useState<string | null>(null);
  const [logLensBusy, setLogLensBusy] = useState<string | null>(null);
  const [panelOpen, setPanelOpen] = useState(true);
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
  const [multiplexer, setMultiplexer] = useState<MultiplexerKind>("native");
  const [muxAvailability, setMuxAvailability] = useState<MultiplexerAvailability[]>([
    { kind: "native", status: "available", version: null, source: null },
    { kind: "tmux", status: "missing", version: null, source: null },
    { kind: "zellij", status: "missing", version: null, source: null },
  ]);
  const [profiles, setProfiles] = useState<WorkspaceProfile[]>([]);
  const [profilesLoaded, setProfilesLoaded] = useState(false);
  const [workspaceReady, setWorkspaceReady] = useState(false);
  const [workspaceLoading, setWorkspaceLoading] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [copyOnSelect, setCopyOnSelect] = useState(loadCopyOnSelect);
  const [terminalFontSize, setTerminalFontSize] = useState(loadTerminalFontSize);
  const [error, setError] = useState<string | null>(null);
  const [contextActionBusy, setContextActionBusy] = useState(false);
  const [contextPane, setContextPane] = useState<Pane | null>(null);
  const [contextPaneCapabilities, setContextPaneCapabilities] = useState<TerminalPaneCapabilities>({
    hasSelection: false,
    hasCwd: false,
  });
  const [contextTab, setContextTab] = useState<Tab | null>(null);
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
  const workspaceLoadingRef = useRef(false);
  const layoutSaveTimer = useRef<number | undefined>(undefined);
  const dashboardRequestRef = useRef<Promise<void> | null>(null);
  const dashboardRefreshQueuedRef = useRef(false);
  const dashboardRequestSequence = useRef(0);
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

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      logLensGeneration.current += 1;
      busyRef.current = null;
      logLensBusyRef.current = null;
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

  useEffect(() => {
    if (!selected) return;
    let disposed = false;
    void detectMultiplexers(selected)
      .then((availability) => {
        if (disposed) return;
        setMuxAvailability(availability);
        setMultiplexer((current) =>
          availability.some((item) => item.kind === current && item.status === "available") ? current : "native"
        );
      })
      .catch(() => {
        if (!disposed) {
          setMuxAvailability([
            { kind: "native", status: "available", version: null, source: null },
            { kind: "tmux", status: "error", version: null, source: null },
            { kind: "zellij", status: "error", version: null, source: null },
          ]);
          setMultiplexer("native");
        }
      });
    return () => {
      disposed = true;
    };
  }, [selected]);

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
        setDistros(next.distros.map(({ name, version, default: isDefault, state }) => ({
          name,
          version,
          default: isDefault,
          state,
        })));
        const fallback = next.distros.find((distro) => distro.default)?.name
          ?? next.distros[0]?.name
          ?? "Ubuntu";
        setSelected((previous) =>
          next.distros.some((distro) => distro.name === previous) ? previous : fallback,
        );
        setError((current) => (current === DASHBOARD_ERROR_MESSAGE ? null : current));
        setDashboardState("fresh");
        setDistrosLoaded(true);
      })
      .catch(() => {
        if (!dashboardMountedRef.current || sequence !== dashboardRequestSequence.current) return;
        // Keep the last good snapshot and its Docker/session/resource data. The terminal
        // transport remains usable; only broadcast is fail-closed by the shared status below.
        // Keep an error state even when the last snapshot is still younger than its normal TTL;
        // a failed poll must never silently re-enable broadcast on the next freshness tick.  On
        // the initial failure, leave distro hydration incomplete so profile restore cannot start
        // a guessed distro before the server-owned snapshot has established one.
        setDashboardState("error");
        setError(DASHBOARD_ERROR_MESSAGE);
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

  useEffect(() => {
    void refreshDashboard().catch(() => undefined);
    // The callback is intentionally stable: its single-flight state lives in refs and must not
    // be retriggered every time a new successful snapshot is committed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
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

  useEffect(() => {
    if (!dashboardSnapshot) return;
    const updateFreshness = () => {
      dashboardClockRef.current = Date.now();
      if (dashboardRequestRef.current) return;
      const stale = dashboardClockRef.current - dashboardSnapshot.capturedAtMs > dashboardSnapshot.staleAfterMs;
      setDashboardState((current) => (current === "error" ? current : stale ? "stale" : "fresh"));
    };
    updateFreshness();
    const timer = window.setInterval(updateFreshness, 1_000);
    return () => window.clearInterval(timer);
  }, [dashboardSnapshot]);

  useEffect(() => {
    if (!dashboardSnapshot) return;
    // Refresh before a successful snapshot can remain stale indefinitely. Bounds protect the
    // renderer from a corrupt/native-regressed TTL while the promise ref keeps this single-flight.
    const intervalMs = Math.min(60_000, Math.max(5_000, dashboardSnapshot.staleAfterMs));
    const timer = window.setInterval(() => {
      void refreshDashboard().catch(() => undefined);
    }, intervalMs);
    return () => window.clearInterval(timer);
  }, [dashboardSnapshot, refreshDashboard]);

  const onDockerAction = async (id: string, action: "start" | "stop" | "restart") => {
    if (busyRef.current !== null || logLensBusyRef.current !== null) return;
    const snapshot = dashboardSnapshot?.distros.find((distro) => distro.name === selected);
    if (
      !snapshot
      || dashboardState !== "fresh"
      || snapshot.dockerAvailability !== "available"
      || !snapshot.containers.some((container) => container.id === id)
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

  const openJournalInLogLens = (name: string) => {
    if (busyRef.current !== null
      || logLensBusyRef.current !== null
      || workspaceLoadingRef.current
      || contextActionBusy) return;
    if (!window.confirm(`'${name}'의 WSL journal을 Log Lens에서 읽기 전용으로 열까요?\n\n로그 원문·명령·자격 증명은 handoff에 포함되지 않습니다.`)) return;
    const generation = ++logLensGeneration.current;
    const token = ++logLensOperationToken.current;
    const operation = `log-lens-journal:${name}`;
    logLensBusyRef.current = operation;
    setLogLensBusy(operation);
    setError(null);
    void openWslJournalInLogLens(name, null)
      .then(() => {
        if (mountedRef.current
          && token === logLensOperationToken.current
          && generation === logLensGeneration.current) {
          setError(null);
        }
      })
      .catch(() => {
        if (mountedRef.current
          && token === logLensOperationToken.current
          && generation === logLensGeneration.current) {
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

  const openFileInLogLens = (name: string) => {
    if (busyRef.current !== null
      || logLensBusyRef.current !== null
      || workspaceLoadingRef.current
      || contextActionBusy) return;
    const entered = window.prompt("Log Lens에서 열 WSL 파일의 절대 경로를 입력하세요 (예: /var/log/app.log)");
    if (entered === null) return;
    const wslPath = entered.trim();
    if (!wslPath) {
      setError("WSL 파일 경로를 입력해야 합니다.");
      return;
    }
    if (!window.confirm(`'${name}'의 선택한 WSL 파일을 Log Lens에서 읽기 전용으로 열까요?\n\n경로는 검증된 WSL adapter 설정으로만 한 번 전달됩니다.`)) return;
    const generation = ++logLensGeneration.current;
    const token = ++logLensOperationToken.current;
    const operation = `log-lens-file:${name}`;
    logLensBusyRef.current = operation;
    setLogLensBusy(operation);
    setError(null);
    void openWslFileInLogLens(name, wslPath)
      .then(() => {
        if (mountedRef.current
          && token === logLensOperationToken.current
          && generation === logLensGeneration.current) {
          setError(null);
        }
      })
      .catch(() => {
        if (mountedRef.current
          && token === logLensOperationToken.current
          && generation === logLensGeneration.current) {
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

  // 팬 하나(세션)를 상태에서 제거한다. 마지막 팬이면 소속 탭도 함께 닫는다.
  // stateRef를 통해서만 tabs/activeTabId/activePaneId를 읽는다 — 위 주석 참고.
  const dropPane = useCallback((paneId: string) => {
    const { tabs: curTabs, activeTabId: curActiveTabId, activePaneId: curActivePaneId } = stateRef.current;
    setPanes((prev) => prev.filter((p) => p.sessionId !== paneId));

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
      : curTabs.map((t) => (t.id === owner.id ? { ...t, paneIds: remaining } : t));
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
    const unsubs: (() => void)[] = [];
    void onTerminalOutput(({ session_id, data }) => {
      writes.current.get(session_id)?.(data);
    }).then((u) => unsubs.push(u));
    void onTerminalClosed(({ session_id }) => {
      // 백엔드가 세션 리소스를 정리한 뒤 보내는 이벤트다. 여기서는 UI 상태만
      // 제거하고 close_session은 호출하지 않는다.
      dropPane(session_id);
      writes.current.delete(session_id);
    }).then((u) => unsubs.push(u));
    return () => unsubs.forEach((u) => u());
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

  /** tabId가 null이면 새 탭을, 아니면 그 탭에 분할 팬을 추가한다. 세션 시작이
   * 성공해야만 탭/팬을 만든다 — "탭은 항상 팬을 최소 1개 갖는다" 불변식의 근거.
   *
   * cwdOverride가 있으면 cwd 입력칸 대신 그 경로를 연다 — applink `path` 타깃(§1.4)이
   * 쓴다. 입력칸에 사용자가 입력 중이던 값은 건드리지 않는다(끝의 조건부 setCwd 참고). */
  const startInTab = async (
    tabId: string | null,
    distro: string,
    cwdOverride?: string,
    safeFailureMessage?: string,
    options?: {
      paneKey?: string;
      startCommand?: string | null;
      multiplexer?: MultiplexerKind;
    },
  ): Promise<boolean> => {
    if (workspaceLoadingRef.current || logLensBusyRef.current !== null) return false;
    setError(null);
    const usedCwd = (cwdOverride ?? cwd).trim() || undefined;
    const usedStartCommand = (options?.startCommand === undefined ? startCommand : options.startCommand)?.trim() || undefined;
    if (usedStartCommand) {
      const commandError = startCommandError(usedStartCommand);
      if (commandError) {
        setError(commandError);
        return false;
      }
      if (!window.confirm(`다음 시작 명령을 '${distro}' 터미널에서 실행할까요?\n\n${usedStartCommand}`)) {
        return false;
      }
    }
    const key = options?.paneKey ?? makeId("p");
    const requestedMultiplexer = options?.multiplexer ?? multiplexer;
    try {
      const started = await startSession(distro, usedCwd, key, requestedMultiplexer);
      const id = started.sessionId;
      setPanes((prev) => [...prev, {
        key,
        sessionId: id,
        distro,
        cwd: usedCwd,
        startCommand: usedStartCommand,
        initialCommand: started.resumed ? undefined : usedStartCommand,
        multiplexer: started.multiplexer,
      }]);

      if (tabId === null) {
        const title = nextTabTitle(tabs.map((t) => t.title), distro);
        const newTabId = makeId("t");
        setTabs((prev) => [...prev, { id: newTabId, title, customTitle: false, layout: "grid", paneIds: [id] }]);
        setActiveTabId(newTabId);
      } else {
        setTabs((prev) => prev.map((t) => (t.id === tabId ? { ...t, paneIds: [...t.paneIds, id] } : t)));
      }
      setActivePaneId(id);

      if (usedCwd) setRecentPaths(pushRecentPath(usedCwd));
      if (cwdOverride === undefined && !pinned) setCwd("");
      // A newly attached/started PTY changes the session half of the shared dashboard
      // generation. Refresh in the background; the terminal itself does not wait on WSL/Docker
      // polling and broadcast remains disabled until the new generation is fresh.
      void refreshDashboard(true).catch(() => undefined);
      return true;
    } catch {
      // Native PTY errors may contain the requested cwd, executable path or OS details. Keep
      // those implementation details out of the renderer; callers that need context provide a
      // fixed feature-specific message.
      setError(safeFailureMessage ?? "터미널을 시작하지 못했습니다.");
      return false;
    }
  };

  const launchWorkspace = async (
    workspace: WorkspaceDefinition,
    options: { replaceExisting: boolean; label: string },
  ): Promise<boolean> => {
    if (workspaceLoadingRef.current || logLensBusyRef.current !== null) return false;
    const oldSessionIds = panes.flatMap((pane) => pane.sessionId ? [pane.sessionId] : []);
    if (
      options.replaceExisting
      && oldSessionIds.length > 0
      && !window.confirm(`'${options.label}' 프로필로 전환할까요? 현재 터미널 ${oldSessionIds.length}개가 닫힙니다.`)
    ) return false;

    const commands = workspace.panes.flatMap((pane) => pane.startCommand
      ? [`[${pane.distro} · ${pane.key}] ${pane.startCommand}`]
      : []);
    const runStartCommands = commands.length === 0 || window.confirm(
      `다음 시작 명령 ${commands.length}개를 실행할까요?\n취소하면 레이아웃만 엽니다.\n\n${commands.join("\n")}`,
    );

    workspaceLoadingRef.current = true;
    setWorkspaceLoading(true);
    setError(null);
    const sessionByPaneKey = new Map<string, string>();
    const nextPanes: Pane[] = [];
    let failed = 0;
    try {
      // 프로필 하나가 과도한 동시 WSL 시작을 만들지 않도록 의도적으로 순차 실행한다.
      for (const definition of workspace.panes) {
        try {
          const started = await startSession(
            definition.distro,
            definition.cwd ?? undefined,
            definition.key,
            definition.multiplexer,
          );
          sessionByPaneKey.set(definition.key, started.sessionId);
          const command = runStartCommands ? (definition.startCommand ?? undefined) : undefined;
          nextPanes.push({
            key: definition.key,
            sessionId: started.sessionId,
            distro: definition.distro,
            cwd: definition.cwd ?? undefined,
            startCommand: definition.startCommand ?? undefined,
            initialCommand: started.resumed ? undefined : command,
            multiplexer: started.multiplexer,
          });
        } catch {
          failed += 1;
        }
      }

      const nextTabs: Tab[] = workspace.tabs.flatMap((definition) => {
        const paneIds = definition.paneKeys
          .map((key) => sessionByPaneKey.get(key))
          .filter((id): id is string => Boolean(id));
        return paneIds.length === 0 ? [] : [{
          id: definition.id,
          title: definition.title,
          customTitle: definition.customTitle,
          layout: definition.layout,
          paneIds,
        }];
      });
      if (nextTabs.length === 0) {
        await Promise.allSettled(nextPanes.flatMap((pane) => pane.sessionId ? [closeSession(pane.sessionId)] : []));
        setError("프로필의 터미널을 하나도 시작하지 못했습니다.");
        return false;
      }

      const nextActiveTab = nextTabs.find((tab) => tab.id === workspace.activeTabId) ?? nextTabs[0];
      const requestedActiveSession = workspace.activePaneKey
        ? sessionByPaneKey.get(workspace.activePaneKey)
        : undefined;
      const nextActivePane = requestedActiveSession && nextActiveTab.paneIds.includes(requestedActiveSession)
        ? requestedActiveSession
        : nextActiveTab.paneIds[nextActiveTab.paneIds.length - 1] ?? null;

      // terminal-closed 이벤트가 늦게 와도 새 workspace를 오래된 tab 상태로 제거하지 않게
      // ref를 React commit보다 먼저 새 identity로 전환한다.
      stateRef.current = {
        tabs: nextTabs,
        activeTabId: nextActiveTab.id,
        activePaneId: nextActivePane,
      };
      setPanes(nextPanes);
      setTabs(nextTabs);
      setActiveTabId(nextActiveTab.id);
      setActivePaneId(nextActivePane);
      setBroadcastOn(false);
      setBroadcastTargetIds(new Set());
      setBroadcastPickerOpen(false);
      const activeDefinition = nextActivePane
        ? workspace.panes.find((pane) => sessionByPaneKey.get(pane.key) === nextActivePane)
        : undefined;
      if (activeDefinition) setSelected(activeDefinition.distro);

      const closeResults = await Promise.allSettled(oldSessionIds.map((id) => closeSession(id)));
      const closeFailed = closeResults.filter((result) => result.status === "rejected").length;
      if (failed > 0 || closeFailed > 0) {
        const details = [
          failed > 0 ? `시작 실패 ${failed}개` : "",
          closeFailed > 0 ? `이전 세션 닫기 실패 ${closeFailed}개` : "",
        ].filter(Boolean).join(" · ");
        setError(`프로필을 부분적으로 열었습니다. ${details}`);
      }
      void refreshDashboard(true).catch(() => undefined);
      return true;
    } finally {
      workspaceLoadingRef.current = false;
      setWorkspaceLoading(false);
    }
  };

  const launchWorkspaceRef = useRef(launchWorkspace);
  launchWorkspaceRef.current = launchWorkspace;

  const openProfile = async (profile: WorkspaceProfile): Promise<boolean> =>
    launchWorkspace(profile, { replaceExisting: true, label: profile.name });

  const openProfileById = async (id: string): Promise<boolean> => {
    const profile = profiles.find((item) => item.id === id);
    if (!profile) {
      setError("요청한 터미널 프로필을 찾을 수 없습니다.");
      return false;
    }
    return openProfile(profile);
  };

  const saveCurrentProfile = async (): Promise<void> => {
    if (workspaceLoadingRef.current) return;
    if (panes.some((pane) => pane.cwd && !isSafeWorkspacePath(pane.cwd))) {
      setError("안전한 절대 경로가 아닌 cwd가 있어 프로필을 저장할 수 없습니다.");
      return;
    }
    const workspace = workspaceFromRuntime(tabs, panes, activeTabId, activePaneId);
    if (!workspace) {
      setError("저장할 터미널 레이아웃이 없습니다.");
      return;
    }
    const input = window.prompt("현재 터미널 레이아웃의 프로필 이름", "새 터미널 프로필");
    if (input === null) return;
    const name = input.trim();
    if (!name) {
      setError("프로필 이름은 비워둘 수 없습니다.");
      return;
    }
    workspaceLoadingRef.current = true;
    setWorkspaceLoading(true);
    setError(null);
    try {
      const saved = await saveWorkspaceProfile({ id: "", name, ...workspace });
      const normalized = normalizeProfile(saved);
      if (!normalized) throw new Error("invalid profile response");
      setProfiles((previous) => [...previous.filter((profile) => profile.id !== normalized.id), normalized]);
    } catch {
      setError("터미널 프로필을 저장하지 못했습니다.");
    } finally {
      workspaceLoadingRef.current = false;
      setWorkspaceLoading(false);
    }
  };

  const requestDeleteProfile = async (profile: WorkspaceProfile): Promise<void> => {
    if (workspaceLoadingRef.current || logLensBusyRef.current !== null) return;
    if (!window.confirm(`'${profile.name}' 터미널 프로필을 삭제할까요? 실행 중인 터미널은 닫히지 않습니다.`)) return;
    workspaceLoadingRef.current = true;
    setWorkspaceLoading(true);
    setError(null);
    try {
      await deleteWorkspaceProfile(profile.id);
      setProfiles((previous) => previous.filter((item) => item.id !== profile.id));
    } catch {
      setError("터미널 프로필을 삭제하지 못했습니다.");
    } finally {
      workspaceLoadingRef.current = false;
      setWorkspaceLoading(false);
    }
  };

  useEffect(() => {
    if (!distrosLoaded || restoreStarted.current) return;
    restoreStarted.current = true;
    const saved = loadLastWorkspace();
    if (!saved) {
      setWorkspaceReady(true);
      return;
    }
    void launchWorkspaceRef.current(saved, { replaceExisting: false, label: "마지막 터미널 레이아웃" })
      .finally(() => setWorkspaceReady(true));
  }, [distrosLoaded]);

  useEffect(() => {
    if (!workspaceReady || workspaceLoading) return;
    window.clearTimeout(layoutSaveTimer.current);
    layoutSaveTimer.current = window.setTimeout(() => {
      saveLastWorkspace(workspaceFromRuntime(tabs, panes, activeTabId, activePaneId));
    }, 150);
    return () => window.clearTimeout(layoutSaveTimer.current);
  }, [activePaneId, activeTabId, panes, tabs, workspaceLoading, workspaceReady]);

  useEffect(() => {
    if (workspaceLoading) setPaletteOpen(false);
  }, [workspaceLoading]);

  const openNewTab = () => startInTab(null, selected);
  // 툴바 "+ Terminal"과 Ctrl+Shift+D는 같은 동작이다: 활성 탭이 있으면 분할 추가,
  // 없으면(앱을 막 띄운 직후) 새 탭을 만든다.
  const addPane = () => startInTab(tabs.length === 0 ? null : activeTabId, selected);

  const closePane = async (paneId: string): Promise<void> => {
    setError(null);
    setContextActionBusy(true);
    try {
      await closeSession(paneId);
      dropPane(paneId);
      void refreshDashboard(true).catch(() => undefined);
    } catch {
      setError("터미널 팬을 닫지 못했습니다.");
    } finally {
      setContextActionBusy(false);
    }
  };

  const closeTabs = async (tabIds: readonly string[]): Promise<void> => {
    const ids = new Set(tabIds);
    const currentTabs = stateRef.current.tabs;
    const targets = currentTabs.filter((tab) => ids.has(tab.id));
    if (targets.length === 0) return;
    const sessionIds = targets.flatMap((tab) => tab.paneIds);
    setError(null);
    setContextActionBusy(true);
    try {
      const results = await Promise.allSettled(sessionIds.map((id) => closeSession(id)));
      const closedSessionIds = new Set(
        sessionIds.filter((_id, index) => results[index]?.status === "fulfilled"),
      );
      const latestTabs = stateRef.current.tabs;
      const latestActiveTabId = stateRef.current.activeTabId;
      const latestActivePaneId = stateRef.current.activePaneId;
      const activeIndex = latestTabs.findIndex((tab) => tab.id === latestActiveTabId);
      const nextTabs = closedSessionIds.size === 0
        ? latestTabs
        : latestTabs
            .map((tab) => ({
              ...tab,
              paneIds: tab.paneIds.filter((id) => !closedSessionIds.has(id)),
            }))
            .filter((tab) => tab.paneIds.length > 0);

      if (closedSessionIds.size > 0) {
        // close_session 완료 이벤트가 먼저 도착했어도 멱등적이다. 닫기 중 팬이
        // 다른 탭으로 이동했거나 새 팬이 추가된 경우에도 성공한 session ID만 제거해
        // 최신 탭/팬 소유권을 보존한다.
        setPanes((previous) => previous.filter(
          (pane) => pane.sessionId === null || !closedSessionIds.has(pane.sessionId),
        ));
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
      setContextActionBusy(false);
    }
  };

  const requestClosePane = (paneId: string) => {
    const pane = panes.find((candidate) => candidate.sessionId === paneId);
    if (!pane || !window.confirm(`'${pane.distro}' 터미널 팬을 닫을까요? 실행 중인 작업이 종료될 수 있습니다.`)) return;
    void closePane(paneId);
  };

  const requestCloseTab = (tabId: string) => {
    const tab = tabs.find((candidate) => candidate.id === tabId);
    if (!tab || !window.confirm(`'${tab.title}' 탭과 터미널 ${tab.paneIds.length}개를 닫을까요? 실행 중인 작업이 종료될 수 있습니다.`)) return;
    void closeTabs([tab.id]);
  };

  const requestCloseOtherTabs = (tabId: string) => {
    const tab = tabs.find((candidate) => candidate.id === tabId);
    const otherTabs = tabs.filter((candidate) => candidate.id !== tabId);
    if (!tab || otherTabs.length === 0) return;
    const paneCount = otherTabs.reduce((total, candidate) => total + candidate.paneIds.length, 0);
    if (!window.confirm(`'${tab.title}' 외 탭 ${otherTabs.length}개와 터미널 ${paneCount}개를 닫을까요?`)) return;
    void closeTabs(otherTabs.map((candidate) => candidate.id));
  };

  const activateTab = (tabId: string) => {
    setActiveTabId(tabId);
    const tab = tabs.find((t) => t.id === tabId);
    if (tab) {
      setActivePaneId((prev) => (prev && tab.paneIds.includes(prev) ? prev : (tab.paneIds[tab.paneIds.length - 1] ?? null)));
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
        : tabs.map((t) => (t.id === owner.id ? { ...t, paneIds: remaining } : t));
    const next = withoutOwnerPane.map((t) => (t.id === targetTabId ? { ...t, paneIds: [...t.paneIds, paneId] } : t));
    setTabs(next);
    setActiveTabId(targetTabId);
    setActivePaneId(paneId);
  };

  const setTabLayout = (tabId: string, layout: Layout) => {
    setTabs((prev) => prev.map((t) => (t.id === tabId ? { ...t, layout } : t)));
  };

  const setActiveTabLayout = (layout: Layout) => setTabLayout(activeTabId, layout);

  const focusPane = (dir: 1 | -1) => {
    const tab = tabs.find((t) => t.id === activeTabId);
    if (!tab || tab.paneIds.length === 0) return;
    const idx = tab.paneIds.indexOf(activePaneId ?? "");
    const base = idx === -1 ? (dir === 1 ? -1 : 0) : idx;
    const next = (base + dir + tab.paneIds.length) % tab.paneIds.length;
    setActivePaneId(tab.paneIds[next]);
  };

  const handleShortcut = (action: ShortcutAction) => {
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
        if (activePaneId && !contextActionBusy) requestClosePane(activePaneId);
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
        focusPane(action.dir);
        break;
    }
  };

  const preparePaneContext = useCallback((target: HTMLElement) => {
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
  }, [panes, tabs]);
  const paneContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => preparePaneContext(target),
  });

  const prepareTabContext = useCallback((target: HTMLElement) => {
    const id = target.dataset.tabId;
    const tab = tabs.find((candidate) => candidate.id === id);
    if (!tab) return;
    setContextTab(tab);
    setActiveTabId(tab.id);
    setActivePaneId((current) =>
      current && tab.paneIds.includes(current)
        ? current
        : (tab.paneIds[tab.paneIds.length - 1] ?? null),
    );
  }, [tabs]);
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
    () => buildPaneContextMenu({
      busy: domainActionsBusy,
      hasSelection: contextPaneCapabilities.hasSelection,
      hasCwd: contextPaneCapabilities.hasCwd,
    }),
    [domainActionsBusy, contextPaneCapabilities.hasCwd, contextPaneCapabilities.hasSelection],
  );
  const tabContextItems = useMemo<readonly ContextMenuEntry[]>(
    () => buildTabContextMenu(domainActionsBusy, tabs.length > 1),
    [domainActionsBusy, tabs.length],
  );

  const splitContextPane = (layout: "cols" | "rows") => {
    const pane = contextPane;
    const owner = tabs.find((tab) =>
      pane?.sessionId !== null && pane?.sessionId !== undefined && tab.paneIds.includes(pane.sessionId),
    );
    if (!pane || !owner || pane.sessionId === null) return;
    setContextActionBusy(true);
    void startInTab(
      owner.id,
      pane.distro,
      pane.cwd,
      "터미널 팬을 안전하게 분할하지 못했습니다.",
      { startCommand: null, multiplexer: pane.multiplexer },
    ).then((started) => {
      if (started) setTabLayout(owner.id, layout);
    }).finally(() => setContextActionBusy(false));
  };

  const renameContextTab = (tab: Tab) => {
    const input = window.prompt("탭 이름 변경", tab.title);
    if (input === null) return;
    const name = normalizeTabName(input);
    if (!name) {
      setError("탭 이름은 비워둘 수 없습니다.");
      return;
    }
    setTabs((previous) => previous.map((candidate) =>
      candidate.id === tab.id ? { ...candidate, title: name, customTitle: true } : candidate
    ));
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
    else if (id === "split-vertical") splitContextPane("cols");
    else if (id === "split-horizontal") splitContextPane("rows");
    else if (id === "close") requestClosePane(pane.sessionId);
  };

  const onTabContextSelect = (id: string) => {
    if (workspaceLoadingRef.current) return;
    const tab = contextTab;
    if (!tab) return;
    if (id === "close") requestCloseTab(tab.id);
    else if (id === "close-others") requestCloseOtherTabs(tab.id);
    else if (id === "rename") renameContextTab(tab);
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
  const activePaneIds = activeTab?.paneIds ?? [];
  const selectedBroadcastIds = activePaneIds.filter((id) => broadcastTargetIds.has(id));
  const broadcastReady = dashboardState === "fresh"
    && !workspaceLoading
    && !contextActionBusy
    && busy === null;

  useEffect(() => {
    if (!broadcastReady) setBroadcastOn(false);
  }, [broadcastReady]);

  useEffect(() => {
    const allowed = new Set(activePaneIds);
    const next = new Set([...broadcastTargetIds].filter((id) => allowed.has(id)));
    setBroadcastTargetIds(next);
    if (next.size < 2) setBroadcastOn(false);
    // 대상 변경은 active tab/pane identity 변화에만 반응한다. Set 자체는 deps에 넣으면
    // 이 effect가 만든 새 Set 때문에 다시 실행된다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
    setContextActionBusy(true);
    void startInTab(
      activeTab.id,
      pane.distro,
      pane.cwd,
      "터미널 팬을 안전하게 분할하지 못했습니다.",
      { startCommand: null, multiplexer: pane.multiplexer },
    ).then((started) => {
      if (started) setTabLayout(activeTab.id, layout);
    }).finally(() => setContextActionBusy(false));
  };

  const paletteActions: PaletteAction[] = [
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
      run: () => activePaneId && terminalHandles.current.get(activePaneId)?.openSearch(),
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
      id: "close-pane",
      label: "팬: 닫기",
      description: "실행 중인 작업이 종료될 수 있음",
      danger: true,
      run: () => {
        if (activePaneId) requestClosePane(activePaneId);
      },
    },
    ...profiles.map((profile): PaletteAction => ({
      id: `profile-${profile.id}`,
      label: `프로필 전환: ${profile.name}`,
      description: `${profile.tabs.length}개 탭 · ${profile.panes.length}개 팬`,
      run: () => void openProfile(profile),
    })),
  ];

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">WSL Desktop</h1>
        <button
          className={`btn panel-toggle ${panelOpen ? "active" : ""}`}
          title="사이드 패널 토글 (배포판/Docker/프로젝트)"
          onClick={() => setPanelOpen((prev) => !prev)}
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
        <select
          aria-label="세션 유지 방식"
          value={multiplexer}
          onChange={(event) => setMultiplexer(event.currentTarget.value as MultiplexerKind)}
          title="native는 외부 도구 없이 동작합니다. tmux/zellij는 설치된 경우에만 선택할 수 있습니다."
        >
          {muxAvailability.map((item) => (
            <option key={item.kind} value={item.kind} disabled={item.status !== "available"}>
              {item.kind}{item.kind === "native"
                ? " (기본)"
                : item.status === "available"
                  ? " (설치됨)"
                  : item.status === "missing"
                    ? " (없음)"
                    : " (확인 오류)"}
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
          placeholder="시작 명령 (선택, 프로필에 저장)"
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
        <button className="btn" disabled={contextActionBusy || workspaceLoading || logLensBusy !== null || !workspaceReady} onClick={() => void addPane()}>
          + 터미널
        </button>
        <button className="btn" title="명령 팔레트 (Ctrl+Shift+P)" onClick={() => setPaletteOpen(true)}>
          명령…
        </button>
        <span className="spacer" />
        <label
          className="toggle"
          title={broadcastReady
            ? "선택한 팬에 동시 입력을 보냅니다"
            : "최신 WSL snapshot이 준비될 때까지 동시 입력을 사용할 수 없습니다"}
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
        >대상 {selectedBroadcastIds.length}/{activePaneIds.length}</button>
        <label className="toggle" title="선택한 터미널 텍스트를 자동으로 복사합니다">
          <input
            type="checkbox"
            checked={copyOnSelect}
            onChange={(event) => {
              const enabled = event.currentTarget.checked;
              setCopyOnSelect(enabled);
              saveCopyOnSelect(enabled);
            }}
          />
          선택 시 복사
        </label>
        <div className="font-controls" aria-label="터미널 글꼴 크기">
          <button
            type="button"
            className="btn compact"
            title="글꼴 축소 (Ctrl+-)"
            onClick={() => updateTerminalFontSize(terminalFontSize - 1)}
          >A−</button>
          <button
            type="button"
            className="btn font-size"
            title="기본 글꼴 크기로 복원 (Ctrl+0)"
            onClick={() => updateTerminalFontSize(DEFAULT_TERMINAL_FONT_SIZE)}
          >{terminalFontSize}px</button>
          <button
            type="button"
            className="btn compact"
            title="글꼴 확대 (Ctrl++)"
            onClick={() => updateTerminalFontSize(terminalFontSize + 1)}
          >A+</button>
        </div>
        {(["grid", "cols", "rows"] as const).map((l) => (
          <button key={l} className={`btn ${activeLayout === l ? "active" : ""}`} disabled={domainActionsBusy} onClick={() => setActiveTabLayout(l)}>
            {l}
          </button>
        ))}
      </header>

      {broadcastPickerOpen && (
        <div id="broadcast-target-picker" className="broadcast-picker" role="group" aria-label="동시 입력 대상 팬 선택">
          <strong>동시 입력 대상</strong>
          <span className="dim">기본 꺼짐 · 최소 2개, 최대 {MAX_BROADCAST_TARGETS}개를 직접 선택해야 켤 수 있습니다.</span>
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
              onSelectDistro={selectDistro}
              onOpenTerminal={openDistroTerminal}
              onOpenJournalInLogLens={openJournalInLogLens}
              onOpenFileInLogLens={openFileInLogLens}
              containers={containers}
              dockerMissing={dockerMissing}
              busy={busy}
              logLensBusy={logLensBusy}
              onAction={onDockerAction}
              onRefresh={() => {
                if (logLensBusyRef.current === null) void refreshDashboard().catch(() => undefined);
              }}
              dashboardDistros={dashboardSnapshot?.distros}
              snapshotState={dashboardState}
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
          {error && <div className="error" role="alert" aria-live="assertive">{error}</div>}

          <TabBar
            tabs={tabs}
            activeTabId={activeTabId}
            onActivate={activateTab}
            onClose={requestCloseTab}
            onReorder={reorderTabs}
            onDropPane={movePaneToTab}
            onNewTab={() => void openNewTab()}
            contextMenuTriggerProps={tabContextMenu.triggerProps}
            actionsDisabled={contextActionBusy || workspaceLoading}
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
              registerWrite={registerWrite}
              unregisterWrite={unregisterWrite}
              registerFocus={registerFocus}
              unregisterFocus={unregisterFocus}
              registerTerminalHandle={registerTerminalHandle}
              unregisterTerminalHandle={unregisterTerminalHandle}
              onClosePane={requestClosePane}
              onFocusPane={(id) => {
                setActivePaneId(id);
                const owner = tabs.find((t) => t.paneIds.includes(id));
                if (owner) setActiveTabId(owner.id);
              }}
              onShortcut={handleShortcut}
              onFontSizeChange={updateTerminalFontSize}
              onMetadataChange={updatePaneMetadata}
              onTerminalError={setError}
              onBroadcastFailure={() => setBroadcastOn(false)}
              windowsBuildNumber={windowsBuildNumber}
              contextMenuTriggerProps={paneContextMenu.triggerProps}
              actionsDisabled={contextActionBusy || workspaceLoading}
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
    </div>
  );
}
