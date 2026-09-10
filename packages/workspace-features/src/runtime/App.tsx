import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  getProcessInfo,
  handoffContainerStop,
  loadPortManagerPreferences,
  killListener,
  listPorts,
  listPortObservations,
  openPortLog,
  openPortOwner,
  openBrowser,
  revealProcess,
  savePortManagerPreferences,
} from "./api";
import type {
  ListenerActionResult,
  ListenerKillRequest,
  LogStream,
  PortManagerPreferences,
  PortCorrelation,
  PortObservationSnapshot,
  PortRow,
  ProtoFilter,
  SnapshotSourceStatus,
  StateFilter,
} from "./types";
import {
  DEFAULT_PREFERENCES,
  MAX_FAVORITES_PER_KIND,
  appendRefreshTimeline,
  MAX_REFRESH_INTERVAL_MS,
  MIN_REFRESH_INTERVAL_MS,
  diffPortRows,
  isPinnedRow,
  isPortFavorite,
  isProcessFavorite,
  portFavoriteFor,
  processFavoriteFor,
  sameProcessFavorite,
  type RefreshTimelineEvent,
  type RefreshTimelineRow,
} from "./refresh";
import "./App.css";

const PROTO_FILTERS: { value: ProtoFilter; label: string }[] = [
  { value: "all", label: "전체" },
  { value: "tcp", label: "TCP" },
  { value: "udp", label: "UDP" },
];

const STATE_FILTERS: { value: StateFilter; label: string }[] = [
  { value: "all", label: "모든 상태" },
  { value: "listening", label: "LISTENING" },
  { value: "established", label: "ESTABLISHED" },
];

const REFRESH_INTERVALS = [1_000, 2_000, 5_000, 10_000, 30_000, 60_000];

export function matches(row: PortRow, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return (
    row.proto.toLowerCase().includes(q) ||
    row.state.toLowerCase().includes(q) ||
    String(row.port).includes(q) ||
    (row.pid != null && String(row.pid).includes(q)) ||
    row.local_addr.toLowerCase().includes(q) ||
    (row.process_name?.toLowerCase().includes(q) ?? false) ||
    (row.command_line?.toLowerCase().includes(q) ?? false) ||
    (row.executable_path?.toLowerCase().includes(q) ?? false) ||
    (row.wsl_distro?.toLowerCase().includes(q) ?? false) ||
    (row.container_id?.toLowerCase().includes(q) ?? false) ||
    (row.container_name?.toLowerCase().includes(q) ?? false) ||
    (row.correlations?.some((correlation) =>
      [
        correlation.source_app,
        correlation.target_kind,
        correlation.target_id,
        correlation.label,
        correlation.confidence,
      ].some((value) => value.toLowerCase().includes(q)),
    ) ?? false)
  );
}

export function portRowKey(row: PortRow): string {
  const identity = row.identity;
  if (identity?.kind === "windows") {
    return (
      row.proto +
      ":" +
      row.local_addr +
      ":" +
      identity.pid +
      ":" +
      identity.start_time
    );
  }
  if (identity?.kind === "wsl") {
    return (
      row.proto +
      ":" +
      row.local_addr +
      ":" +
      identity.distro +
      ":" +
      identity.pid +
      ":" +
      identity.start_tick
    );
  }
  if (identity?.kind === "container") {
    return (
      row.proto +
      ":" +
      row.local_addr +
      ":" +
      identity.engine +
      ":" +
      identity.distro +
      ":" +
      identity.container_id
    );
  }
  // An identity-less row is keyed by its complete endpoint and source.  A
  // PID-only fallback can collide for two source rows or for malformed
  // fixtures whose address does not include the port.
  return (
    (row.source ?? "windows") +
    ":" +
    row.proto +
    ":" +
    row.local_addr +
    ":" +
    row.port +
    ":" +
    (row.pid ?? 0)
  );
}

export function localhostUrl(row: PortRow): string | null {
  return row.port > 0 ? "http://localhost:" + row.port : null;
}

export function listenerKillRequest(row: PortRow): ListenerKillRequest | null {
  if (!row.identity) return null;
  return {
    endpoint: {
      proto: row.proto,
      local_addr: row.local_addr,
      port: row.port,
      state: row.state,
    },
    identity: row.identity,
  };
}

export function sourceLabel(row: PortRow): string {
  switch (row.source ?? "windows") {
    case "wsl":
      return "WSL";
    case "container":
      return "컨테이너";
    default:
      return "Windows";
  }
}

export function provenanceLabel(row: PortRow): string {
  const source = sourceLabel(row);
  if (row.source === "wsl" && row.wsl_distro) return source + " · " + row.wsl_distro;
  if (row.source === "container") {
    const engine = row.container_engine ?? "container";
    const distro = row.wsl_distro ? " · " + row.wsl_distro : "";
    const container = row.container_id ? " · " + row.container_id : "";
    return source + " · " + engine + distro + container;
  }
  return source;
}

export function safeActionError(_error: unknown): string {
  return "작업을 완료하지 못했습니다. 목록을 새로 고친 후 다시 시도하세요.";
}

export function shouldIgnoreComposingShortcut(isComposing: boolean, key: string): boolean {
  return isComposing && (key === "Enter" || key === " " || key === "F10");
}

export function isCurrentRequest(request: number, current: number): boolean {
  return request === current;
}

export function correlationSummary(correlation: PortCorrelation): string {
  return [
    correlation.source_app,
    correlation.target_kind,
    correlation.target_id,
  ].join(" · ");
}

export function sourceStatusLabel(source: SnapshotSourceStatus): string {
  const freshness =
    source.freshness_ms == null ? "최신 상태 알 수 없음" : `${source.freshness_ms}ms 전`;
  return `${source.producer} · ${source.state} · ${freshness}`;
}

function ownerLabels(row: RefreshTimelineRow | undefined): string {
  const labels = row?.owner_labels ?? [];
  return labels.length > 0 ? labels.join(", ") : "소유자 없음";
}

function timelineTime(observedAtMs: number): string {
  const date = new Date(observedAtMs);
  return Number.isFinite(date.getTime())
    ? date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })
    : "시간 알 수 없음";
}

async function readObservationSnapshot(): Promise<PortObservationSnapshot> {
  // The native build always exposes list_port_observations. Keeping this
  // narrow adapter makes older browser/test harnesses that only provide the
  // listener list continue to render without inventing source diagnostics.
  if (typeof listPortObservations === "function") return listPortObservations();
  return { rows: await listPorts(), sources: [], correlations_truncated: false };
}

function isListening(row: PortRow): boolean {
  return row.state.toLowerCase() === "listening";
}

function isListener(row: PortRow): boolean {
  const state = row.state.toLowerCase();
  return state === "" || state === "listening" || state === "unconn" || state === "bound";
}

function matchesStateFilter(row: PortRow, filter: StateFilter): boolean {
  if (filter === "all") return true;
  if (filter === "listening") return isListener(row);
  return row.state.toLowerCase() === filter;
}

function displayValue(value: string | number | null | undefined): string {
  return value === null || value === undefined || value === "" ? "-" : String(value);
}

function clonePreferences(value: PortManagerPreferences): PortManagerPreferences {
  const rawInterval = Number(value?.refresh_interval_ms);
  const interval = Number.isFinite(rawInterval)
    ? Math.round(rawInterval)
    : DEFAULT_PREFERENCES.refresh_interval_ms;
  const favoritePorts = Array.isArray(value?.favorite_ports) ? value.favorite_ports : [];
  const favoriteProcesses = Array.isArray(value?.favorite_processes)
    ? value.favorite_processes
    : [];
  return {
    schema_version: 1,
    refresh_interval_ms: Math.min(
      MAX_REFRESH_INTERVAL_MS,
      Math.max(MIN_REFRESH_INTERVAL_MS, interval),
    ),
    pinned_only: value?.pinned_only === true,
    favorite_ports: favoritePorts.slice(0, MAX_FAVORITES_PER_KIND),
    favorite_processes: favoriteProcesses.slice(0, MAX_FAVORITES_PER_KIND),
  };
}

type ProcessPathState = {
  rowKey: string;
  path: string | null;
};

export default function App() {
  const [ports, setPorts] = useState<PortRow[]>([]);
  const [query, setQuery] = useState("");
  const [protoFilter, setProtoFilter] = useState<ProtoFilter>("all");
  const [stateFilter, setStateFilter] = useState<StateFilter>("all");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busyRowKey, setBusyRowKey] = useState<string | null>(null);
  const [selectedRowKey, setSelectedRowKey] = useState<string | null>(null);
  const [contextRow, setContextRow] = useState<PortRow | null>(null);
  const [processPath, setProcessPath] = useState<ProcessPathState | null>(null);
  const [handoff, setHandoff] = useState<string | null>(null);
  const [isComposing, setIsComposing] = useState(false);
  const [preferences, setPreferences] = useState<PortManagerPreferences>(DEFAULT_PREFERENCES);
  const [preferencesReady, setPreferencesReady] = useState(false);
  const [preferencesSaving, setPreferencesSaving] = useState(false);
  const [settingsWarning, setSettingsWarning] = useState<string | null>(null);
  const [autoRefreshPaused, setAutoRefreshPaused] = useState(false);
  const [snapshotHealthy, setSnapshotHealthy] = useState(false);
  const [sources, setSources] = useState<SnapshotSourceStatus[]>([]);
  const [correlationsTruncated, setCorrelationsTruncated] = useState(false);
  const [timeline, setTimeline] = useState<RefreshTimelineEvent[]>([]);
  const [hasComparedSnapshot, setHasComparedSnapshot] = useState(false);
  const [busyCorrelationAction, setBusyCorrelationAction] = useState<string | null>(null);
  const processPathRequest = useRef(0);
  const refreshRequest = useRef(0);
  const refreshInFlight = useRef<Promise<void> | null>(null);
  const previousSnapshot = useRef<PortRow[] | null>(null);
  const timelineRef = useRef<RefreshTimelineEvent[]>([]);
  const busyActionRef = useRef<string | null>(null);
  const busyCorrelationActionRef = useRef<string | null>(null);
  const snapshotHealthyRef = useRef(false);
  const preferencesRef = useRef<PortManagerPreferences>(DEFAULT_PREFERENCES);
  const preferenceSaveInFlight = useRef(false);
  const preferenceSaveRequest = useRef(0);
  const initializeRequest = useRef(0);
  const mounted = useRef(false);

  const markSnapshotHealthy = useCallback((healthy: boolean) => {
    snapshotHealthyRef.current = healthy;
    setSnapshotHealthy(healthy);
  }, []);

  const refresh = useCallback(async () => {
    if (!mounted.current) return;
    if (refreshInFlight.current) return refreshInFlight.current;
    const request = ++refreshRequest.current;
    setLoading(true);
    setError(null);

    const operation = (async () => {
      try {
        const observation = await readObservationSnapshot();
        const next = observation.rows;
        if (mounted.current && refreshRequest.current === request) {
          const prior = previousSnapshot.current;
          previousSnapshot.current = next;
          setPorts(next);
          const changes = diffPortRows(prior, next);
          if (prior !== null) {
            const nextTimeline = appendRefreshTimeline(timelineRef.current, changes);
            timelineRef.current = nextTimeline;
            setTimeline(nextTimeline);
          }
          setSources(observation.sources);
          setCorrelationsTruncated(observation.correlations_truncated);
          setHasComparedSnapshot(prior !== null);
          markSnapshotHealthy(true);

          const nextByKey = new Map(next.map((row) => [portRowKey(row), row]));
          setSelectedRowKey((selected) =>
            selected && nextByKey.has(selected) ? selected : null,
          );
          setContextRow((current) => {
            if (!current) return null;
            return nextByKey.get(portRowKey(current)) ?? null;
          });
          processPathRequest.current += 1;
          setProcessPath(null);
        }
      } catch (caught) {
        if (mounted.current && refreshRequest.current === request) {
          // Keep the last stable rows and favorites, but fail closed for
          // process actions until a complete snapshot succeeds again.
          markSnapshotHealthy(false);
          setError(safeActionError(caught));
        }
      } finally {
        if (mounted.current && refreshRequest.current === request) {
          setLoading(false);
        }
      }
    })();
    refreshInFlight.current = operation;
    try {
      await operation;
    } finally {
      if (refreshInFlight.current === operation) {
        refreshInFlight.current = null;
      }
    }
  }, [markSnapshotHealthy]);

  const refreshAfterMutation = useCallback(async () => {
    // A timer poll may have captured its native snapshot before the listener
    // mutation completed. Wait for that single-flight operation and then
    // require a fresh call so a successful Kill never leaves pre-kill rows on
    // screen until the next interval.
    const inFlight = refreshInFlight.current;
    if (inFlight) await inFlight;
    if (mounted.current) await refresh();
  }, [refresh]);

  const savePreferences = useCallback(async (next: PortManagerPreferences) => {
    if (!mounted.current || preferenceSaveInFlight.current) return;
    if (
      next.favorite_ports.length > MAX_FAVORITES_PER_KIND ||
      next.favorite_processes.length > MAX_FAVORITES_PER_KIND
    ) {
      setError("즐겨찾기는 종류별로 제한됩니다. 새 항목을 추가하려면 하나를 먼저 제거하세요.");
      return;
    }
    const safe = clonePreferences(next);
    const request = ++preferenceSaveRequest.current;
    preferenceSaveInFlight.current = true;
    setPreferencesSaving(true);
    try {
      await savePortManagerPreferences(safe);
      if (mounted.current && preferenceSaveRequest.current === request) {
        preferencesRef.current = safe;
        setPreferences(safe);
        setSettingsWarning(null);
      }
    } catch (caught) {
      if (mounted.current && preferenceSaveRequest.current === request) {
        setError(safeActionError(caught));
      }
    } finally {
      if (preferenceSaveRequest.current === request) {
        preferenceSaveInFlight.current = false;
        if (mounted.current) setPreferencesSaving(false);
      }
    }
  }, []);

  const togglePortFavorite = useCallback(
    (row: PortRow) => {
      if (row.port <= 0) return;
      const current = preferencesRef.current;
      const favorite = portFavoriteFor(row);
      const exists = isPortFavorite(row, current.favorite_ports);
      const favorite_ports = exists
        ? current.favorite_ports.filter(
            (candidate) =>
              !(
                candidate.source === favorite.source &&
                candidate.proto === favorite.proto &&
                candidate.local_addr === favorite.local_addr &&
                candidate.port === favorite.port
              ),
          )
        : [...current.favorite_ports, favorite];
      void savePreferences({ ...current, favorite_ports });
    },
    [savePreferences],
  );

  const toggleProcessFavorite = useCallback(
    (row: PortRow) => {
      const favorite = processFavoriteFor(row);
      if (!favorite) return;
      const current = preferencesRef.current;
      const exists = isProcessFavorite(row, current.favorite_processes);
      const favorite_processes = exists
        ? current.favorite_processes.filter(
            (candidate) => !sameProcessFavorite(candidate, favorite),
          )
        : [...current.favorite_processes, favorite];
      void savePreferences({ ...current, favorite_processes });
    },
    [savePreferences],
  );

  const initialize = useCallback(async () => {
    const request = ++initializeRequest.current;
    try {
      const loaded = clonePreferences(await loadPortManagerPreferences());
      if (!mounted.current || initializeRequest.current !== request) return;
      preferencesRef.current = loaded;
      setPreferences(loaded);
    } catch {
      if (!mounted.current || initializeRequest.current !== request) return;
      preferencesRef.current = clonePreferences(DEFAULT_PREFERENCES);
      setPreferences(clonePreferences(DEFAULT_PREFERENCES));
      setSettingsWarning("저장된 보기 설정을 사용할 수 없어 안전한 기본값을 적용했습니다.");
    } finally {
      if (mounted.current && initializeRequest.current === request) setPreferencesReady(true);
    }
    if (!mounted.current || initializeRequest.current !== request) return;
    await refresh();
  }, [refresh]);

  useEffect(() => {
    mounted.current = true;
    previousSnapshot.current = null;
    timelineRef.current = [];
    setPreferencesReady(false);
    setSources([]);
    setCorrelationsTruncated(false);
    setTimeline([]);
    setHasComparedSnapshot(false);
    markSnapshotHealthy(false);
    void initialize();
    return () => {
      mounted.current = false;
      initializeRequest.current += 1;
      refreshRequest.current += 1;
      refreshInFlight.current = null;
      previousSnapshot.current = null;
      timelineRef.current = [];
      processPathRequest.current += 1;
      busyActionRef.current = null;
      busyCorrelationActionRef.current = null;
      preferenceSaveRequest.current += 1;
      preferenceSaveInFlight.current = false;
      snapshotHealthyRef.current = false;
    };
  }, [initialize, markSnapshotHealthy]);

  useEffect(() => {
    if (!preferencesReady || autoRefreshPaused) return;
    const timer = window.setInterval(() => {
      void refresh();
    }, preferences.refresh_interval_ms);
    return () => window.clearInterval(timer);
  }, [autoRefreshPaused, preferences.refresh_interval_ms, preferencesReady, refresh]);

  const visible = useMemo(() => {
    return ports.filter(
      (row) =>
        matches(row, query) &&
        (protoFilter === "all" || row.proto.toLowerCase().startsWith(protoFilter)) &&
        matchesStateFilter(row, stateFilter) &&
        (!preferences.pinned_only || isPinnedRow(row, preferences)),
    );
  }, [ports, preferences, query, protoFilter, stateFilter]);

  const counts = useMemo(() => {
    const listening = ports.filter((p) => isListener(p)).length;
    return { total: ports.length, listening };
  }, [ports]);

  const unhealthySources = useMemo(
    () => sources.filter((source) => source.state !== "available"),
    [sources],
  );

  const runAction = useCallback(async (action: () => Promise<void>) => {
    if (!mounted.current) return;
    setError(null);
    try {
      await action();
    } catch (caught) {
      if (mounted.current) setError(safeActionError(caught));
    }
  }, []);

  const onKill = async (row: PortRow) => {
    if (!mounted.current || busyActionRef.current !== null) return;
    if (!snapshotHealthyRef.current) {
      setError("리스너 스냅샷을 사용할 수 없습니다. 목록을 새로 고친 후 시도하세요.");
      return;
    }
    const request = listenerKillRequest(row);
    if (!request || !isListener(row)) {
      setError("식별 정보를 사용할 수 없습니다. 목록을 새로 고친 후 시도하세요.");
      return;
    }
    const processLabel = row.process_name ? " (" + row.process_name + ")" : "";
    const actionLabel = row.source === "container" ? "WSL Desktop에서 중지" : "리스너 종료";
    if (!window.confirm(row.local_addr + processLabel + " " + actionLabel + "할까요?")) return;

    const rowKey = portRowKey(row);
    busyActionRef.current = rowKey;
    setBusyRowKey(rowKey);
    setError(null);
    setHandoff(null);
    try {
      const result: ListenerActionResult =
        row.source === "container"
          ? { kind: "handoff", handoff: await handoffContainerStop(request) }
          : await killListener(request);
      if (result.kind === "handoff" && mounted.current) {
        setHandoff(
          "WSL Desktop에서 " + result.handoff.container_id + " 컨테이너를 중지하세요.",
        );
      } else {
        await refreshAfterMutation();
      }
    } catch (caught) {
      if (mounted.current) setError(safeActionError(caught));
    } finally {
      if (busyActionRef.current === rowKey) {
        busyActionRef.current = null;
        if (mounted.current) setBusyRowKey(null);
      }
    }
  };

  const onOpen = async (row: PortRow) => {
    const url = localhostUrl(row);
    if (!url || !isListening(row)) return;
    await runAction(() => openBrowser(url));
  };

  const onOpenCorrelation = useCallback(
    async (correlation: PortCorrelation, stream?: LogStream) => {
      if (!mounted.current || !correlation.action_key) return;
      const busyKey = `${correlation.action_key}:${stream ?? "owner"}`;
      if (busyCorrelationActionRef.current !== null) return;
      busyCorrelationActionRef.current = busyKey;
      setBusyCorrelationAction(busyKey);
      setError(null);
      try {
        if (stream) {
          await openPortLog(correlation.action_key, stream);
        } else {
          await openPortOwner(correlation.action_key);
        }
      } catch (caught) {
        if (mounted.current) setError(safeActionError(caught));
      } finally {
        if (busyCorrelationActionRef.current === busyKey) {
          busyCorrelationActionRef.current = null;
          if (mounted.current) setBusyCorrelationAction(null);
        }
      }
    },
    [],
  );

  const prepareContextRow = useCallback(
    (target: HTMLElement) => {
      const rowKey = target.dataset.portRowKey;
      const row = ports.find((candidate) => portRowKey(candidate) === rowKey);
      if (!row || !rowKey) return;

      setSelectedRowKey(rowKey);
      setContextRow(row);
      setProcessPath(null);

      const request = ++processPathRequest.current;
      if (row.pid == null || row.source === "wsl" || row.source === "container") return;
      void getProcessInfo(row.pid)
        .then((info) => {
          if (mounted.current && processPathRequest.current === request) {
            setProcessPath({ rowKey, path: info.executable_path ?? info.exe });
          }
        })
        .catch(() => {
          if (mounted.current && processPathRequest.current === request) {
            setProcessPath({ rowKey, path: null });
          }
        });
    },
    [ports],
  );

  const contextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareContextRow(target),
  });

  const contextPath =
    contextRow && processPath?.rowKey === portRowKey(contextRow) ? processPath.path : null;
  const contextMenuItems = useMemo<readonly ContextMenuEntry[]>(() => {
    if (!contextRow) return [];
    const url = localhostUrl(contextRow);
    const hasPid = contextRow.pid != null;
    const request = listenerKillRequest(contextRow);
    const isContainer = contextRow.source === "container";
    const isBusy = busyRowKey === portRowKey(contextRow);
    const portFavorite = isPortFavorite(contextRow, preferences.favorite_ports);
    const processFavorite = isProcessFavorite(contextRow, preferences.favorite_processes);
    return [
      { type: "item", id: "copy-port", label: "포트 복사", disabled: contextRow.port <= 0 },
      { type: "item", id: "copy-pid", label: "PID 복사", disabled: !hasPid },
      { type: "item", id: "copy-localhost-url", label: "localhost URL 복사", disabled: !url },
      {
        type: "item",
        id: "open-localhost",
        label: "localhost 열기",
        disabled: !url || !isListening(contextRow),
      },
      { type: "separator", id: "process-separator" },
      {
        type: "item",
        id: "copy-process-path",
        label: "프로세스 경로 복사",
        disabled: !contextPath,
      },
      {
        type: "item",
        id: "reveal-process",
        label: "탐색기에서 보기",
        disabled: !hasPid || !contextPath || contextRow.source !== "windows",
      },
      { type: "separator", id: "favorite-separator" },
      {
        type: "item",
        id: "toggle-port-favorite",
        label: portFavorite ? "포트 즐겨찾기 해제" : "포트 즐겨찾기",
        disabled: preferencesSaving || contextRow.port <= 0,
      },
      {
        type: "item",
        id: "toggle-process-favorite",
        label: processFavorite ? "프로세스 즐겨찾기 해제" : "프로세스 즐겨찾기",
        disabled: !contextRow.identity || preferencesSaving,
      },
      { type: "separator", id: "danger-separator" },
      {
        type: "item",
        id: isContainer ? "handoff-container-stop" : "kill-listener",
        label: isContainer
          ? isBusy
            ? "전달 준비 중…"
            : "WSL Desktop에서 중지"
            : isBusy
            ? "종료 중…"
            : "리스너 종료",
        disabled: !snapshotHealthy || !request || !isListener(contextRow) || isBusy,
        danger: true,
      },
    ];
  }, [
    busyRowKey,
    contextPath,
    contextRow,
    preferences.favorite_ports,
    preferences.favorite_processes,
    preferencesSaving,
    snapshotHealthy,
  ]);

  const onContextMenuSelect = (id: string) => {
    const row = contextRow;
    if (!row) return;
    const url = localhostUrl(row);

    switch (id) {
      case "copy-port":
        if (row.port > 0) void runAction(() => navigator.clipboard.writeText(String(row.port)));
        break;
      case "copy-pid":
        if (row.pid != null) void runAction(() => navigator.clipboard.writeText(String(row.pid)));
        break;
      case "copy-localhost-url":
        if (url) void runAction(() => navigator.clipboard.writeText(url));
        break;
      case "open-localhost":
        void onOpen(row);
        break;
      case "copy-process-path":
        if (contextPath) void runAction(() => navigator.clipboard.writeText(contextPath));
        break;
      case "reveal-process":
        if (row.pid != null && contextPath) {
          const pid = row.pid;
          void runAction(() => revealProcess(pid));
        }
        break;
      case "toggle-port-favorite":
        togglePortFavorite(row);
        break;
      case "toggle-process-favorite":
        toggleProcessFavorite(row);
        break;
      case "kill-listener":
      case "handoff-container-stop":
        void onKill(row);
        break;
    }
  };

  const selectedRow = selectedRowKey
    ? ports.find((row) => portRowKey(row) === selectedRowKey) ?? null
    : null;

  return (
    <div className="app">
      <header className="toolbar">
        <h1 className="title">Port Manager</h1>
        <input
          className="search"
          aria-label="리스너 검색"
          placeholder="검색 (포트 / 프로토콜 / PID / 프로세스)..."
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
          onCompositionStart={() => setIsComposing(true)}
          onCompositionEnd={() => setIsComposing(false)}
          onKeyDown={(event) => {
            if (shouldIgnoreComposingShortcut(isComposing, event.key)) {
              event.stopPropagation();
            }
          }}
        />
        <div className="filters" aria-label="리스너 필터">
          {PROTO_FILTERS.map((filter) => (
            <button
              key={filter.value}
              type="button"
              className={"chip " + (protoFilter === filter.value ? "active" : "")}
              aria-pressed={protoFilter === filter.value}
              onClick={() => setProtoFilter(filter.value)}
            >
              {filter.label}
            </button>
          ))}
          <span className="divider" />
          {STATE_FILTERS.map((filter) => (
            <button
              key={filter.value}
              type="button"
              className={"chip " + (stateFilter === filter.value ? "active" : "")}
              aria-pressed={stateFilter === filter.value}
              onClick={() => setStateFilter(filter.value)}
            >
              {filter.label}
            </button>
          ))}
          <button
            type="button"
            className={"chip " + (preferences.pinned_only ? "active" : "")}
            aria-pressed={preferences.pinned_only}
            disabled={preferencesSaving}
            onClick={() =>
              void savePreferences({
                ...preferencesRef.current,
                pinned_only: !preferencesRef.current.pinned_only,
              })
            }
          >
            고정
          </button>
        </div>
        <label className="refresh-settings">
          <span>자동 새로 고침</span>
          <select
            aria-label="자동 새로 고침 간격"
            value={preferences.refresh_interval_ms}
            disabled={!preferencesReady || preferencesSaving}
            onChange={(event) => {
              const value = Number(event.currentTarget.value);
              if (
                Number.isInteger(value) &&
                value >= MIN_REFRESH_INTERVAL_MS &&
                value <= MAX_REFRESH_INTERVAL_MS
              ) {
                void savePreferences({ ...preferencesRef.current, refresh_interval_ms: value });
              }
            }}
          >
            {REFRESH_INTERVALS.map((interval) => (
              <option key={interval} value={interval}>
                {interval / 1_000}s
              </option>
            ))}
            {!REFRESH_INTERVALS.includes(preferences.refresh_interval_ms) && (
              <option value={preferences.refresh_interval_ms}>
                {preferences.refresh_interval_ms / 1_000}s
              </option>
            )}
          </select>
        </label>
        <button
          type="button"
          className="btn pause"
          aria-pressed={autoRefreshPaused}
          disabled={!preferencesReady}
          onClick={() => setAutoRefreshPaused((paused) => !paused)}
        >
          {autoRefreshPaused ? "재개" : "일시 중지"}
        </button>
        <button
          type="button"
          className="btn refresh"
          onClick={() => void refresh()}
          disabled={loading || !preferencesReady}
        >
          {loading ? "새로 고치는 중..." : "새로 고침"}
        </button>
      </header>

      {settingsWarning && (
        <div className="warning" role="status" aria-live="polite">
          {settingsWarning}
        </div>
      )}
      {error && (
        <div className="error" role="alert" aria-live="assertive">
          {error}
        </div>
      )}
      {handoff && (
        <div className="handoff" role="status" aria-live="polite">
          {handoff}
        </div>
      )}

      {sources.length > 0 && (
        <section className="source-diagnostics" aria-label="연결 출처 상태">
          <div className="source-heading">
            <strong>연결 출처</strong>
            <span>
              {unhealthySources.length === 0
                ? "모두 사용 가능"
                : `${unhealthySources.length}개 사용 불가`}
            </span>
          </div>
          <ul>
            {sources.map((source) => (
              <li key={source.producer} aria-label={sourceStatusLabel(source)}>
                <span className="mono">{source.producer}</span>
                <span className={`source-state source-${source.state}`}>{source.state}</span>
                <span className="dim">
                  {source.freshness_ms == null ? "최신 상태 알 수 없음" : `${source.freshness_ms}ms 전`}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {correlationsTruncated && (
        <div className="warn" role="status">
          연결 결과가 안전한 표시 한도에 도달했습니다. 중복된 예상 포트 또는 서비스 선언을 확인하세요.
        </div>
      )}

      <div className="statusbar" aria-live="polite">
        <span>
          {visible.length} / {counts.total}행
        </span>
        <span className="dot-green" /> 리스너 {counts.listening}개
        <span className={snapshotHealthy ? "snapshot-ok" : "snapshot-stale"}>
          {snapshotHealthy ? "스냅샷 안정" : "스냅샷 사용 불가 · 작업 잠김"}
        </span>
        <span>{autoRefreshPaused ? "자동 새로 고침 일시 중지됨" : "자동 새로 고침 켜짐"}</span>
      </div>

      {hasComparedSnapshot && (
        <section className="diff-panel" aria-label="새로 고침 타임라인">
          <div className="diff-heading">
            <strong>새로 고침 타임라인</strong>
            <span>{timeline.length === 0 ? "변경 없음" : `${timeline.length}개 이벤트`}</span>
          </div>
          {timeline.length > 0 && (
            <ol>
              {[...timeline].reverse().map((change, index) => {
                const row = change.after ?? change.before;
                const ownerChange =
                  change.kind === "owner-changed"
                    ? `${ownerLabels(change.before)} → ${ownerLabels(change.after)}`
                    : null;
                return (
                  <li key={change.key + ":" + index}>
                    <time dateTime={new Date(change.observed_at_ms).toISOString()}>
                      {timelineTime(change.observed_at_ms)}
                    </time>
                    <span className={"diff-kind diff-" + change.kind}>{change.kind}</span>
                    <span className="mono">{row?.local_addr ?? "-"}</span>
                    {ownerChange ? (
                      <span>{ownerChange}</span>
                    ) : (
                      <span>{row?.process_name ?? "알 수 없는 프로세스"}</span>
                    )}
                  </li>
                );
              })}
            </ol>
          )}
        </section>
      )}

      <div className="table-wrap">
        <table aria-label="리스너 목록">
          <thead>
            <tr>
              <th>출처</th>
              <th>프로토콜</th>
              <th>포트</th>
              <th>로컬 주소</th>
              <th>상태</th>
              <th>PID</th>
              <th>프로세스</th>
              <th>소유자</th>
              <th>작업</th>
            </tr>
          </thead>
          <tbody>
            {visible.map((row) => {
              const rowKey = portRowKey(row);
              const selected = selectedRowKey === rowKey;
              const busy = busyRowKey === rowKey;
              return (
                <tr
                  key={rowKey}
                  data-port-row-key={rowKey}
                  tabIndex={0}
                  aria-selected={selected}
                  aria-label={provenanceLabel(row) + " " + row.local_addr}
                  className={selected ? "selected" : undefined}
                  onContextMenu={contextMenu.triggerProps.onContextMenu}
                  onClick={() => setSelectedRowKey(rowKey)}
                  onKeyDown={(event) => {
                    contextMenu.triggerProps.onKeyDown?.(event);
                    if (event.defaultPrevented) return;
                    // Action buttons inside the row own their keyboard
                    // activation. Preventing their Enter/Space default here
                    // would make favorite/kill/open inaccessible by keyboard.
                    if (event.target !== event.currentTarget) return;
                    if (shouldIgnoreComposingShortcut(isComposing, event.key)) return;
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      setSelectedRowKey(rowKey);
                    }
                  }}
                >
                  <td>{provenanceLabel(row)}</td>
                  <td>{row.proto}</td>
                  <td className="mono">{row.port || "-"}</td>
                  <td className="mono dim">{row.local_addr}</td>
                  <td>{row.state || "-"}</td>
                  <td className="mono">{row.pid ?? "-"}</td>
                  <td>{row.process_name ?? "-"}</td>
                  <td>
                    {row.correlations && row.correlations.length > 0 ? (
                      <div className="correlation-cell" aria-label="리스너 연결">
                        {row.correlations.map((correlation) => (
                          <span
                            key={correlation.action_key}
                            className={`correlation-badge confidence-${correlation.confidence}`}
                            title={correlationSummary(correlation)}
                          >
                            {correlation.label} · {correlation.confidence}
                          </span>
                        ))}
                      </div>
                    ) : (
                      <span className="dim">-</span>
                    )}
                  </td>
                  <td className="actions">
                    <button
                      type="button"
                      className="btn favorite"
                      aria-label={
                        isPortFavorite(row, preferences.favorite_ports)
                          ? "포트 즐겨찾기 해제"
                          : "포트 즐겨찾기"
                      }
                      aria-pressed={isPortFavorite(row, preferences.favorite_ports)}
                      disabled={preferencesSaving || row.port <= 0}
                      onClick={() => togglePortFavorite(row)}
                    >
                      {isPortFavorite(row, preferences.favorite_ports) ? "★" : "☆"}
                    </button>
                    {row.identity && (
                      <button
                        type="button"
                        className="btn favorite"
                        aria-label={
                          isProcessFavorite(row, preferences.favorite_processes)
                            ? "프로세스 즐겨찾기 해제"
                            : "프로세스 즐겨찾기"
                        }
                        aria-pressed={isProcessFavorite(row, preferences.favorite_processes)}
                        disabled={preferencesSaving}
                        onClick={() => toggleProcessFavorite(row)}
                      >
                        {isProcessFavorite(row, preferences.favorite_processes) ? "●" : "○"}
                      </button>
                    )}
                    {row.identity && isListener(row) && (
                      <button
                        type="button"
                        className="btn danger"
                        aria-label={
                          row.source === "container" ? "WSL Desktop에서 중지" : "리스너 종료"
                        }
                        disabled={busy || !snapshotHealthy}
                        onClick={() => void onKill(row)}
                      >
                        {busy
                          ? row.source === "container"
                            ? "준비 중..."
                            : "종료 중..."
                          : row.source === "container"
                            ? "중지"
                            : "종료"}
                      </button>
                    )}
                    {row.port > 0 && isListening(row) && (
                      <button type="button" className="btn" onClick={() => void onOpen(row)}>
                        열기
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
            {visible.length === 0 && (
              <tr>
                <td colSpan={9} className="empty">
                  결과 없음
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {selectedRow && (
        <aside className="details" aria-label="리스너 세부 정보">
          <div className="details-heading">
            <h2>리스너 세부 정보</h2>
            <span>{provenanceLabel(selectedRow)}</span>
          </div>
          <dl>
            <div>
              <dt>출처</dt>
              <dd>{provenanceLabel(selectedRow)}</dd>
            </div>
            <div>
              <dt>엔드포인트</dt>
              <dd className="mono">
                {selectedRow.proto} {selectedRow.local_addr}
              </dd>
            </div>
            <div>
              <dt>PID</dt>
              <dd className="mono">{displayValue(selectedRow.pid)}</dd>
            </div>
            <div>
              <dt>프로세스</dt>
              <dd>{displayValue(selectedRow.process_name)}</dd>
            </div>
            <div>
              <dt>명령줄</dt>
              <dd className="mono details-value">
                {displayValue(selectedRow.command_line)}
              </dd>
            </div>
            <div>
              <dt>실행 파일 경로</dt>
              <dd className="mono details-value">
                {displayValue(selectedRow.executable_path)}
              </dd>
            </div>
            <div>
              <dt>프로세스 시작 시간</dt>
              <dd className="mono">
                {displayValue(selectedRow.process_start_time)}
              </dd>
            </div>
            {selectedRow.wsl_distro && (
              <div>
                <dt>WSL 배포판</dt>
                <dd>{selectedRow.wsl_distro}</dd>
              </div>
            )}
            {selectedRow.wsl_start_tick != null && (
              <div>
                <dt>WSL 시작 틱</dt>
                <dd className="mono">{selectedRow.wsl_start_tick}</dd>
              </div>
            )}
            {selectedRow.container_id && (
              <div>
                <dt>컨테이너</dt>
                <dd className="mono">
                  {displayValue(selectedRow.container_engine)} / {selectedRow.container_id}
                </dd>
              </div>
            )}
          </dl>
          <section className="correlation-details" aria-label="리스너 연결">
            <div className="correlation-details-heading">
              <h3>연결</h3>
              <span>{selectedRow.correlations?.length ?? 0}</span>
            </div>
            {(selectedRow.correlations?.length ?? 0) > 0 ? (
              <ul>
                {selectedRow.correlations?.map((correlation) => {
                  const ownerBusyKey = `${correlation.action_key}:owner`;
                  const stdoutBusyKey = `${correlation.action_key}:stdout`;
                  const stderrBusyKey = `${correlation.action_key}:stderr`;
                  return (
                    <li key={correlation.action_key} className="correlation-detail">
                      <div className="correlation-detail-heading">
                        <span
                          className={`correlation-badge confidence-${correlation.confidence}`}
                        >
                          {correlation.confidence}
                        </span>
                        <strong>{correlation.label}</strong>
                      </div>
                      <div className="correlation-meta">
                        {correlationSummary(correlation)}
                      </div>
                      <div className="correlation-actions">
                        <button
                          type="button"
                          className="btn"
                          aria-label={`${correlation.label} 소유자 열기`}
                          disabled={busyCorrelationAction !== null || !correlation.action_key}
                          onClick={() => void onOpenCorrelation(correlation)}
                        >
                          {busyCorrelationAction === ownerBusyKey ? "여는 중…" : "소유자 열기"}
                        </button>
                        {correlation.logs_available && (
                          <>
                            <button
                              type="button"
                              className="btn"
                              aria-label={`Log Lens에서 ${correlation.label} stdout 열기`}
                              disabled={busyCorrelationAction !== null || !correlation.action_key}
                              onClick={() => void onOpenCorrelation(correlation, "stdout")}
                            >
                              {busyCorrelationAction === stdoutBusyKey
                                ? "여는 중…"
                                : "Log Lens에서 stdout 열기"}
                            </button>
                            <button
                              type="button"
                              className="btn"
                              aria-label={`Log Lens에서 ${correlation.label} stderr 열기`}
                              disabled={busyCorrelationAction !== null || !correlation.action_key}
                              onClick={() => void onOpenCorrelation(correlation, "stderr")}
                            >
                              {busyCorrelationAction === stderrBusyKey
                                ? "여는 중…"
                                : "Log Lens에서 stderr 열기"}
                            </button>
                          </>
                        )}
                      </div>
                    </li>
                  );
                })}
              </ul>
            ) : (
              <p className="dim">연결 없음</p>
            )}
          </section>
          <div className="details-actions">
            <button
              type="button"
              className="btn favorite"
              aria-label={
                isPortFavorite(selectedRow, preferences.favorite_ports)
                  ? "포트 즐겨찾기 해제"
                  : "포트 즐겨찾기"
              }
              aria-pressed={isPortFavorite(selectedRow, preferences.favorite_ports)}
              disabled={preferencesSaving || selectedRow.port <= 0}
              onClick={() => togglePortFavorite(selectedRow)}
            >
              {isPortFavorite(selectedRow, preferences.favorite_ports)
                ? "포트 즐겨찾기 해제"
                : "포트 즐겨찾기"}
            </button>
            {selectedRow.identity && (
              <button
                type="button"
                className="btn favorite"
                aria-label={
                  isProcessFavorite(selectedRow, preferences.favorite_processes)
                  ? "프로세스 즐겨찾기 해제"
                  : "프로세스 즐겨찾기"
                }
                aria-pressed={isProcessFavorite(selectedRow, preferences.favorite_processes)}
                disabled={preferencesSaving}
                onClick={() => toggleProcessFavorite(selectedRow)}
              >
                {isProcessFavorite(selectedRow, preferences.favorite_processes)
                  ? "프로세스 즐겨찾기 해제"
                  : "프로세스 즐겨찾기"}
              </button>
            )}
          </div>
          {selectedRow.source === "container" && listenerKillRequest(selectedRow) && (
            <button
              type="button"
              className="btn danger"
              disabled={busyRowKey === portRowKey(selectedRow) || !snapshotHealthy}
              onClick={() => void onKill(selectedRow)}
            >
              WSL Desktop에서 중지
            </button>
          )}
        </aside>
      )}

      <ContextMenu
        open={contextMenu.open}
        anchor={contextMenu.anchor}
        restoreFocusTo={contextMenu.restoreFocusTo}
        items={contextMenuItems}
        onSelect={onContextMenuSelect}
        onClose={contextMenu.close}
        ariaLabel="포트 작업"
      />
    </div>
  );
}
