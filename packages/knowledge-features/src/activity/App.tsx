export {
  toDateStr,
  formatNullableCount,
  formatNullableTimestamp,
  formatRunSummary,
  formatKnowledgeSummary,
  formatDailyActivity,
  weekRange,
  monthRange,
  digestSourceExplanation,
  digestInputFromResponse,
  buildDigestInput,
  FIXED_SOURCE_EXPLANATIONS,
  buildExportInput,
} from "./lib/activityPresentation";
export { DataSourceRow } from "./components/DataSourceRow";
import { ActivityExportDialog } from "./components/ActivityExportDialog";
import { ActivityToolbar } from "./components/ActivityToolbar";
import { ActivitySummary } from "./components/ActivitySummary";
import { ActivityTimeline } from "./components/ActivityTimeline";
import { ActivitySettings } from "./components/ActivitySettings";
import {
  safeNativeErrorCode,
  toDateStr,
  type ViewTab,
  buildDigestInput,
  digestInputFromResponse,
  rangeFromDigest,
  dayFromDigest,
  buildExportInput,
} from "./lib/activityPresentation";
import { usePolling, useOperation } from "@devbox/hooks";
import { ContextMenu, useContextMenu, type ContextMenuEntry } from "@devbox/context-menu";
import { isImeComposing } from "@devbox/a11y";
import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  autostartStatus,
  cancelDigest,
  exportLifeLog,
  getDigest,
  getAppStats,
  getIdleThreshold,
  getPrivacyRules,
  getProjects,
  getTimeline,
  integrationSources,
  knowledgeDraftHistory,
  isTracking,
  projectAttribution,
  probeProject,
  EMPTY_PRIVACY_RULES,
  setProjects,
  saveLifeLog,
  saveDigest,
  sendDigestToKnowledge,
  startTracking,
  stopTracking,
  type ExportInput,
  type ExportFormat,
  type DigestResponse,
  type AttributionResult,
  type AutostartStatus,
  type PrivacyRules,
  type ProjectProbe,
  type SourceStatus,
  type KnowledgeDraftHistoryEntry,
} from "./api";
import { buildDateContextMenu, parseDateKey } from "./lib/contextMenu";
import { isTauri } from "./lib/isTauri";
import type { AppTotal, DaySummary, RangeSummary, Session } from "./types";
import "./App.css";

export default function App({
  active = true,
  selectedDate,
  onDateChange,
  onDaily,
  onDraft,
  lifecycleSettings,
  projectRevision = 0,
}: {
  active?: boolean;
  selectedDate?: string;
  projectRevision?: number;
  onDateChange?: (date: string) => void;
  onDaily?: () => void;
  onDraft?: () => void;
  lifecycleSettings?: React.ReactNode;
} = {}) {
  const activeRef = useRef(active);
  activeRef.current = active;
  const [localDate, setLocalDate] = useState(() => new Date());
  const date = useMemo(
    () => (selectedDate ? (parseDateKey(selectedDate) ?? localDate) : localDate),
    [selectedDate, localDate],
  );
  const setDate = useCallback(
    (next: Date) => {
      setLocalDate(next);
      onDateChange?.(toDateStr(next));
    },
    [onDateChange],
  );
  const dateStr = useMemo(() => toDateStr(date), [date]);
  const [view, setView] = useState<ViewTab>("day");
  const [day, setDay] = useState<DaySummary | null>(null);
  const [range, setRange] = useState<RangeSummary | null>(null);
  const [digest, setDigest] = useState<DigestResponse | null>(null);
  const [digestAppFilter, setDigestAppFilter] = useState<string | null>(null);
  const [attribution, setAttribution] = useState<AttributionResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [stats, setStats] = useState<AppTotal[]>([]);
  const [tracking, setTracking] = useState(false);
  const [projects, setProjectsState] = useState<string[]>([]);
  const [projectInput, setProjectInput] = useState("");
  const { busy: projectSaving, run: runProjectOperation } = useOperation();
  const [projectProbePath, setProjectProbePath] = useState<string | null>(null);
  const [projectProbes, setProjectProbes] = useState<Record<string, ProjectProbe>>({});
  const [idleThreshold, setIdleThresholdState] = useState(300000);
  const [privacy, setPrivacy] = useState<PrivacyRules>(EMPTY_PRIVACY_RULES);
  const [privacyHealthy, setPrivacyHealthy] = useState(false);
  const [autoStart, setAutoStart] = useState<AutostartStatus | null>(null);
  const [sources, setSources] = useState<SourceStatus[]>([]);
  const [draftHistory, setDraftHistory] = useState<KnowledgeDraftHistoryEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [contextDate, setContextDate] = useState<string | null>(null);
  const [transferBusy, setTransferBusy] = useState(false);
  const { busy: clipboardBusy, run: runClipboardOperation } = useOperation();
  const contextActionBusy = transferBusy || clipboardBusy;
  const [exportDialogOpen, setExportDialogOpen] = useState(false);
  const [exportStartDate, setExportStartDate] = useState(dateStr);
  const [exportEndDate, setExportEndDate] = useState(dateStr);
  const [exportFormat, setExportFormat] = useState<ExportFormat>("markdown");
  const exportBusyRef = useRef(false);
  const exportRequestRef = useRef(0);
  const digestBusyRef = useRef(false);
  const digestRequestRef = useRef(0);
  const loadRequestRef = useRef(0);
  const historyRequestRef = useRef(0);
  const projectSettingsRequestRef = useRef(0);
  const appMountedRef = useRef(true);
  const dateContextFocusRequestRef = useRef(0);
  const exportDialogRef = useRef<HTMLElement>(null);
  const exportFirstFieldRef = useRef<HTMLInputElement>(null);
  const exportRestoreFocusRef = useRef<HTMLElement | null>(null);
  const dailyChartRefs = useRef<Array<HTMLButtonElement | null>>([]);

  const invalidatePendingLoad = useCallback(() => {
    // Invalidate synchronously with the navigation event. The effect that
    // starts the next load runs later, so copy/save cannot briefly target the
    // previous period during React's batched state update.
    loadRequestRef.current += 1;
    setLoading(true);
    setDigest(null);
    setDay(null);
    setRange(null);
    setAttribution(null);
    setSessions([]);
    setStats([]);
    setError(null);
    setNotice(null);
  }, []);

  // An export can outlive the component (for example when the window closes
  // while the native command is still preparing Git data). Invalidate the
  // request token and busy ref during unmount so its completion cannot update
  // detached UI or make a later mount inherit a stale lock.
  useEffect(() => {
    appMountedRef.current = true;
    return () => {
      exportRequestRef.current += 1;
      exportBusyRef.current = false;
      digestRequestRef.current += 1;
      digestBusyRef.current = false;
      loadRequestRef.current += 1;
      historyRequestRef.current += 1;
      appMountedRef.current = false;
      dateContextFocusRequestRef.current += 1;
      void cancelDigest().catch(() => undefined);
    };
  }, []);

  const prepareDateContext = useCallback(
    (target: HTMLElement) => {
      const value = target.dataset.date;
      const parsed = value ? parseDateKey(value) : null;
      if (!value || !parsed) {
        setContextDate(null);
        return;
      }
      setContextDate(value);
      if (value === dateStr) return;
      invalidatePendingLoad();
      setDate(parsed);
    },
    [dateStr, invalidatePendingLoad, setDate],
  );
  const dateContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareDateContext(target),
  });
  const dateContextItems = useMemo<readonly ContextMenuEntry[]>(
    () => buildDateContextMenu(contextActionBusy || loading || contextDate === null),
    [contextActionBusy, contextDate, loading],
  );

  const copyContextDate = async () => {
    const value = contextDate;
    if (!value || !parseDateKey(value) || contextActionBusy || loading || exportBusyRef.current) return;
    return runClipboardOperation(async () => {
      setError(null);
      setNotice(null);
      try {
        if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
        await navigator.clipboard.writeText(value);
        setNotice(`${value} 날짜를 복사했습니다.`);
      } catch {
        setError("날짜를 클립보드에 복사하지 못했습니다.");
      }
    });
  };

  const saveOrDownloadExport = async (input: ExportInput): Promise<{ saved: boolean; preview: boolean }> => {
    if (isTauri()) {
      return { saved: (await saveLifeLog(input)).saved, preview: false };
    }
    const result = await exportLifeLog(input);
    const expectedExtension = input.format === "markdown" ? "md" : input.format;
    const expectedMime =
      input.format === "markdown"
        ? "text/markdown;charset=utf-8"
        : `${input.format === "json" ? "application/json" : "text/csv"};charset=utf-8`;
    const byteLength = new TextEncoder().encode(result.content).byteLength;
    if (
      result.origin !== "browser-preview" ||
      result.format !== input.format ||
      result.extension !== expectedExtension ||
      result.mimeType !== expectedMime ||
      result.byteLength !== byteLength ||
      result.byteLength > 4 * 1024 * 1024
    ) {
      throw new Error("export 미리보기 결과가 올바르지 않습니다");
    }
    if (typeof URL.createObjectURL !== "function") throw new Error("export 다운로드를 사용할 수 없습니다");
    const blob = new Blob([result.content], { type: result.mimeType });
    const anchor = document.createElement("a");
    let objectUrl: string | null = null;
    try {
      objectUrl = URL.createObjectURL(blob);
      anchor.href = objectUrl;
      anchor.download = `life-log-${input.startDate}-${input.endDate}.${expectedExtension}`;
      anchor.click();
    } catch {
      throw new Error("export 다운로드를 사용할 수 없습니다");
    } finally {
      if (objectUrl && typeof URL.revokeObjectURL === "function") {
        // Let the browser start the download before releasing the object URL.
        setTimeout(() => URL.revokeObjectURL(objectUrl!), 0);
      }
    }
    return { saved: true, preview: true };
  };

  const beginExport = (): number | null => {
    if (contextActionBusy || loading || exportBusyRef.current) return null;
    exportBusyRef.current = true;
    const request = exportRequestRef.current + 1;
    exportRequestRef.current = request;
    setTransferBusy(true);
    setError(null);
    setNotice(null);
    return request;
  };

  const isCurrentExport = (request: number): boolean => exportRequestRef.current === request;

  const finishExport = (request: number) => {
    if (!isCurrentExport(request)) return;
    exportBusyRef.current = false;
    setTransferBusy(false);
  };

  const exportNotice = (format: ExportFormat, preview: boolean): string =>
    `${format.toUpperCase()} 내보내기를 ${preview ? "브라우저 미리보기로 다운로드" : isTauri() ? "저장" : "다운로드"}했습니다.`;

  const exportFailure = (format: ExportFormat): string =>
    isTauri()
      ? `${format.toUpperCase()} 내보내기를 저장하지 못했습니다.`
      : `${format.toUpperCase()} 내보내기 미리보기를 다운로드하지 못했습니다.`;

  const exportDate = async (format: ExportFormat) => {
    const value = contextDate;
    const input = value ? buildExportInput(value, value, format) : null;
    if (!input) return;
    const request = beginExport();
    if (request === null) return;
    try {
      const outcome = await saveOrDownloadExport(input);
      if (isCurrentExport(request) && outcome.saved) setNotice(exportNotice(format, outcome.preview));
    } catch {
      // Native path/OS 오류와 parser/DB 내부 오류를 UI에 반향하지 않는다.
      if (isCurrentExport(request)) setError(exportFailure(format));
    } finally {
      finishExport(request);
    }
  };

  const openExportDialog = () => {
    if (contextActionBusy || loading || exportBusyRef.current) return;
    setExportStartDate(contextDate ?? dateStr);
    setExportEndDate(contextDate ?? dateStr);
    setExportFormat("markdown");
    setExportDialogOpen(true);
  };

  const submitRangeExport = async () => {
    const input = buildExportInput(exportStartDate, exportEndDate, exportFormat);
    if (!input) {
      setError("내보내기 날짜 범위가 올바르지 않습니다. 최대 366일까지 선택할 수 있습니다.");
      return;
    }
    const request = beginExport();
    if (request === null) return;
    try {
      const outcome = await saveOrDownloadExport(input);
      if (isCurrentExport(request) && outcome.saved) {
        setNotice(exportNotice(input.format, outcome.preview));
        setExportDialogOpen(false);
      }
    } catch {
      if (isCurrentExport(request)) setError(exportFailure(exportFormat));
    } finally {
      finishExport(request);
    }
  };

  const beginDigestAction = (): { request: number; loadRequest: number } | null => {
    if (contextActionBusy || exportBusyRef.current || digestBusyRef.current || !digest) return null;
    digestBusyRef.current = true;
    const request = digestRequestRef.current + 1;
    digestRequestRef.current = request;
    setTransferBusy(true);
    setError(null);
    setNotice(null);
    return { request, loadRequest: loadRequestRef.current };
  };

  const finishDigestAction = (action: { request: number; loadRequest: number }) => {
    if (digestRequestRef.current !== action.request) return;
    digestBusyRef.current = false;
    setTransferBusy(false);
  };

  const isCurrentDigestAction = (action: { request: number; loadRequest: number }): boolean =>
    digestRequestRef.current === action.request && loadRequestRef.current === action.loadRequest;

  const copyDigest = async () => {
    const response = digest;
    const action = beginDigestAction();
    if (!response || action === null) return;
    try {
      if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(response.markdown);
      if (isCurrentDigestAction(action)) setNotice("현재 요약을 클립보드에 복사했습니다.");
    } catch {
      if (isCurrentDigestAction(action)) setError("요약을 클립보드에 복사하지 못했습니다.");
    } finally {
      finishDigestAction(action);
    }
  };

  const downloadDigest = async () => {
    const response = digest;
    const action = beginDigestAction();
    if (!response || action === null) return;
    try {
      if (isTauri()) {
        if (!response.handle) throw new Error("요약 핸들을 사용할 수 없습니다");
        const result = await saveDigest(response.handle);
        if (isCurrentDigestAction(action) && result.saved) {
          setNotice("현재 요약을 저장했습니다.");
        }
      } else {
        if (typeof URL.createObjectURL !== "function") throw new Error("다운로드를 사용할 수 없습니다");
        const blob = new Blob([response.markdown], { type: "text/markdown;charset=utf-8" });
        const anchor = document.createElement("a");
        let objectUrl: string | null = null;
        try {
          objectUrl = URL.createObjectURL(blob);
          anchor.href = objectUrl;
          anchor.download = `life-log-${response.document.period}-${response.document.range.startDate}-${response.document.range.endDate}.md`;
          anchor.click();
        } finally {
          if (objectUrl && typeof URL.revokeObjectURL === "function") {
            setTimeout(() => URL.revokeObjectURL(objectUrl!), 0);
          }
        }
        if (isCurrentDigestAction(action)) setNotice("현재 요약을 브라우저 미리보기로 다운로드했습니다.");
      }
    } catch {
      if (isCurrentDigestAction(action)) {
        setError(isTauri() ? "요약을 저장하지 못했습니다." : "요약 미리보기를 다운로드하지 못했습니다.");
      }
    } finally {
      finishDigestAction(action);
    }
  };

  const sendDigestDraft = async () => {
    if (!isTauri()) return;
    const response = digest;
    const action = beginDigestAction();
    if (!response || action === null) return;
    try {
      const result = await sendDigestToKnowledge(digestInputFromResponse(response));
      if (isCurrentDigestAction(action) && result.kind === "knowledge-draft/v1") {
        onDraft?.();
        await refreshDraftHistory();
        setNotice("Knowledge 초안을 미리보기로 보냈습니다. 저장 전 내용을 확인하세요.");
      }
    } catch {
      if (isCurrentDigestAction(action)) {
        setError("Knowledge 초안을 보내지 못했습니다. 잠시 후 다시 시도하세요.");
      }
    } finally {
      finishDigestAction(action);
    }
  };

  const regenerateDraft = async (entry: KnowledgeDraftHistoryEntry) => {
    if (!isTauri() || !digest) return;
    const action = beginDigestAction();
    if (!action) return;
    try {
      const result = await sendDigestToKnowledge(digestInputFromResponse(digest), entry.handoffId);
      if (isCurrentDigestAction(action) && result.kind === "knowledge-draft/v1") {
        onDraft?.();
        await refreshDraftHistory();
        setNotice("새 Knowledge 초안을 만들었습니다. 이전 handoff와 별도의 ID로 다시 확인하세요.");
      }
    } catch {
      if (isCurrentDigestAction(action)) setError("Knowledge 초안을 다시 만들지 못했습니다.");
    } finally {
      finishDigestAction(action);
    }
  };

  // Keep keyboard focus inside the modal and return it to the control that
  // opened the dialog. The busy ref is used instead of a state dependency so
  // a progress update cannot tear down and recreate the focus trap.
  useEffect(() => {
    if (!active || !exportDialogOpen) return;
    exportRestoreFocusRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = exportDialogRef.current;
    const focusTask = window.setTimeout(() => exportFirstFieldRef.current?.focus(), 0);
    const onKeyDown = (event: KeyboardEvent) => {
      if (isImeComposing(event)) return;
      if (event.key === "Escape") {
        if (!exportBusyRef.current) setExportDialogOpen(false);
        event.preventDefault();
        return;
      }
      if (event.key !== "Tab" || !dialog) return;
      const focusable = Array.from(
        dialog.querySelectorAll<HTMLElement>(
          "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
        ),
      ).filter((element) => !element.hasAttribute("aria-hidden"));
      if (focusable.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      window.clearTimeout(focusTask);
      document.removeEventListener("keydown", onKeyDown);
      if (activeRef.current && exportRestoreFocusRef.current?.isConnected) exportRestoreFocusRef.current.focus();
      exportRestoreFocusRef.current = null;
    };
  }, [active, exportDialogOpen]);

  const restoreDateContextFocus = (request: number, target: HTMLElement | null, value: string | null) => {
    if (!target || !value || !parseDateKey(value)) return;
    const triggerClass = target.classList.contains("daily-col")
      ? "daily-col"
      : target.classList.contains("date-input")
        ? "date-input"
        : null;
    window.setTimeout(() => {
      if (dateContextFocusRequestRef.current !== request) return;
      const replacement = target.isConnected
        ? target
        : Array.from(document.querySelectorAll<HTMLElement>("[data-date]")).find(
            (element) =>
              element.dataset.date === value && (triggerClass === null || element.classList.contains(triggerClass)),
          );
      if (
        !replacement ||
        (replacement instanceof HTMLButtonElement && replacement.disabled) ||
        (replacement instanceof HTMLInputElement && replacement.disabled)
      )
        return;
      replacement.focus({ preventScroll: true });
    }, 0);
  };

  const onDateContextSelect = (id: string) => {
    const focusRequest = dateContextFocusRequestRef.current + 1;
    dateContextFocusRequestRef.current = focusRequest;
    const restoreFocusTo = dateContextMenu.restoreFocusTo;
    const selectedDate = contextDate;
    let action: Promise<void> | null = null;
    if (id === "copy-date") action = copyContextDate();
    if (id === "export-markdown") action = exportDate("markdown");
    if (id === "export-json") action = exportDate("json");
    if (id === "export-csv") action = exportDate("csv");
    if (action) {
      void action.finally(() => restoreDateContextFocus(focusRequest, restoreFocusTo, selectedDate));
    }
  };

  const refreshDraftHistory = useCallback(async () => {
    if (!isTauri()) {
      setDraftHistory([]);
      return;
    }
    const request = historyRequestRef.current + 1;
    historyRequestRef.current = request;
    try {
      const history = await knowledgeDraftHistory();
      if (appMountedRef.current && historyRequestRef.current === request) {
        setDraftHistory(history);
      }
    } catch {
      // History is auxiliary UI state; a digest remains usable when the
      // local reconciliation store is temporarily unavailable.
    }
  }, []);

  const loadSettings = useCallback(async () => {
    const projectRequest = projectSettingsRequestRef.current + 1;
    projectSettingsRequestRef.current = projectRequest;
    const historyRequest = historyRequestRef.current + 1;
    historyRequestRef.current = historyRequest;
    try {
      const [pr, idle, privacyRules, ast, src, history] = await Promise.allSettled([
        getProjects(),
        getIdleThreshold(),
        getPrivacyRules(),
        autostartStatus(),
        integrationSources(),
        knowledgeDraftHistory(),
      ]);
      if (!appMountedRef.current) return;
      if (pr.status === "fulfilled" && projectSettingsRequestRef.current === projectRequest) {
        setProjectsState(pr.value);
      }
      if (idle.status === "fulfilled") setIdleThresholdState(idle.value);
      if (privacyRules.status === "fulfilled") {
        setPrivacy(privacyRules.value.rules);
        setPrivacyHealthy(privacyRules.value.healthy);
      } else {
        setPrivacyHealthy(false);
      }
      if (ast.status === "fulfilled") setAutoStart(ast.value);
      if (src.status === "fulfilled") setSources(src.value);
      if (history.status === "fulfilled" && appMountedRef.current && historyRequestRef.current === historyRequest) {
        setDraftHistory(history.value);
      }
      if ([pr, idle, privacyRules, ast, src, history].some((result) => result.status === "rejected")) {
        setError("일부 Life Log 설정을 불러오지 못했습니다.");
      }
    } catch {
      setError("Life Log 설정을 불러오지 못했습니다.");
    }
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: existing dependency list; review in P1-15
  const load = useCallback(async () => {
    const request = loadRequestRef.current + 1;
    loadRequestRef.current = request;
    setLoading(true);
    setDigest(null);
    setDay(null);
    setRange(null);
    setAttribution(null);
    setSessions([]);
    setStats([]);
    setError(null);
    setNotice(null);
    try {
      // A previous request may still be inside the native DB progress hook or
      // bounded Git child. Wait for its cancellation command before claiming
      // the single-flight slot for this generation.
      if (isTauri()) {
        try {
          await cancelDigest();
        } catch {
          // The following native request still has its own fixed error path.
        }
      }
      if (loadRequestRef.current !== request) return;
      if (view === "day") {
        const input = buildDigestInput(date, "day", digestAppFilter);
        if (!input) throw new Error("invalid digest range");
        const nextDigest = await getDigest(input);
        if (loadRequestRef.current !== request) return;
        setDay(dayFromDigest(nextDigest));
        setDigest(nextDigest);
        try {
          const nextAttribution = await projectAttribution(input.dayStart, input.dayEnd);
          if (loadRequestRef.current === request) setAttribution(nextAttribution);
        } catch {
          if (loadRequestRef.current === request) setAttribution(null);
        }
      } else if (view === "week") {
        const input = buildDigestInput(date, "week", digestAppFilter);
        if (!input) throw new Error("invalid digest range");
        const nextDigest = await getDigest(input);
        if (loadRequestRef.current !== request) return;
        setRange(rangeFromDigest(nextDigest, `${input.startDate} ~ ${input.endDate}`));
        setDigest(nextDigest);
      } else if (view === "month") {
        const input = buildDigestInput(date, "month", digestAppFilter);
        if (!input) throw new Error("invalid digest range");
        const nextDigest = await getDigest(input);
        if (loadRequestRef.current !== request) return;
        setRange(rangeFromDigest(nextDigest, input.startDate.slice(0, 7)));
        setDigest(nextDigest);
      } else if (view === "timeline") {
        const dayInput = buildExportInput(dateStr, dateStr, "json");
        if (!dayInput) throw new Error("invalid day range");
        const [ts, st, tr] = await Promise.all([
          getTimeline(dayInput.dayStart, dayInput.dayEnd),
          getAppStats(dayInput.dayStart, dayInput.dayEnd),
          isTracking(),
        ]);
        if (loadRequestRef.current !== request) return;
        setSessions(ts);
        setStats(st);
        setTracking(tr);
      }
    } catch (reason) {
      if (loadRequestRef.current === request) {
        const code = safeNativeErrorCode(reason);
        setError(
          view === "week" || view === "month"
            ? `local digest를 불러오지 못했습니다.${code ? ` (${code})` : ""}`
            : `Life Log 데이터를 불러오지 못했습니다.${code ? ` (${code})` : ""}`,
        );
      }
    } finally {
      if (loadRequestRef.current === request) setLoading(false);
    }
  }, [view, date, dateStr, digestAppFilter, projectRevision]);

  useEffect(() => {
    void load();
  }, [load]);

  // Settings are app state, not date/view state. Reloading them on every
  // navigation can race with an acknowledged save and overwrite the
  // authoritative rows with an older request.
  useEffect(() => {
    void loadSettings();
  }, [loadSettings]);

  usePolling(refreshDraftHistory, { intervalMs: 30_000, active: isTauri(), immediate: false });
  usePolling(load, { intervalMs: 30_000, active: view === "timeline", immediate: false });

  const toggleTracking = async () => {
    setError(null);
    try {
      if (tracking) {
        await stopTracking();
        setTracking(false);
      } else {
        await startTracking();
        setTracking(true);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const addProject = async () => {
    const p = projectInput.trim();
    if (!p || projectSaving) return;
    const next = projects.includes(p) ? projects : [...projects, p];
    return runProjectOperation(async () => {
      setError(null);
      try {
        const saved = await setProjects(next);
        projectSettingsRequestRef.current += 1;
        setProjectsState(saved);
        setProjectInput("");
        setNotice("Git 프로젝트 경로를 저장했습니다.");
      } catch (reason) {
        const code = safeNativeErrorCode(reason);
        setError(`Git 프로젝트 경로를 저장하지 못했습니다.${code ? ` (${code})` : ""}`);
      }
    });
  };

  const removeProject = async (p: string) => {
    if (projectSaving) return;
    const next = projects.filter((x) => x !== p);
    return runProjectOperation(async () => {
      setError(null);
      try {
        const saved = await setProjects(next);
        projectSettingsRequestRef.current += 1;
        setProjectsState(saved);
        setProjectProbes((current) => {
          const copy = { ...current };
          delete copy[p];
          return copy;
        });
        setNotice("Git 프로젝트 경로를 제거했습니다.");
      } catch (reason) {
        const code = safeNativeErrorCode(reason);
        setError(`Git 프로젝트 경로를 제거하지 못했습니다.${code ? ` (${code})` : ""}`);
      }
    });
  };

  const checkProject = async (path: string) => {
    if (projectProbePath) return;
    setProjectProbePath(path);
    setError(null);
    try {
      const result = await probeProject(path);
      setProjectProbes((current) => ({ ...current, [path]: result }));
    } catch (reason) {
      const code = safeNativeErrorCode(reason);
      setProjectProbes((current) => ({
        ...current,
        [path]: { path, target: "windows", repository: false, errorCode: code ?? "project_probe_failed" },
      }));
    } finally {
      setProjectProbePath(null);
    }
  };

  const shift = (delta: number) => {
    if (contextActionBusy) return;
    const d = new Date(date);
    if (view === "day") d.setDate(d.getDate() + delta);
    else if (view === "week") d.setDate(d.getDate() + delta * 7);
    else if (view === "month") d.setMonth(d.getMonth() + delta);
    invalidatePendingLoad();
    setDate(d);
  };

  const selectDate = (next: Date) => {
    if (contextActionBusy) return;
    invalidatePendingLoad();
    setDate(next);
  };

  const selectView = (next: ViewTab) => {
    if (contextActionBusy || view === next) return;
    invalidatePendingLoad();
    setView(next);
  };

  const selectDigestFilter = (next: string | null) => {
    if (contextActionBusy || loading) return;
    invalidatePendingLoad();
    setDigestAppFilter(next);
  };

  const cancelCurrentLoad = async () => {
    if (!loading) return;
    invalidatePendingLoad();
    const cancellationRequest = loadRequestRef.current;
    try {
      await cancelDigest();
      if (loadRequestRef.current === cancellationRequest) setLoading(false);
    } catch {
      // Keep the busy state while the native generation still owns the
      // single-flight slot; a new Git/DB request must not race a timeout.
      if (loadRequestRef.current === cancellationRequest) {
        setError("현재 digest 작업을 취소하지 못했습니다. 잠시 후 다시 시도해 주세요.");
      }
    }
  };

  const onDailyChartKeyDown = (
    event: ReactKeyboardEvent<HTMLButtonElement>,
    index: number,
    points: RangeSummary["daily"],
  ) => {
    if (points.length === 0) return;
    let nextIndex: number | null = null;
    if (event.key === "ArrowLeft" || event.key === "ArrowUp") nextIndex = Math.max(0, index - 1);
    if (event.key === "ArrowRight" || event.key === "ArrowDown") nextIndex = Math.min(points.length - 1, index + 1);
    if (event.key === "Home") nextIndex = 0;
    if (event.key === "End") nextIndex = points.length - 1;
    if (nextIndex == null || nextIndex === index) return;
    event.preventDefault();
    const point = points[nextIndex];
    const button = dailyChartRefs.current[nextIndex];
    if (!point || !button) return;
    button.focus();
    selectDate(new Date(point.day_ms));
  };

  const topApp = day?.app_totals[0];
  const maxDaily = Math.max(1, ...(range?.daily.map((d) => d.pc_usage_ms) ?? []));
  const maxStatDuration = Math.max(1, ...stats.map((stat) => stat.duration_ms));
  const summary = view === "day" ? day : range;
  const maxSummaryDuration = Math.max(1, ...(summary?.app_totals.map((app) => app.duration_ms) ?? []));
  const digestAppOptions = useMemo(() => {
    const options = digest?.document.appTotals.map((app) => app.app) ?? [];
    if (digestAppFilter && !options.includes(digestAppFilter)) options.unshift(digestAppFilter);
    return options;
  }, [digest, digestAppFilter]);

  return (
    <div className="app">
      <ActivityToolbar
        onDaily={onDaily}
        contextActionBusy={contextActionBusy}
        shift={shift}
        dateStr={dateStr}
        selectDate={selectDate}
        dateContextMenu={dateContextMenu}
        loading={loading}
        cancelCurrentLoad={cancelCurrentLoad}
        view={view}
        selectView={selectView}
        load={load}
        openExportDialog={openExportDialog}
      />

      {error && (
        <div className="error" role="alert">
          {error}
        </div>
      )}
      {notice && (
        <div className="notice" role="status" aria-live="polite">
          {notice}
        </div>
      )}

      {view === "settings" ? (
        <ActivitySettings
          lifecycleSettings={lifecycleSettings}
          sources={sources}
          refreshDraftHistory={refreshDraftHistory}
          contextActionBusy={contextActionBusy}
          draftHistory={draftHistory}
          regenerateDraft={regenerateDraft}
          digest={digest}
          loading={loading}
          projects={projects}
          projectProbes={projectProbes}
          checkProject={checkProject}
          projectProbePath={projectProbePath}
          projectSaving={projectSaving}
          removeProject={removeProject}
          projectInput={projectInput}
          setProjectInput={setProjectInput}
          addProject={addProject}
          idleThreshold={idleThreshold}
          setIdleThresholdState={setIdleThresholdState}
          autoStart={autoStart}
          setError={setError}
          setNotice={setNotice}
          setAutoStart={setAutoStart}
          privacy={privacy}
          privacyHealthy={privacyHealthy}
          setPrivacy={setPrivacy}
          setPrivacyHealthy={setPrivacyHealthy}
        />
      ) : view === "timeline" ? (
        <ActivityTimeline
          tracking={tracking}
          toggleTracking={toggleTracking}
          sessions={sessions}
          stats={stats}
          maxStatDuration={maxStatDuration}
        />
      ) : (
        <ActivitySummary
          summary={summary}
          topApp={topApp}
          view={view}
          loading={loading}
          contextActionBusy={contextActionBusy}
          cancelCurrentLoad={cancelCurrentLoad}
          copyDigest={copyDigest}
          digest={digest}
          downloadDigest={downloadDigest}
          sendDigestDraft={sendDigestDraft}
          digestAppFilter={digestAppFilter}
          selectDigestFilter={selectDigestFilter}
          digestAppOptions={digestAppOptions}
          range={range}
          dateStr={dateStr}
          dailyChartRefs={dailyChartRefs}
          onDailyChartKeyDown={onDailyChartKeyDown}
          selectDate={selectDate}
          dateContextMenu={dateContextMenu}
          maxDaily={maxDaily}
          maxSummaryDuration={maxSummaryDuration}
          attribution={attribution}
        />
      )}
      {exportDialogOpen && (
        <div className="export-backdrop" role="presentation">
          <ActivityExportDialog
            exportDialogRef={exportDialogRef}
            contextActionBusy={contextActionBusy}
            exportFirstFieldRef={exportFirstFieldRef}
            exportStartDate={exportStartDate}
            setExportStartDate={setExportStartDate}
            exportEndDate={exportEndDate}
            setExportEndDate={setExportEndDate}
            exportFormat={exportFormat}
            setExportFormat={setExportFormat}
            setExportDialogOpen={setExportDialogOpen}
            submitRangeExport={submitRangeExport}
          />
        </div>
      )}
      <ContextMenu
        open={dateContextMenu.open}
        anchor={dateContextMenu.anchor}
        restoreFocusTo={dateContextMenu.restoreFocusTo}
        items={dateContextItems}
        onSelect={onDateContextSelect}
        onClose={dateContextMenu.close}
        ariaLabel="Life Log 날짜 메뉴"
      />
    </div>
  );
}
