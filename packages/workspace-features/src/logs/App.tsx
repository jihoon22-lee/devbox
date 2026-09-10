import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from "react";
import {
  acceptLogSource,
  cancelRead,
  classifyHandoffError,
  discardLogSource,
  exportRecords,
  handoffErrorCode,
  listSavedViews,
  onOpenRequest,
  previewLogSource,
  readSources,
  removeSavedView,
  renewLogSource,
  saveSavedView,
  sendSelectionToToolbox,
  takePendingOpen,
} from "./api";
import { browserSnapshot } from "./browserFixture";
import {
  createLiteralRegex,
  createSafeRegex,
  filterRecords,
  recordKey,
  truncateUtf8,
  utf8ByteLength,
} from "./filter";
import { isTauri } from "./lib/isTauri";
import type {
  ContainerEngine,
  FileCursor,
  FilterSpec,
  HandoffOpenTarget,
  LogLevel,
  LogRecord,
  LogSourcePreview,
  SourceKind,
  SourceSpec,
  SavedView,
  SourcesSnapshot,
} from "./types";
import "./App.css";

const MAX_RENDERED_ROWS = 2_000;
const MAX_SELECTED_RECORDS = MAX_RENDERED_ROWS;
const MAX_SOURCES = 16;
const MAX_SAVED_VIEWS = 20;
const MAX_HIGHLIGHTS = 256;
const MAX_HANDOFF_RECOVERY_ATTEMPTS = 3;
const MAX_TOOLBOX_TEXT_BYTES = 512 * 1024;
const MAX_TOOLBOX_TEXT_CHARS = 256_000;

const EXPORT_TRUNCATED_ERROR = "내보내기 안전 제한에 도달해 일부 내용만 처리했습니다.";
const STALE_SELECTION_ERROR = "선택한 로그가 최신 상태가 아닙니다. 새로고침한 뒤 다시 선택하세요.";
const SELECTION_LIMIT_ERROR = "로그는 최대 2,000개까지 선택할 수 있습니다.";
const TOOLBOX_EXPORT_ERROR = "선택한 로그가 Developer Toolbox 전송 안전 제한을 초과했습니다.";
const TOOLBOX_SEND_ERROR = "선택한 로그를 Developer Toolbox로 보내지 못했습니다. 클립보드로 자동 전환하지 않았습니다.";
const TOOLBOX_SEND_SUCCESS = "선택한 로그를 Developer Toolbox로 보냈습니다.";
const TOOLBOX_SEND_REDACTED = "민감정보를 마스킹한 뒤 선택한 로그를 Developer Toolbox로 보냈습니다.";

type HandoffRecoveryAction = "preview" | "accept" | "discard" | "renew";

interface HandoffRecovery {
  id: string;
  kind: HandoffOpenTarget["handoffKind"];
  action: HandoffRecoveryAction;
  attempts: number;
}

const SAVED_VIEW_ERRORS = new Set([
  "저장된 뷰 저장소를 읽을 수 없습니다",
  "저장된 뷰 저장소를 저장할 수 없습니다",
  "저장된 뷰 설정이 유효하지 않습니다",
  "저장된 뷰가 다른 작업에서 변경되었습니다. 다시 불러온 뒤 시도해 주세요",
  "저장된 뷰가 최대 개수에 도달했습니다. 기존 뷰를 삭제한 뒤 다시 시도해 주세요",
  "저장된 뷰를 찾을 수 없습니다",
  "저장된 뷰 응답이 유효하지 않습니다",
]);

function savedViewFailureMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  return SAVED_VIEW_ERRORS.has(message) ? message : "저장된 뷰 작업을 완료하지 못했습니다";
}

function handoffFailureMessage(error: unknown, fallback: string): string {
  switch (handoffErrorCode(error)) {
    case "handoff-missing":
    case "handoff-expired":
    case "handoff-lease-expired":
      return "Log Lens source handoff가 만료되었거나 이미 처리되었습니다. 다시 보내 주세요.";
    case "handoff-busy":
      return "다른 Log Lens source handoff를 처리 중입니다.";
    default:
      return fallback;
  }
}

function handoffRetryMessage(action: HandoffRecoveryAction, attempts: number): string {
  if (attempts >= MAX_HANDOFF_RECOVERY_ATTEMPTS) {
    return "Log Lens source handoff 복구 재시도 한도에 도달했습니다. 저장소 상태를 확인하고 Log Lens를 재시작한 뒤 새 handoff를 보내 주세요.";
  }
  const operation = action === "discard"
    ? "취소"
    : action === "accept"
      ? "추가"
      : action === "renew"
        ? "lease 갱신"
        : "미리보기";
  return `Log Lens source handoff ${operation} 저장소 작업을 완료하지 못했습니다. 재시도할 수 있습니다 (${attempts}/${MAX_HANDOFF_RECOVERY_ATTEMPTS}).`;
}

function makeOperationId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) return crypto.randomUUID();
  return `op-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}

function formatTimestamp(timestamp: number | null): string {
  const iso = toIsoTimestamp(timestamp);
  return iso ?? "—";
}

function toIsoTimestamp(timestamp: number | null): string | undefined {
  if (timestamp === null || !Number.isSafeInteger(timestamp)) return undefined;
  const date = new Date(timestamp);
  return Number.isNaN(date.getTime()) ? undefined : date.toISOString();
}

function labelForKind(kind: SourceKind): string {
  switch (kind) {
    case "localFile": return "로컬 파일";
    case "directory": return "로컬 디렉터리";
    case "wslFile": return "WSL 파일";
    case "wslJournal": return "WSL 저널";
    case "run": return "Run Manager 연결";
    case "webhookCapture": return "Webhook 캡처";
    case "container": return "컨테이너 로그";
  }
}

function labelForStatus(status: string): string {
  switch (status) {
    case "initial": return "최초 읽기";
    case "advanced": return "추가 읽기";
    case "rotated": return "순환 감지";
    case "truncated": return "제한 도달";
    case "unavailable": return "사용 불가";
    default: return "대기";
  }
}

function compareOpaqueIds(left: string, right: string): number {
  // Native merge uses Rust's bytewise BTreeMap ordering. `localeCompare`
  // varies by machine locale/options, which could otherwise reorder equal
  // timestamps differently between browser fixtures and the packaged app.
  if (left === right) return 0;
  return left < right ? -1 : 1;
}

function mergeClientRecords(
  existing: LogRecord[],
  incoming: LogRecord[],
  sourceSummaries: SourcesSnapshot["sources"],
  cursors: SourcesSnapshot["cursors"],
  statuses: SourcesSnapshot["statuses"],
): LogRecord[] {
  const byKey = new Map(existing.map((record) => [recordKey(record), record]));
  sourceSummaries.forEach((source, index) => {
    if (!cursors[index] || statuses[index] === "rotated" || statuses[index] === "truncated") {
      for (const [key, record] of byKey) {
        if (record.sourceId === source.sourceId) byKey.delete(key);
      }
    }
  });
  for (const record of incoming) byKey.set(recordKey(record), record);
  return [...byKey.values()]
    .sort((left, right) => compareSafeNumbers(left.timestampMillis, right.timestampMillis)
      || compareOpaqueIds(left.sourceId, right.sourceId)
      || left.sequence - right.sequence)
    .slice(-100_000);
}

function compareSafeNumbers(left: number | null, right: number | null): number {
  const leftValue = left ?? Number.MAX_SAFE_INTEGER;
  const rightValue = right ?? Number.MAX_SAFE_INTEGER;
  if (leftValue === rightValue) return 0;
  return leftValue < rightValue ? -1 : 1;
}

function highlightMessage(message: string, filter: FilterSpec): ReactNode {
  if (!filter.text) return message;
  const matcher = filter.regex
    ? createSafeRegex(filter.text, "g")
    : createLiteralRegex(filter.text, "g");
  if (!matcher) return message;
  const parts: ReactNode[] = [];
  let cursor = 0;
  let matchCount = 0;
  for (const match of message.matchAll(matcher)) {
    if (matchCount >= MAX_HIGHLIGHTS) break;
    const index = match.index ?? 0;
    if (index > cursor) parts.push(message.slice(cursor, index));
    parts.push(<mark key={`${index}-${matchCount}`}>{match[0]}</mark>);
    cursor = index + match[0].length;
    matchCount += 1;
  }
  if (cursor === 0) return message;
  if (cursor < message.length) parts.push(message.slice(cursor));
  return parts;
}

function App() {
  const [sources, setSources] = useState<SourceSpec[]>(() => isTauri() ? [] : [{ kind: "localFile", path: "fixture.log" }]);
  const [cursors, setCursors] = useState<Array<FileCursor | null>>(() => isTauri() ? [] : [null]);
  const [records, setRecords] = useState<LogRecord[]>(() => isTauri() ? [] : browserSnapshot().records);
  const [snapshot, setSnapshot] = useState<SourcesSnapshot | null>(() => isTauri() ? null : browserSnapshot());
  const [filter, setFilter] = useState<FilterSpec>({ text: "", regex: false });
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [selectedGeneration, setSelectedGeneration] = useState<number | null>(null);
  const [bookmarks, setBookmarks] = useState<Set<string>>(new Set());
  const [savedViews, setSavedViews] = useState<SavedView[]>([]);
  const [savedViewsRevision, setSavedViewsRevision] = useState(0);
  const [savedViewsBusy, setSavedViewsBusy] = useState(false);
  const [viewName, setViewName] = useState("");
  const [selectedViewName, setSelectedViewName] = useState("");
  const [kind, setKind] = useState<SourceKind>("localFile");
  const [path, setPath] = useState("");
  const [pattern, setPattern] = useState("*.log");
  const [distro, setDistro] = useState("Ubuntu");
  const [unit, setUnit] = useState("");
  const [sourceId, setSourceId] = useState("");
  const [engine, setEngine] = useState<ContainerEngine>("docker");
  const [containerId, setContainerId] = useState("");
  const [follow, setFollow] = useState(false);
  const [connected, setConnected] = useState(true);
  const [paused, setPaused] = useState(false);
  const [wrapLines, setWrapLines] = useState(true);
  const [busy, setBusy] = useState(false);
  const [toolboxBusy, setToolboxBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [handoffPreview, setHandoffPreview] = useState<LogSourcePreview | null>(null);
  const [handoffBusy, setHandoffBusy] = useState(false);
  const [handoffRecovery, setHandoffRecovery] = useState<HandoffRecovery | null>(null);
  const [contextRecord, setContextRecord] = useState<LogRecord | null>(null);
  const generation = useRef(0);
  const operation = useRef<string | null>(null);
  const mounted = useRef(true);
  const connectedRef = useRef(true);
  const refreshInFlight = useRef(false);
  const refreshPending = useRef<{
    sources: SourceSpec[];
    cursors: Array<FileCursor | null>;
  } | null>(null);
  const refreshRef = useRef<(
    nextSources?: SourceSpec[],
    nextCursors?: Array<FileCursor | null>,
  ) => Promise<void>>(async () => undefined);
  const pausedRef = useRef(paused);
  const compositionRef = useRef(false);
  const selectedRef = useRef(selected);
  const selectedGenerationRef = useRef<number | null>(selectedGeneration);
  const selectionRevisionRef = useRef(0);
  const snapshotGenerationRef = useRef<number | null>(snapshot?.generation ?? null);
  const toolboxBusyRef = useRef(false);
  const contextRecordRef = useRef<LogRecord | null>(null);
  const handoffPreviewRef = useRef<LogSourcePreview | null>(null);
  const handoffBusyRef = useRef(false);
  const handoffRecoveryRef = useRef<HandoffRecovery | null>(null);
  const handoffGeneration = useRef(0);
  // Native single-instance delivery is at-least-once from the UI's point of
  // view. Keep one bounded latest-id slot while the current preview/action is
  // busy so a second producer handoff is not silently dropped.
  const queuedHandoffRef = useRef<HandoffOpenTarget | null>(null);
  const handoffOpenerRef = useRef<HTMLElement | null>(null);
  const handoffCancelRef = useRef<HTMLButtonElement | null>(null);
  const handoffAcceptRef = useRef<HTMLButtonElement | null>(null);
  const handoffRetryRef = useRef<HTMLButtonElement | null>(null);
  pausedRef.current = paused;
  selectedRef.current = selected;
  selectedGenerationRef.current = selectedGeneration;
  snapshotGenerationRef.current = snapshot?.generation ?? null;
  connectedRef.current = connected;

  const prepareLogContext = useCallback((_reason: "pointer" | "keyboard", target: HTMLElement) => {
    const element = target as HTMLElement | null;
    const key = element?.dataset?.logKey
      ?? element?.closest?.<HTMLElement>("[data-log-key]")?.dataset.logKey;
    const record = key
      ? records.find((candidate) => recordKey(candidate) === key) ?? null
      : contextRecordRef.current;
    contextRecordRef.current = record;
    setContextRecord(record);
  }, [records]);
  const logContextMenu = useContextMenu({ onBeforeOpen: prepareLogContext });
  const logContextTrigger = logContextMenu.triggerProps;
  const closeLogContextMenu = useCallback(() => {
    contextRecordRef.current = null;
    setContextRecord(null);
    logContextMenu.close();
  }, [logContextMenu.close]);

  const invalidateSelection = useCallback(() => {
    if (selectedRef.current.size === 0 && selectedGenerationRef.current === null) return;
    selectionRevisionRef.current += 1;
    selectedRef.current = new Set();
    selectedGenerationRef.current = null;
    setSelected(new Set());
    setSelectedGeneration(null);
    setError(STALE_SELECTION_ERROR);
    setNotice(null);
  }, []);

  const refresh = useCallback(async (
    nextSources = sources,
    nextCursors = cursors,
  ) => {
    if (!mounted.current || !connectedRef.current || nextSources.length === 0) {
      refreshPending.current = null;
      return;
    }
    if (refreshInFlight.current) {
      // Reads are intentionally single-flight, but a source add/remove/view
      // load must not disappear behind the current request. Keep only the
      // latest bounded descriptor set and cancel the superseded native read;
      // its finally block drains this slot after releasing the flight lock.
      refreshPending.current = {
        sources: [...nextSources],
        cursors: [...nextCursors],
      };
      generation.current += 1;
      const active = operation.current;
      operation.current = null;
      if (active) void cancelRead(active);
      return;
    }
    refreshInFlight.current = true;
    const currentGeneration = generation.current + 1;
    generation.current = currentGeneration;
    const operationId = makeOperationId();
    operation.current = operationId;
    setBusy(true);
    setError(null);
    setNotice(null);
    invalidateSelection();
    try {
      const sequenceStarts = nextSources.map((_, index) => {
        if (!nextCursors[index]) return 0;
        const previousSourceId = snapshot?.sources[index]?.sourceId;
        return records
          .filter((record) => record.sourceId === previousSourceId)
          .reduce((maximum, record) => Math.max(maximum, record.sequence + 1), 0);
      });
      const next = await readSources(nextSources, nextCursors, sequenceStarts, currentGeneration, operationId);
      if (!mounted.current || generation.current !== currentGeneration || pausedRef.current) return;
      setSnapshot(next);
      setRecords(nextSources === sources
        ? (current) => mergeClientRecords(current, next.records, next.sources, next.cursors, next.statuses)
        : next.records);
      setCursors(next.cursors);
    } catch {
      if (mounted.current && generation.current === currentGeneration) {
        setError("로그 source를 읽지 못했습니다. source를 확인한 뒤 다시 시도하세요.");
      }
    } finally {
      const pending = refreshPending.current;
      refreshPending.current = null;
      refreshInFlight.current = false;
      if (mounted.current && pending) {
        void refreshRef.current(pending.sources, pending.cursors);
      } else if (mounted.current && generation.current === currentGeneration) {
        setBusy(false);
        operation.current = null;
      } else if (mounted.current) {
        // A superseding empty-source change invalidates this operation without
        // starting a replacement read. Do not leave the toolbar permanently
        // busy in that stale-generation case.
        setBusy(false);
        operation.current = null;
      }
    }
  }, [cursors, invalidateSelection, paused, records, snapshot, sources]);
  refreshRef.current = refresh;

  const updateHandoffRecovery = useCallback((next: HandoffRecovery | null) => {
    handoffRecoveryRef.current = next;
    setHandoffRecovery(next);
  }, []);

  const clearHandoffPreview = useCallback(() => {
    handoffPreviewRef.current = null;
    setHandoffPreview(null);
  }, []);

  const clearHandoffState = useCallback(() => {
    clearHandoffPreview();
    updateHandoffRecovery(null);
  }, [clearHandoffPreview, updateHandoffRecovery]);

  const startLogSourcePreview = useCallback(async (
    handoffKind: HandoffOpenTarget["handoffKind"],
    id: string,
    attempts = 0,
  ) => {
    const actionGeneration = ++handoffGeneration.current;
    const activeElement = document.activeElement;
    handoffOpenerRef.current = activeElement instanceof HTMLElement ? activeElement : null;
    handoffBusyRef.current = true;
    setHandoffBusy(true);
    setError(null);
    setNotice(null);
    try {
      const preview = await previewLogSource(handoffKind, id);
      if (!mounted.current || handoffGeneration.current !== actionGeneration) {
        void discardLogSource(preview.id).catch(() => undefined);
        return;
      }
      handoffPreviewRef.current = preview;
      setHandoffPreview(preview);
      updateHandoffRecovery(null);
    } catch (error: unknown) {
      if (!mounted.current || handoffGeneration.current !== actionGeneration) {
        // The native command may have claimed before reporting a retryable
        // storage/restore failure. Best-effort release the same opaque id;
        // the native id check makes this harmless if no claim was acquired.
        void discardLogSource(id).catch(() => undefined);
        return;
      }
      const code = handoffErrorCode(error);
      if (classifyHandoffError(error) === "retryable") {
        // A malformed successful preview or a native restore failure means
        // the native slot still owns this exact claim; retry restoration
        // rather than attempting a second claim for the same id.
        const action: HandoffRecoveryAction = code === "handoff-response-invalid"
          || code === "handoff-restore-failed"
          ? "discard"
          : "preview";
        const nextAttempts = Math.min(attempts + 1, MAX_HANDOFF_RECOVERY_ATTEMPTS);
        updateHandoffRecovery({ id, kind: handoffKind, action, attempts: nextAttempts });
        setError(null);
      } else {
        clearHandoffState();
        setError(handoffFailureMessage(
          error,
          "Log Lens source handoff를 미리볼 수 없습니다. 다시 보내 주세요.",
        ));
      }
    } finally {
      if (handoffGeneration.current === actionGeneration) {
        handoffBusyRef.current = false;
        if (mounted.current) setHandoffBusy(false);
      }
    }
  }, [clearHandoffState, updateHandoffRecovery]);

  const openLogSourcePreview = useCallback((target: HandoffOpenTarget) => {
    if (!/^[0-9a-f]{32}$/.test(target.id)
      || !["log-source/v1", "webhook-log/v1"].includes(target.handoffKind)) return;
    if (handoffBusyRef.current || handoffPreviewRef.current || handoffRecoveryRef.current) {
      queuedHandoffRef.current = target;
      if (mounted.current) setNotice("다른 Log Lens handoff를 처리한 뒤 최신 요청을 미리봅니다.");
      return;
    }
    void startLogSourcePreview(target.handoffKind, target.id);
  }, [startLogSourcePreview]);

  const restoreHandoffClaim = useCallback(async (
    kind: HandoffOpenTarget["handoffKind"],
    id: string,
    attempts = 0,
  ) => {
    const actionGeneration = ++handoffGeneration.current;
    handoffBusyRef.current = true;
    setHandoffBusy(true);
    updateHandoffRecovery(null);
    try {
      await discardLogSource(id);
      if (!mounted.current || handoffGeneration.current !== actionGeneration) return;
      clearHandoffState();
      setError(null);
      setNotice("Log Lens source handoff를 취소했습니다. 다시 열 수 있습니다.");
    } catch (error: unknown) {
      if (!mounted.current || handoffGeneration.current !== actionGeneration) return;
      if (classifyHandoffError(error) === "retryable") {
        const nextAttempts = Math.min(attempts + 1, MAX_HANDOFF_RECOVERY_ATTEMPTS);
        updateHandoffRecovery({ id, kind, action: "discard", attempts: nextAttempts });
        setError(null);
      } else {
        clearHandoffState();
        setError(handoffFailureMessage(error, "Log Lens source handoff를 취소하지 못했습니다."));
      }
    } finally {
      if (handoffGeneration.current === actionGeneration) {
        handoffBusyRef.current = false;
        if (mounted.current) setHandoffBusy(false);
      }
    }
  }, [clearHandoffState, updateHandoffRecovery]);

  const cancelLogSourcePreview = useCallback(() => {
    const preview = handoffPreviewRef.current;
    if (!preview || handoffBusyRef.current) return;
    const recovery = handoffRecoveryRef.current;
    if (recovery && recovery.attempts >= MAX_HANDOFF_RECOVERY_ATTEMPTS) return;
    const attempts = recovery?.attempts ?? 0;
    void restoreHandoffClaim(preview.kind, preview.id, attempts);
  }, [restoreHandoffClaim]);

  const applyAcceptedSource = useCallback(async (
    kind: HandoffOpenTarget["handoffKind"],
    id: string,
    attempts = 0,
  ) => {
    if (sources.length >= MAX_SOURCES) {
      setError(`source는 한 번에 최대 ${MAX_SOURCES}개까지 불러올 수 있습니다.`);
      return;
    }
    const actionGeneration = ++handoffGeneration.current;
    handoffBusyRef.current = true;
    setHandoffBusy(true);
    updateHandoffRecovery(null);
    setError(null);
    try {
      const source = await acceptLogSource(id);
      if (!mounted.current || handoffGeneration.current !== actionGeneration) return;
      clearHandoffState();
      if (sources.some((candidate) => JSON.stringify(candidate) === JSON.stringify(source))) {
        setNotice("이 source는 이미 선택되어 있습니다. handoff는 소비되었습니다.");
        return;
      }
      const nextSources = [...sources, source];
      const nextCursors = nextSources.map(() => null);
      connectedRef.current = true;
      setConnected(true);
      setSources(nextSources);
      setCursors(nextCursors);
      setNotice("Log Lens source를 추가했습니다. 읽기 전용 adapter로 불러옵니다.");
      void refresh(nextSources, nextCursors);
    } catch (error: unknown) {
      if (!mounted.current || handoffGeneration.current !== actionGeneration) return;
      // Native ack failures retain the claim for this exact id. Keep the
      // modal and expose a bounded retry instead of clearing its busy state
      // into an unowned/irrecoverable envelope.
      if (classifyHandoffError(error) === "retryable"
        && handoffErrorCode(error) !== "handoff-response-invalid") {
        const nextAttempts = Math.min(attempts + 1, MAX_HANDOFF_RECOVERY_ATTEMPTS);
        updateHandoffRecovery({ id, kind, action: "accept", attempts: nextAttempts });
        setError(null);
      } else {
        clearHandoffState();
        setError(handoffFailureMessage(
          error,
          "Log Lens source handoff를 적용하지 못했습니다. 다시 보내 주세요.",
        ));
      }
    } finally {
      if (handoffGeneration.current === actionGeneration) {
        handoffBusyRef.current = false;
        if (mounted.current) setHandoffBusy(false);
      }
    }
  }, [clearHandoffState, refresh, sources, updateHandoffRecovery]);

  const acceptLogSourcePreview = useCallback(() => {
    const preview = handoffPreviewRef.current;
    if (!preview || handoffBusyRef.current) return;
    const attempts = handoffRecoveryRef.current?.action === "accept"
      ? handoffRecoveryRef.current.attempts
      : 0;
    void applyAcceptedSource(preview.kind, preview.id, attempts);
  }, [applyAcceptedSource]);

  const renewPreviewLease = useCallback(async (
    kind: HandoffOpenTarget["handoffKind"],
    id: string,
    attempts = 0,
  ) => {
    const actionGeneration = ++handoffGeneration.current;
    handoffBusyRef.current = true;
    setHandoffBusy(true);
    updateHandoffRecovery(null);
    try {
      const leaseUntilMs = await renewLogSource(id);
      if (!mounted.current || handoffGeneration.current !== actionGeneration) return;
      setHandoffPreview((current) => current?.id === id ? { ...current, leaseUntilMs } : current);
      setError(null);
    } catch (error: unknown) {
      if (!mounted.current || handoffGeneration.current !== actionGeneration) return;
      const code = handoffErrorCode(error);
      if (code === "handoff-response-invalid") {
        // Native renewal succeeded before the response failed validation, so
        // the claim is still held. Restore that exact id instead of clearing
        // a UI state whose native slot remains live.
        const nextAttempts = Math.min(attempts + 1, MAX_HANDOFF_RECOVERY_ATTEMPTS);
        updateHandoffRecovery({ id, kind, action: "discard", attempts: nextAttempts });
        setError(null);
      } else if (classifyHandoffError(error) === "retryable") {
        const nextAttempts = Math.min(attempts + 1, MAX_HANDOFF_RECOVERY_ATTEMPTS);
        updateHandoffRecovery({ id, kind, action: "renew", attempts: nextAttempts });
        setError(null);
      } else {
        clearHandoffState();
        setError(handoffFailureMessage(
          error,
          "Log Lens source handoff lease를 갱신하지 못했습니다.",
        ));
      }
    } finally {
      if (handoffGeneration.current === actionGeneration) {
        handoffBusyRef.current = false;
        if (mounted.current) setHandoffBusy(false);
      }
    }
  }, [clearHandoffState, updateHandoffRecovery]);

  const retryHandoffRecovery = useCallback(() => {
    const recovery = handoffRecoveryRef.current;
    if (!recovery || handoffBusyRef.current || recovery.attempts >= MAX_HANDOFF_RECOVERY_ATTEMPTS) return;
    switch (recovery.action) {
      case "preview":
        void startLogSourcePreview(recovery.kind, recovery.id, recovery.attempts);
        break;
      case "discard":
        void restoreHandoffClaim(recovery.kind, recovery.id, recovery.attempts);
        break;
      case "accept":
        void applyAcceptedSource(recovery.kind, recovery.id, recovery.attempts);
        break;
      case "renew":
        void renewPreviewLease(recovery.kind, recovery.id, recovery.attempts);
        break;
    }
  }, [applyAcceptedSource, renewPreviewLease, restoreHandoffClaim, startLogSourcePreview]);

  // Drain at most one latest queued id after the current claim/action has
  // released its slot. This avoids unbounded UI memory while preserving a
  // deterministic handoff when producers race each other.
  useEffect(() => {
    if (handoffBusy || handoffPreview || handoffRecovery || !queuedHandoffRef.current) return;
    const queued = queuedHandoffRef.current;
    queuedHandoffRef.current = null;
    void openLogSourcePreview(queued);
  }, [handoffBusy, handoffPreview, handoffRecovery, openLogSourcePreview]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      generation.current += 1;
      refreshPending.current = null;
      handoffGeneration.current += 1;
      queuedHandoffRef.current = null;
      const active = operation.current;
      operation.current = null;
      if (active) void cancelRead(active);
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    setSavedViewsBusy(true);
    void listSavedViews()
      .then((document) => {
        if (disposed || !mounted.current) return;
        setSavedViews(document.views);
        setSavedViewsRevision(document.revision);
      })
      .catch((loadError: unknown) => {
        if (!disposed && mounted.current) setError(savedViewFailureMessage(loadError));
      })
      .finally(() => {
        if (!disposed && mounted.current) setSavedViewsBusy(false);
      });
    return () => {
      disposed = true;
    };
  }, []);

  // Cold-start pull and single-instance forwarding converge on the same
  // preview path. Merely receiving argv never reads a source or auto-adds it.
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    const consumePending = () => {
      if (disposed) return;
      void takePendingOpen()
        .then((request) => {
          if (disposed || !request || request.target.kind !== "handoff") return;
          if (!["log-source/v1", "webhook-log/v1"].includes(request.target.handoffKind)) {
            setError("지원하지 않는 Log Lens handoff입니다.");
            return;
          }
          void openLogSourcePreview(request.target);
        })
        .catch(() => {
          if (!disposed) setError("Log Lens source handoff를 확인하지 못했습니다.");
        });
    };
    let coldStartConsumed = false;
    const consumeColdStart = () => {
      if (disposed || coldStartConsumed) return;
      coldStartConsumed = true;
      consumePending();
    };
    void onOpenRequest(consumePending)
      .then((unlisten) => {
        if (disposed) unlisten();
        else {
          stop = unlisten;
          consumeColdStart();
        }
      })
      .catch(() => {
        if (disposed) return;
        setError("Log Lens source handoff listener를 시작하지 못했습니다.");
        consumeColdStart();
      });
    return () => {
      disposed = true;
      stop?.();
    };
  }, [openLogSourcePreview]);

  useEffect(() => {
    if (!handoffPreview) return undefined;
    const id = handoffPreview.id;
    const timer = window.setInterval(() => {
      if (handoffBusyRef.current || handoffRecoveryRef.current) return;
      const kind = handoffPreviewRef.current?.kind;
      if (kind) void renewPreviewLease(kind, id);
    }, 30_000);
    return () => window.clearInterval(timer);
  }, [handoffPreview, renewPreviewLease]);

  useEffect(() => {
    return () => {
      const preview = handoffPreviewRef.current;
      const recovery = handoffRecoveryRef.current;
      const id = preview?.id ?? recovery?.id;
      if (id) void discardLogSource(id).catch(() => undefined);
    };
  }, []);

  // Keep the handoff modal keyboard-contained and return focus to the element
  // that was active before an external request arrived. The action refs avoid
  // re-installing this listener for lease-only preview updates.
  useEffect(() => {
    const id = handoffPreview?.id ?? handoffRecovery?.id;
    const recoveryActive = handoffRecovery !== null;
    if (!id) return undefined;
    const opener = handoffOpenerRef.current;
    const focusTimer = window.setTimeout(() => {
      if (recoveryActive) handoffRetryRef.current?.focus();
      else handoffCancelRef.current?.focus();
    }, 0);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (handoffBusyRef.current) return;
        event.preventDefault();
        void cancelLogSourcePreview();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = [handoffCancelRef.current, handoffAcceptRef.current, handoffRetryRef.current]
        .filter((element): element is HTMLButtonElement => Boolean(element && !element.disabled));
      if (!focusable.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (event.shiftKey ? active === first : active === last) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      } else if (!focusable.includes(active as HTMLButtonElement)) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      window.clearTimeout(focusTimer);
      document.removeEventListener("keydown", onKeyDown);
      if (handoffPreviewRef.current?.id !== id
        && handoffRecoveryRef.current?.id !== id
        && opener?.isConnected) opener.focus();
    };
  }, [cancelLogSourcePreview, handoffPreview?.id, handoffRecovery?.id, handoffRecovery !== null]);

  useEffect(() => {
    if (!connected || !follow || paused || sources.length === 0) return undefined;
    const timer = window.setInterval(() => void refresh(), 1_500);
    return () => window.clearInterval(timer);
  }, [connected, follow, paused, refresh, sources.length]);

  const visibleRecords = useMemo(
    () => filterRecords(records, filter).slice(-MAX_RENDERED_ROWS),
    [filter, records],
  );

  const selectedRecords = useMemo(() => {
    const currentGeneration = snapshot?.generation ?? null;
    if (selected.size === 0
      || selected.size > MAX_SELECTED_RECORDS
      || selectedGeneration === null
      || currentGeneration === null
      || selectedGeneration !== currentGeneration
      || currentGeneration !== generation.current) return [];
    const resolved = records
      .filter((record) => selected.has(recordKey(record)))
      .sort((left, right) => compareSafeNumbers(left.timestampMillis, right.timestampMillis)
        || compareOpaqueIds(left.sourceId, right.sourceId)
        || left.sequence - right.sequence);
    return resolved.length === selected.size ? resolved : [];
  }, [busy, records, selected, selectedGeneration, snapshot?.generation]);

  const sendSelectedLogs = useCallback(async () => {
    if (toolboxBusyRef.current) return;
    const keys = new Set(selectedRef.current);
    const actionRevision = selectionRevisionRef.current;
    const actionGeneration = generation.current;
    const actionSnapshotGeneration = snapshotGenerationRef.current;
    const targets = records
      .filter((record) => keys.has(recordKey(record)))
      .sort((left, right) => compareSafeNumbers(left.timestampMillis, right.timestampMillis)
        || compareOpaqueIds(left.sourceId, right.sourceId)
        || left.sequence - right.sequence);
    const isCurrentSelection = () => mounted.current
      && selectionRevisionRef.current === actionRevision
      && generation.current === actionGeneration
      && snapshotGenerationRef.current === actionSnapshotGeneration
      && selectedGenerationRef.current === actionSnapshotGeneration
      && selectedRef.current.size === keys.size
      && [...keys].every((key) => selectedRef.current.has(key));

    if (keys.size === 0
      || keys.size > MAX_SELECTED_RECORDS
      || actionSnapshotGeneration === null
      || actionSnapshotGeneration !== actionGeneration
      || selectedGenerationRef.current !== actionSnapshotGeneration
      || targets.length !== keys.size) {
      invalidateSelection();
      setError(STALE_SELECTION_ERROR);
      return;
    }

    toolboxBusyRef.current = true;
    setToolboxBusy(true);
    setError(null);
    setNotice(null);
    try {
      const exported = await exportRecords(targets);
      if (!mounted.current) return;
      if (!isCurrentSelection()) {
        setError(STALE_SELECTION_ERROR);
        setNotice(null);
        return;
      }
      if (exported.truncated
        || exported.text.trim().length === 0
        || utf8ByteLength(exported.text) > MAX_TOOLBOX_TEXT_BYTES
        || Array.from(exported.text).length > MAX_TOOLBOX_TEXT_CHARS) {
        setError(TOOLBOX_EXPORT_ERROR);
        setNotice(null);
        return;
      }
      const dispatch = await sendSelectionToToolbox(exported.text);
      if (!mounted.current) return;
      if (!isCurrentSelection()) {
        setError(STALE_SELECTION_ERROR);
        setNotice(null);
        return;
      }
      setError(null);
      setNotice(dispatch.redacted ? TOOLBOX_SEND_REDACTED : TOOLBOX_SEND_SUCCESS);
    } catch {
      if (!mounted.current) return;
      setError(isCurrentSelection() ? TOOLBOX_SEND_ERROR : STALE_SELECTION_ERROR);
      setNotice(null);
    } finally {
      toolboxBusyRef.current = false;
      if (mounted.current) setToolboxBusy(false);
    }
  }, [invalidateSelection, records]);

  const buildSource = (): SourceSpec | null => {
    switch (kind) {
      case "localFile": return path ? { kind, path } : null;
      case "directory": return path && pattern ? { kind, path, pattern } : null;
      case "wslFile": return distro && path ? { kind, distro, path } : null;
      case "wslJournal": return distro ? (unit ? { kind, distro, unit } : { kind, distro }) : null;
      case "run": return sourceId ? { kind, sourceId } : null;
      case "webhookCapture": return null;
      case "container": return containerId ? { kind, engine, containerId } : null;
    }
  };

  const addSource = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const nativeEvent = event.nativeEvent as SubmitEvent & { isComposing?: boolean };
    if (compositionRef.current || nativeEvent.isComposing) return;
    const source = buildSource();
    if (!source) {
      setError("올바른 source 값을 입력하세요.");
      return;
    }
    if (sources.some((candidate) => JSON.stringify(candidate) === JSON.stringify(source))) {
      setError("이미 선택한 source입니다.");
      setNotice(null);
      return;
    }
    if (sources.length >= MAX_SOURCES) {
      setError(`source는 한 번에 최대 ${MAX_SOURCES}개까지 불러올 수 있습니다.`);
      setNotice(null);
      return;
    }
    const nextSources = [...sources, source];
    const nextCursors = nextSources.map(() => null);
    connectedRef.current = true;
    setConnected(true);
    setSources(nextSources);
    setCursors(nextCursors);
    setError(null);
    setNotice(null);
    void refresh(nextSources, nextCursors);
  };

  const removeSource = (index: number) => {
    const nextSources = sources.filter((_, sourceIndex) => sourceIndex !== index);
    const nextCursors = cursors.filter((_, sourceIndex) => sourceIndex !== index);
    setSources(nextSources);
    setCursors(nextCursors);
    setError(null);
    setNotice(null);
    if (nextSources.length && connectedRef.current) void refresh(nextSources, nextCursors);
    else {
      invalidateSelection();
      generation.current += 1;
      refreshPending.current = null;
      const active = operation.current;
      operation.current = null;
      if (active) void cancelRead(active);
      setBusy(false);
      setRecords([]);
      setSnapshot(null);
    }
  };

  const saveView = async () => {
    const name = viewName.trim();
    if (!name || name.length > 128 || !sources.length) {
      setError("뷰 이름과 하나 이상의 source가 필요합니다.");
      setNotice(null);
      return;
    }
    if (sources.some((source) => source.kind === "wslFile" || source.kind === "webhookCapture")) {
      setError("WSL 파일 및 일회성 Webhook capture source는 저장된 뷰에 보관할 수 없습니다.");
      setNotice(null);
      return;
    }
    const replacing = savedViews.some((view) => view.name === name);
    setError(null);
    setNotice(null);
    setSavedViewsBusy(true);
    try {
      const document = await saveSavedView(savedViewsRevision, {
        name,
        sources: structuredClone(sources),
        filter: { ...filter },
      });
      if (!mounted.current) return;
      setSavedViews(document.views);
      setSavedViewsRevision(document.revision);
      setSelectedViewName(name);
      setViewName("");
      setNotice(replacing ? `저장된 뷰 “${name}”을 업데이트했습니다.` : `뷰 “${name}”을 저장했습니다.`);
    } catch (saveError: unknown) {
      if (mounted.current) setError(savedViewFailureMessage(saveError));
    } finally {
      if (mounted.current) setSavedViewsBusy(false);
    }
  };

  const loadView = (name: string) => {
    setSelectedViewName(name);
    const view = savedViews.find((candidate) => candidate.name === name);
    if (!view) return;
    const nextSources = view.sources.map((source) => ({ ...source })) as SourceSpec[];
    const nextCursors = nextSources.map(() => null);
    connectedRef.current = false;
    setConnected(false);
    setFollow(false);
    setPaused(false);
    generation.current += 1;
    refreshPending.current = null;
    const active = operation.current;
    operation.current = null;
    if (active) void cancelRead(active);
    setSources(nextSources);
    setCursors(nextCursors);
    setFilter({ ...view.filter });
    setRecords([]);
    setSnapshot(null);
    selectedRef.current = new Set();
    selectedGenerationRef.current = null;
    setSelected(new Set());
    setSelectedGeneration(null);
    setBookmarks(new Set());
    setBusy(false);
    setError(null);
    setNotice(`저장된 뷰 “${name}” 설정을 불러왔습니다. source를 읽으려면 재연결하세요.`);
  };

  const deleteView = async () => {
    if (!selectedViewName) return;
    const deletedName = selectedViewName;
    setError(null);
    setNotice(null);
    setSavedViewsBusy(true);
    try {
      const document = await removeSavedView(savedViewsRevision, deletedName);
      if (!mounted.current) return;
      setSavedViews(document.views);
      setSavedViewsRevision(document.revision);
      setSelectedViewName("");
      setNotice(`저장된 뷰 “${deletedName}”을 삭제했습니다.`);
    } catch (deleteError: unknown) {
      if (mounted.current) setError(savedViewFailureMessage(deleteError));
    } finally {
      if (mounted.current) setSavedViewsBusy(false);
    }
  };

  const reconnectSources = async () => {
    if (!sources.length || busy) return;
    const nextCursors = sources.map(() => null);
    connectedRef.current = true;
    setConnected(true);
    setCursors(nextCursors);
    setRecords([]);
    setSnapshot(null);
    setError(null);
    setNotice("source에 다시 연결하는 중입니다.");
    await refresh(sources, nextCursors);
  };

  const toggleSelection = useCallback((record: LogRecord) => {
    const currentGeneration = snapshotGenerationRef.current;
    if (currentGeneration === null || currentGeneration !== generation.current) {
      invalidateSelection();
      setError(STALE_SELECTION_ERROR);
      return false;
    }
    const key = recordKey(record);
    const current = selectedRef.current;
    const next = new Set(current);
    if (next.has(key)) {
      next.delete(key);
    } else {
      if (next.size >= MAX_SELECTED_RECORDS) {
        setError(SELECTION_LIMIT_ERROR);
        setNotice(null);
        return false;
      }
      next.add(key);
    }
    selectedRef.current = next;
    selectedGenerationRef.current = next.size ? currentGeneration : null;
    selectionRevisionRef.current += 1;
    setSelected(next);
    setSelectedGeneration(next.size ? currentGeneration : null);
    setError(null);
    return true;
  }, [invalidateSelection]);

  const logContextItems = useMemo<readonly ContextMenuEntry[]>(() => {
    if (!contextRecord) return [];
    const key = recordKey(contextRecord);
    return [
      { type: "item", id: "copy-line", label: "로그 줄 복사", shortcut: "Ctrl+C" },
      {
        type: "item",
        id: "toggle-bookmark",
        label: bookmarks.has(key) ? "북마크 제거" : "북마크 추가",
      },
      {
        type: "item",
        id: "toggle-selection",
        label: selected.has(key) ? "로그 선택 해제" : "로그 선택",
      },
    ];
  }, [bookmarks, contextRecord, selected]);

  const onLogContextSelect = useCallback((id: string) => {
    const record = contextRecordRef.current;
    if (!record) return;
    const key = recordKey(record);
    if (id === "copy-line") {
      void exportRecords([record])
        .then(async (exported) => {
          if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
          await navigator.clipboard.writeText(exported.text.trimEnd());
          if (mounted.current) {
            if (exported.truncated) {
              setError(EXPORT_TRUNCATED_ERROR);
              setNotice(null);
            } else {
              setError(null);
              setNotice("로그 줄을 복사했습니다.");
            }
          }
        })
        .catch(() => {
          if (mounted.current) {
            setError("클립보드에 복사하지 못했습니다.");
            setNotice(null);
          }
        });
      return;
    }
    if (id === "toggle-bookmark") {
      setBookmarks((current) => {
        const next = new Set(current);
        if (next.has(key)) next.delete(key); else next.add(key);
        return next;
      });
      setNotice("북마크를 업데이트했습니다.");
    } else if (id === "toggle-selection") {
      if (toggleSelection(record)) setNotice("로그 선택을 업데이트했습니다.");
      return;
    }
    setError(null);
  }, [toggleSelection]);

  const toggleBookmark = (record: LogRecord) => {
    const key = recordKey(record);
    setBookmarks((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key); else next.add(key);
      return next;
    });
  };

  const exportVisible = async (copy: boolean) => {
    const targets = visibleRecords.filter((record) => selected.has(recordKey(record)));
    const exportTargets = targets.length ? targets : visibleRecords;
    if (!exportTargets.length) return;
    try {
      const exported = await exportRecords(exportTargets);
      if (copy) {
        await navigator.clipboard.writeText(exported.text);
        if (exported.truncated) {
          setError(EXPORT_TRUNCATED_ERROR);
          setNotice(null);
        }
        return;
      }
      const url = URL.createObjectURL(new Blob([exported.text], { type: "text/plain;charset=utf-8" }));
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = "log-lens-export.txt";
      anchor.style.display = "none";
      document.body.append(anchor);
      anchor.click();
      anchor.remove();
      window.setTimeout(() => URL.revokeObjectURL(url), 0);
      if (exported.truncated) setError(EXPORT_TRUNCATED_ERROR);
    } catch {
      setError(copy ? "클립보드에 복사하지 못했습니다." : "로그를 내보내지 못했습니다.");
    }
  };

  return (
    <main className="app-shell">
      {(handoffPreview || handoffRecovery) && <div className="handoff-backdrop" role="presentation">
        <section
          className="handoff-dialog"
          role="dialog"
          tabIndex={-1}
          aria-modal="true"
          aria-labelledby="log-source-handoff-title"
          aria-describedby="log-source-handoff-description"
          aria-busy={handoffBusy}
        >
          <h2 id="log-source-handoff-title">{handoffPreview ? "Log Lens source 미리보기" : "Log Lens source handoff 복구"}</h2>
          <p id="log-source-handoff-description" className="muted">
            {handoffPreview
              ? "아래의 검증된 읽기 전용 source만 추가합니다. 로그 원문, 명령, 환경변수, 자격 증명은 handoff에 포함되지 않습니다."
              : handoffRecovery?.action === "preview"
                ? "handoff 요청 ID를 유지하고 있습니다. 저장소 작업을 다시 시도해 주세요. 원문이나 경로는 표시하지 않습니다."
                : "handoff claim은 유지되고 있습니다. 저장소 복구를 다시 시도해 주세요. 원문이나 경로는 표시하지 않습니다."}
          </p>
          {handoffPreview && <dl className="handoff-details">
            <div><dt>보낸 앱</dt><dd>{handoffPreview.sourceApp}</dd></div>
            <div><dt>어댑터</dt><dd>{labelForKind(handoffPreview.source.kind)}</dd></div>
            <div><dt>비공개 source ID</dt><dd><code>{handoffPreview.source.sourceId}</code></dd></div>
          </dl>}
          {handoffRecovery && <p className="notice" role="status">
            {handoffRetryMessage(handoffRecovery.action, handoffRecovery.attempts)}
          </p>}
          <div className="handoff-actions">
            {handoffPreview && <button ref={handoffCancelRef} type="button" className="button" onClick={() => cancelLogSourcePreview()} disabled={handoffBusy || Boolean(handoffRecovery)}>취소</button>}
            {handoffPreview && <button ref={handoffAcceptRef} type="button" className="button primary" onClick={() => acceptLogSourcePreview()} disabled={handoffBusy || Boolean(handoffRecovery)}>읽기 전용 source 추가</button>}
            {handoffRecovery && <button
              ref={handoffRetryRef}
              type="button"
              className="button primary"
              onClick={() => retryHandoffRecovery()}
              disabled={handoffBusy || handoffRecovery.attempts >= MAX_HANDOFF_RECOVERY_ATTEMPTS}
            >
              {handoffRecovery.action === "discard"
                ? "복구 재시도"
                : handoffRecovery.action === "accept"
                  ? "source 추가 재시도"
                  : handoffRecovery.action === "renew"
                    ? "lease 갱신 재시도"
                    : "미리보기 재시도"}
            </button>}
          </div>
        </section>
      </div>}
      <header className="topbar">
        <div>
          <p className="eyebrow">DEVBOX · 오프라인</p>
          <h1>Log Lens</h1>
          <p className="subtitle">안전 제한을 적용한 로컬 로그 추적·병합·검사</p>
        </div>
        <div className="top-actions">
          <label className="toggle"><input type="checkbox" checked={follow} disabled={!connected} onChange={(event) => setFollow(event.target.checked)} /> 자동 새로고침</label>
          <label className="toggle"><input type="checkbox" checked={paused} onChange={(event) => setPaused(event.target.checked)} /> 화면 갱신 일시정지</label>
          <label className="toggle"><input type="checkbox" checked={wrapLines} onChange={(event) => setWrapLines(event.target.checked)} /> 줄 바꿈</label>
          <button className="button" type="button" onClick={() => void refresh()} disabled={busy || !connected || !sources.length}>새로고침</button>
          {!connected && <button className="button primary" type="button" onClick={() => void reconnectSources()} disabled={busy || !sources.length}>source 재연결</button>}
          <button className="button danger" type="button" onClick={() => { const active = operation.current; if (active) void cancelRead(active); }} disabled={!busy}>읽기 취소</button>
        </div>
      </header>

      <section className="source-panel" aria-labelledby="sources-heading">
        <div className="section-heading"><h2 id="sources-heading">로그 source</h2><span className="muted">{sources.length}/{MAX_SOURCES}개 선택 · 읽기 전용 · 네트워크 수집 없음 · 아카이브 없음</span></div>
        <form className="source-form" onSubmit={addSource} onCompositionStart={() => { compositionRef.current = true; }} onCompositionEnd={() => { compositionRef.current = false; }}>
          <label>종류<select value={kind} onChange={(event) => setKind(event.target.value as SourceKind)}>{(["localFile", "directory", "wslFile", "wslJournal", "run", "container"] as SourceKind[]).map((value) => <option key={value} value={value}>{labelForKind(value)}</option>)}</select></label>
          {(kind === "localFile" || kind === "directory" || kind === "wslFile") && <label>경로<input value={path} onChange={(event) => setPath(truncateUtf8(event.target.value, 4 * 1024))} placeholder={kind === "wslFile" ? "/var/log/app.log" : "C:\\logs\\app.log"} /></label>}
          {kind === "directory" && <label>파일 패턴<input value={pattern} onChange={(event) => setPattern(truncateUtf8(event.target.value, 128))} placeholder="*.log" /></label>}
          {(kind === "wslFile" || kind === "wslJournal") && <label>배포판<input value={distro} onChange={(event) => setDistro(truncateUtf8(event.target.value, 128))} placeholder="Ubuntu" /></label>}
          {kind === "wslJournal" && <label>Unit (선택)<input value={unit} onChange={(event) => setUnit(truncateUtf8(event.target.value, 128))} placeholder="sshd.service" /></label>}
          {kind === "run" && <label>비공개 source ID<input value={sourceId} onChange={(event) => setSourceId(truncateUtf8(event.target.value, 192))} placeholder="run-manager:run-1:stdout" /></label>}
          {kind === "container" && <><label>엔진<select value={engine} onChange={(event) => setEngine(event.target.value as ContainerEngine)}><option value="docker">Docker</option><option value="podman">Podman</option></select></label><label>컨테이너 ID/이름<input value={containerId} onChange={(event) => setContainerId(truncateUtf8(event.target.value, 128))} /></label></>}
          <button className="button primary" type="submit" disabled={busy}>source 추가</button>
        </form>
        <div className="source-list" aria-live="polite">{sources.length ? sources.map((source, index) => {
          const status = snapshot?.statuses[index];
          return <div className="source-chip" key={`${source.kind}-${index}`}><span>{labelForKind(source.kind)}</span><span className={`source-status source-status-${status ?? "pending"}`} aria-label={`상태: ${labelForStatus(status ?? "pending")}`}>{labelForStatus(status ?? "pending")}</span><button type="button" disabled={busy} aria-label={`${labelForKind(source.kind)} 제거`} onClick={() => removeSource(index)}>×</button></div>;
        }) : <span className="muted">선택한 source가 없습니다. 브라우저 모드에서는 제한된 예시를 표시합니다.</span>}</div>
      </section>

      <section className="filter-panel" aria-labelledby="filter-heading">
        <div className="section-heading"><h2 id="filter-heading">필터</h2><span className="muted">{visibleRecords.length}개 표시 · {records.length}개 보관</span></div>
        <div className="filter-row"><label className="filter-grow">텍스트<input value={filter.text} onChange={(event) => setFilter((current) => ({ ...current, text: truncateUtf8(event.target.value, 512) }))} placeholder="메시지 또는 필드 값" /></label><label className="toggle"><input type="checkbox" checked={filter.regex} onChange={(event) => setFilter((current) => ({ ...current, regex: event.target.checked }))} /> 정규식</label><label>레벨<select value={filter.level ?? ""} onChange={(event) => setFilter((current) => ({ ...current, level: (event.target.value || undefined) as LogLevel | undefined }))}><option value="">모든 레벨</option>{(["trace", "debug", "info", "warn", "error", "fatal"] as LogLevel[]).map((value) => <option key={value} value={value}>{value}</option>)}</select></label><label>Source 필터<select value={filter.sourceId ?? ""} onChange={(event) => setFilter((current) => ({ ...current, sourceId: event.target.value || undefined }))}><option value="">모든 source</option>{(snapshot?.sources ?? []).map((source) => <option key={source.sourceId} value={source.sourceId}>{labelForKind(source.kind)}</option>)}</select></label><button className="button" type="button" onClick={() => void exportVisible(false)} disabled={!visibleRecords.length}>내보내기</button><button className="button" type="button" onClick={() => void exportVisible(true)} disabled={!visibleRecords.length}>복사</button><button className="button primary" type="button" onClick={() => void sendSelectedLogs()} disabled={busy || toolboxBusy || !selectedRecords.length}>선택 로그를 Developer Toolbox로 보내기</button></div>
        <div className="filter-row"><label>필드<input value={filter.field ?? ""} onChange={(event) => setFilter((current) => ({ ...current, field: event.target.value ? truncateUtf8(event.target.value, 4 * 1024) : undefined }))} placeholder="필드 이름" /></label><label>필드 값<input value={filter.fieldValue ?? ""} onChange={(event) => setFilter((current) => ({ ...current, fieldValue: event.target.value ? truncateUtf8(event.target.value, 4 * 1024) : undefined }))} placeholder="값" /></label><label>시작 epoch ms<input type="number" value={filter.startAt ?? ""} onChange={(event) => setFilter((current) => ({ ...current, startAt: event.target.value ? Number(event.target.value) : undefined }))} /></label><label>종료 epoch ms<input type="number" value={filter.endAt ?? ""} onChange={(event) => setFilter((current) => ({ ...current, endAt: event.target.value ? Number(event.target.value) : undefined }))} /></label><label>뷰 이름<input value={viewName} onChange={(event) => setViewName(truncateUtf8(event.target.value, 128))} placeholder="뷰 이름" /></label><button className="button" type="button" onClick={() => void saveView()} disabled={savedViewsBusy || !sources.length || sources.some((source) => source.kind === "wslFile" || source.kind === "webhookCapture")}>저장</button><select aria-label="저장된 뷰 불러오기" value={selectedViewName} disabled={savedViewsBusy} onChange={(event) => loadView(event.target.value)}><option value="">뷰 불러오기…</option>{savedViews.map((view) => <option key={view.name} value={view.name}>{view.name}</option>)}</select><button className="button" type="button" onClick={() => void deleteView()} disabled={savedViewsBusy || !selectedViewName}>뷰 삭제</button></div>
      </section>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="notice" role="status" aria-live="polite">{notice}</p>}
      <section className="log-panel" aria-labelledby="log-heading">
        <div className="section-heading"><h2 id="log-heading">병합 결과</h2><span className="muted" aria-live="polite">{busy ? "읽는 중…" : snapshot?.truncated ? "안전 제한 도달" : "준비됨"}</span></div>
        <div className="log-table" role="table" aria-label="병합된 로그 레코드" aria-busy={busy}>
          <div className="log-row log-head" role="row"><span role="columnheader">선택</span><span role="columnheader">시간</span><span role="columnheader">레벨</span><span role="columnheader">Source</span><span role="columnheader">메시지</span><span role="columnheader">북마크</span></div>
          {visibleRecords.map((record) => { const key = recordKey(record); return <div className="log-row" role="row" tabIndex={0} key={key} data-log-key={key} aria-label={`로그 줄 ${record.sequence}: ${record.message}`} onContextMenu={(event) => { contextRecordRef.current = record; setContextRecord(record); logContextTrigger.onContextMenu?.(event); }} onKeyDown={(event) => { contextRecordRef.current = record; setContextRecord(record); logContextTrigger.onKeyDown?.(event); }}><span role="cell" className="row-select"><input type="checkbox" aria-label={`로그 줄 ${record.sequence} 선택`} checked={selected.has(key)} onChange={() => toggleSelection(record)} /></span><time role="cell" dateTime={toIsoTimestamp(record.timestampMillis)}>{formatTimestamp(record.timestampMillis)}</time><span role="cell" className={`level level-${record.level ?? "unknown"}`}>{record.level ?? "—"}</span><code role="cell" title={record.sourceId}>{record.sourceId.slice(-12)}</code><span role="cell" className={`message ${wrapLines ? "" : "nowrap"}`}>{highlightMessage(record.message, filter)}</span><span role="cell" className="bookmark-cell"><button className={`bookmark ${bookmarks.has(key) ? "active" : ""}`} type="button" aria-label={`${bookmarks.has(key) ? "북마크 제거" : "북마크 추가"}`} aria-pressed={bookmarks.has(key)} onClick={() => toggleBookmark(record)}>★</button></span></div>; })}
          {!visibleRecords.length && <p className="empty">현재 필터와 일치하는 레코드가 없습니다.</p>}
        </div>
      </section>
      <ContextMenu
        open={logContextMenu.open && contextRecord !== null}
        anchor={logContextMenu.anchor}
        restoreFocusTo={logContextMenu.restoreFocusTo}
        items={logContextItems}
        onSelect={onLogContextSelect}
        onClose={closeLogContextMenu}
        ariaLabel="로그 줄 작업"
      />
      <footer className="statusbar" aria-live="polite"><span>{connected ? "연결됨" : "재연결 필요"}</span><span>source {snapshot?.sources.length ?? 0}개</span><span>링 버퍼 제외 {snapshot?.droppedRecords ?? 0}개</span><span>북마크 {bookmarks.size}개</span><span>저장된 뷰 {savedViews.length}/{MAX_SAVED_VIEWS}</span></footer>
    </main>
  );
}

export default App;
