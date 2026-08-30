import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
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
  redactExisting,
  setAutostart,
  setIdleThreshold,
  setPrivacyRules,
  setProjects,
  saveLifeLog,
  saveDigest,
  sendDigestToKnowledge,
  startTracking,
  stopTracking,
  type ExportDayBoundary,
  type ExportInput,
  type ExportFormat,
  type DigestInput,
  type DigestDay,
  type DigestPeriod,
  type DigestResponse,
  type KnowledgeDigest,
  type RunDigest,
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

function fmtDuration(ms: number): string {
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

function shortApp(app: string): string {
  return Array.from(app.replace(/\.exe$/i, "")).slice(0, 22).join("");
}

function safeNativeErrorCode(error: unknown): string | null {
  const value = error instanceof Error ? error.message : String(error ?? "");
  return /^[a-z][a-z0-9_]{0,63}$/.test(value) ? value : null;
}

export function toDateStr(d: Date): string {
  return `${String(d.getFullYear()).padStart(4, "0")}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function fmtDay(dayMs: number): string {
  const d = new Date(dayMs);
  return `${d.getMonth() + 1}/${d.getDate()}`;
}

function fmtTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function formatNullableCount(value: number | null): string {
  return value == null ? "—" : value.toLocaleString();
}

export function formatNullableTimestamp(value: number | null): string {
  return value == null ? "—" : new Date(value).toLocaleString();
}

export function formatRunSummary(run: RunDigest | null): string {
  return run == null
    ? "—"
    : `${formatNullableCount(run.succeeded)} succeeded · ${formatNullableCount(run.failed)} failed`;
}

export function formatKnowledgeSummary(knowledge: KnowledgeDigest | null): string {
  return knowledge == null
    ? "—"
    : `${formatNullableCount(knowledge.notesModified)} modified`;
}

export function formatDailyActivity(day: Pick<DigestDay, "runSucceeded" | "runFailed" | "knowledgeNotesModified">): string {
  const runs = day.runSucceeded == null || day.runFailed == null
    ? "Run —"
    : `Run ${formatNullableCount(day.runSucceeded)}/${formatNullableCount(day.runFailed)}`;
  const notes = day.knowledgeNotesModified == null
    ? "Knowledge —"
    : `Knowledge ${formatNullableCount(day.knowledgeNotesModified)}`;
  return `${runs} · ${notes}`;
}

type ViewTab = "day" | "week" | "month" | "timeline" | "settings";

export function weekRange(date: Date): { start: number; end: number } {
  const d = new Date(date);
  const day = (d.getDay() + 6) % 7; // 월요일 시작
  d.setDate(d.getDate() - day);
  const start = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const endDate = new Date(d.getFullYear(), d.getMonth(), d.getDate() + 7);
  return { start, end: endDate.getTime() };
}

export function monthRange(date: Date): { start: number; end: number } {
  const start = new Date(date.getFullYear(), date.getMonth(), 1).getTime();
  const end = new Date(date.getFullYear(), date.getMonth() + 1, 1).getTime();
  return { start, end };
}

function periodDateKeys(date: Date, period: DigestPeriod): { startDate: string; endDate: string } {
  if (period === "day") {
    const day = new Date(date.getFullYear(), date.getMonth(), date.getDate());
    const dateKey = toDateStr(day);
    return { startDate: dateKey, endDate: dateKey };
  }
  if (period === "week") {
    const start = new Date(date.getFullYear(), date.getMonth(), date.getDate());
    const day = (start.getDay() + 6) % 7;
    start.setDate(start.getDate() - day);
    const end = new Date(start.getFullYear(), start.getMonth(), start.getDate() + 6);
    return { startDate: toDateStr(start), endDate: toDateStr(end) };
  }
  const start = new Date(date.getFullYear(), date.getMonth(), 1);
  const end = new Date(date.getFullYear(), date.getMonth() + 1, 0);
  return { startDate: toDateStr(start), endDate: toDateStr(end) };
}

export function buildDigestInput(
  date: Date,
  period: DigestPeriod,
  app: string | null = null,
): DigestInput | null {
  const { startDate, endDate } = periodDateKeys(date, period);
  const range = buildExportInput(startDate, endDate, "json");
  if (!range) return null;
  // `format` belongs to the export DTO but not to the digest wire contract.
  return {
    startDate: range.startDate,
    endDate: range.endDate,
    timezone: range.timezone,
    dayStart: range.dayStart,
    dayEnd: range.dayEnd,
    dayBoundaries: range.dayBoundaries,
    period,
    filter: { app },
  };
}

export function digestInputFromResponse(response: DigestResponse): DigestInput {
  const { document } = response;
  return {
    startDate: document.range.startDate,
    endDate: document.range.endDate,
    timezone: document.range.timezone,
    dayStart: document.range.startMs,
    dayEnd: document.range.endMs,
    dayBoundaries: document.range.dayBoundaries.map((boundary) => ({ ...boundary })),
    period: document.period,
    filter: { ...document.filter },
  };
}

function rangeFromDigest(response: DigestResponse, label: string): RangeSummary {
  return {
    label,
    pc_usage_ms: response.document.summary.pcUsageMs,
    app_totals: response.document.appTotals.map((app) => ({
      app: app.app,
      duration_ms: app.durationMs,
      sessions: app.sessions,
    })),
    git: {
      projects: response.document.git.projects.map((project) => ({
        path: project.path,
        commits: project.commits,
      })),
      total_commits: response.document.git.totalCommits,
    },
    daily: response.document.daily.map((day) => ({
      day_ms: day.startMs,
      pc_usage_ms: day.pcUsageMs,
    })),
  };
}

function dayFromDigest(response: DigestResponse): DaySummary {
  return {
    date: response.document.range.startDate,
    pc_usage_ms: response.document.summary.pcUsageMs,
    app_totals: response.document.appTotals.map((app) => ({
      app: app.app,
      duration_ms: app.durationMs,
      sessions: app.sessions,
    })),
    git: {
      projects: response.document.git.projects.map((project) => ({
        path: project.path,
        commits: project.commits,
      })),
      total_commits: response.document.git.totalCommits,
    },
  };
}

function digestSourceDetails(
  source: DigestResponse["document"]["sources"][number],
): string | null {
  const details: string[] = [];
  if (Number.isSafeInteger(source.schemaVersion) && source.schemaVersion != null && source.schemaVersion > 0) {
    details.push(`schema v${source.schemaVersion}`);
  }
  if (Number.isSafeInteger(source.snapshotVersion) && source.snapshotVersion != null && source.snapshotVersion > 0) {
    details.push(`snapshot v${source.snapshotVersion}`);
  }
  if (source.producerVersion && /^[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?(?:\+[A-Za-z0-9.-]+)?$/.test(source.producerVersion)) {
    details.push(source.producerVersion);
  }
  if (source.generatedAt && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(source.generatedAt)) {
    details.push(source.generatedAt);
  }
  if (Number.isSafeInteger(source.freshnessMs) && source.freshnessMs != null && source.freshnessMs >= 0) {
    details.push(`${fmtDuration(source.freshnessMs)} old`);
  }
  if (source.view === "activity" || source.view === "legacy-data" || source.view === "daily-activity") details.push(source.view);
  return details.length > 0 ? details.join(" · ") : null;
}

function digestSourceId(value: string): string {
  return ["life-log", "git", "run-manager", "knowledge-base"].includes(value)
    ? value
    : "unknown source";
}

function digestSourceScope(value: string): string {
  return [
    "live-local",
    "requested-range",
    "requested-range-partial",
    "latest-snapshot-out-of-range",
    "browser-preview-only",
    "unavailable",
  ].includes(value)
    ? value
    : "scope unavailable";
}

function sourceFreshnessState(
  freshnessMs: number | null,
  available: boolean,
  errorCode?: string | null,
): "fresh" | "stale" | "expired" | "unknown" | "error" {
  if (errorCode === "snapshot_stale") return "stale";
  if (errorCode || !available) return errorCode ? "error" : "unknown";
  if (freshnessMs == null || freshnessMs < 0) return "unknown";
  if (freshnessMs <= 120_000) return "fresh";
  if (freshnessMs <= 900_000) return "stale";
  return "expired";
}

function sourceFreshnessLabel(state: ReturnType<typeof sourceFreshnessState>): string {
  return {
    fresh: "fresh",
    stale: "stale",
    expired: "expired",
    unknown: "freshness unknown",
    error: "error",
  }[state];
}

export const FIXED_SOURCE_EXPLANATIONS = {
  snapshot_range_partial: "선택한 기간의 일부 daily snapshot만 일치해 나머지 native 지표는 사용 불가로 표시하며 최신값으로 대체하지 않습니다.",
  snapshot_range_unavailable: "선택한 기간에 일치하는 daily snapshot이 없어 native 지표는 사용 불가로 표시하며 최신값으로 대체하지 않습니다.",
  snapshot_boundary_mismatch: "daily snapshot의 날짜·시간대 경계가 요청 범위와 일치하지 않아 native 지표는 사용 불가로 표시하며 최신값으로 대체하지 않습니다.",
  snapshot_stale: "daily snapshot이 오래되어 native 지표는 사용 불가로 표시하며 최신값으로 대체하지 않습니다.",
} as const;

type DigestSource = DigestResponse["document"]["sources"][number];

type SourceFreshness = ReturnType<typeof sourceFreshnessState>;

function fixedSourceExplanation(
  source: Pick<DigestSource, "scope" | "errorCode" | "available" | "freshnessMs"> & { freshnessState?: SourceFreshness },
): string | null {
  if (source.errorCode && source.errorCode in FIXED_SOURCE_EXPLANATIONS) {
    return FIXED_SOURCE_EXPLANATIONS[source.errorCode as keyof typeof FIXED_SOURCE_EXPLANATIONS];
  }
  if (source.scope === "requested-range-partial") {
    return FIXED_SOURCE_EXPLANATIONS.snapshot_range_partial;
  }
  const freshness = source.freshnessState ?? sourceFreshnessState(source.freshnessMs, source.available, source.errorCode);
  if (freshness === "stale" || freshness === "expired") {
    return FIXED_SOURCE_EXPLANATIONS.snapshot_stale;
  }
  return null;
}

export function digestSourceExplanation(
  source: DigestResponse["document"]["sources"][number],
): string {
  const fixed = fixedSourceExplanation(source);
  if (fixed) return fixed;
  if (source.scope === "browser-preview-only") {
    return "브라우저 미리보기에서는 native DB와 local snapshot을 읽지 않습니다.";
  }
  if (source.id === "life-log") return "Life Log 로컬 DB를 선택한 날짜 범위와 필터로 집계합니다.";
  if (source.id === "git") return "설정된 프로젝트의 read-only Git count를 요청 범위로 제한합니다.";
  if (source.id === "run-manager") return "Run Manager 최신 snapshot은 provenance로만 표시하며 활동 통계에 합치지 않습니다.";
  if (source.id === "knowledge-base") return "Knowledge 최신 snapshot은 provenance로만 표시하며 원문을 읽지 않습니다.";
  return "이 source는 통계에 조용히 합치지 않도록 별도로 표시됩니다.";
}

function digestActivitySourceNotice(document: DigestResponse["document"]): string | null {
  const notices: string[] = [];
  for (const source of document.sources) {
    if (source.id !== "run-manager" && source.id !== "knowledge-base") continue;
    const explanation = fixedSourceExplanation(source);
    if (explanation) {
      notices.push(`${source.id === "run-manager" ? "Run Manager" : "Knowledge"}: ${explanation}`);
    }
  }
  return notices.length > 0 ? notices.join(" ") : null;
}

export function buildExportInput(
  startDate: string,
  endDate: string,
  format: ExportFormat,
): ExportInput | null {
  const start = parseDateKey(startDate);
  const end = parseDateKey(endDate);
  if (!start || !end) return null;
  if (end.getTime() < start.getTime()) return null;

  // Keep each civil-day boundary instead of deriving later days by adding a
  // fixed 24 hours. This preserves local calendar semantics across DST.
  const dayBoundaries: ExportDayBoundary[] = [];
  let cursor = new Date(start.getTime());
  while (cursor.getTime() <= end.getTime()) {
    if (dayBoundaries.length >= 366) return null;
    const date = toDateStr(cursor);
    const dayStart = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate()).getTime();
    const next = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + 1);
    const dayEnd = next.getTime();
    if (dayEnd <= dayStart) return null;
    dayBoundaries.push({ date, startMs: dayStart, endMs: dayEnd });
    cursor = next;
  }
  const first = dayBoundaries[0];
  const last = dayBoundaries[dayBoundaries.length - 1];
  if (!first || !last || last.date !== endDate) return null;
  return {
    startDate,
    endDate,
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone || "local",
    dayStart: first.startMs,
    dayEnd: last.endMs,
    dayBoundaries,
    format,
  };
}

export function DataSourceRow({ source }: { source: SourceStatus }) {
  const diagnostics = [
    source.schemaVersion != null ? `v${source.schemaVersion}` : null,
    source.producerVersion,
    source.freshnessMs != null ? `${fmtDuration(source.freshnessMs)} 전 갱신` : null,
  ].filter((value): value is string => value != null);
  const activity = source.knowledgeActivity;
  const freshness = sourceFreshnessState(source.freshnessMs, source.available, source.errorCode ?? source.error);
  const sourceMetadata: DigestSource = {
    id: source.producer,
    available: source.available,
    schemaVersion: source.schemaVersion,
    snapshotVersion: null,
    producerVersion: source.producerVersion,
    generatedAt: source.generatedAt,
    freshnessMs: source.freshnessMs,
    view: null,
    scope: source.scope ?? "unavailable",
    errorCode: source.errorCode ?? null,
  };
  const fixedExplanation = fixedSourceExplanation({ ...sourceMetadata, freshnessState: source.freshnessState });

  return (
    <div className="git-row source-row">
      <span className="mono">{source.producer}</span>
      <div className="source-details">
        <span className={`freshness-badge freshness-${freshness}`}>{sourceFreshnessLabel(freshness)}</span>
        {diagnostics.length > 0 && <span className="dim">{diagnostics.join(" · ")}</span>}
        <span className="dim">{source.scope ?? "scope unavailable"}</span>
        <span className="source-explanation">{fixedExplanation ?? source.explanation ?? digestSourceExplanation(sourceMetadata)}</span>
        {source.available && activity && (
          <span className="source-activity">
            오늘 작성·수정 {activity.notesModifiedToday}개
            {activity.lastModifiedAtMs != null && ` · 마지막 수정 ${new Date(activity.lastModifiedAtMs).toLocaleString()}`}
            {activity.legacySnapshot && " · 구버전 snapshot"}
            {!activity.identifiersComplete && !activity.legacySnapshot && ` · 식별자 ${activity.identifiedNotes}개만 포함`}
          </span>
        )}
        {!source.available && (
          <span role="alert" className="source-error">
            {source.errorCode ? `${source.errorCode} · ` : ""}{fixedExplanation ? "사용할 수 없음" : source.error ?? "사용할 수 없음"}
          </span>
        )}
      </div>
    </div>
  );
}

export default function App() {
  const [date, setDate] = useState(new Date());
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
  const [projectSaving, setProjectSaving] = useState(false);
  const [projectProbePath, setProjectProbePath] = useState<string | null>(null);
  const [projectProbes, setProjectProbes] = useState<Record<string, ProjectProbe>>({});
  const [idleThreshold, setIdleThresholdState] = useState(300000);
  const [privacy, setPrivacy] = useState<PrivacyRules>({ excludedProcesses: [], excludedTitlePatterns: [], redactTitlePatterns: [], maskAllTitles: false });
  const [autoStart, setAutoStart] = useState<AutostartStatus | null>(null);
  const [sources, setSources] = useState<SourceStatus[]>([]);
  const [draftHistory, setDraftHistory] = useState<KnowledgeDraftHistoryEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [contextDate, setContextDate] = useState<string | null>(null);
  const [contextActionBusy, setContextActionBusy] = useState(false);
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

  const prepareDateContext = useCallback((target: HTMLElement) => {
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
  }, [dateStr, invalidatePendingLoad]);
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
    setContextActionBusy(true);
    setError(null);
    setNotice(null);
    try {
      if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(value);
      setNotice(`${value} 날짜를 복사했습니다.`);
    } catch {
      setError("날짜를 클립보드에 복사하지 못했습니다.");
    } finally {
      setContextActionBusy(false);
    }
  };

  const saveOrDownloadExport = async (
    input: ExportInput,
  ): Promise<{ saved: boolean; preview: boolean }> => {
    if (isTauri()) {
      return { saved: (await saveLifeLog(input)).saved, preview: false };
    }
    const result = await exportLifeLog(input);
    const expectedExtension = input.format === "markdown" ? "md" : input.format;
    const expectedMime = input.format === "markdown"
      ? "text/markdown;charset=utf-8"
      : `${input.format === "json" ? "application/json" : "text/csv"};charset=utf-8`;
    const byteLength = new TextEncoder().encode(result.content).byteLength;
    if (result.origin !== "browser-preview"
        || result.format !== input.format
        || result.extension !== expectedExtension
        || result.mimeType !== expectedMime
        || result.byteLength !== byteLength
        || result.byteLength > 4 * 1024 * 1024) {
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
    setContextActionBusy(true);
    setError(null);
    setNotice(null);
    return request;
  };

  const isCurrentExport = (request: number): boolean => exportRequestRef.current === request;

  const finishExport = (request: number) => {
    if (!isCurrentExport(request)) return;
    exportBusyRef.current = false;
    setContextActionBusy(false);
  };

  const exportNotice = (format: ExportFormat, preview: boolean): string =>
    `${format.toUpperCase()} export를 ${preview ? "브라우저 미리보기로 다운로드" : isTauri() ? "저장" : "다운로드"}했습니다.`;

  const exportFailure = (format: ExportFormat): string =>
    isTauri()
      ? `${format.toUpperCase()} export를 저장하지 못했습니다.`
      : `${format.toUpperCase()} export 미리보기를 다운로드하지 못했습니다.`;

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
      setError("export 날짜 범위가 올바르지 않습니다. 최대 366일까지 선택할 수 있습니다.");
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
    setContextActionBusy(true);
    setError(null);
    setNotice(null);
    return { request, loadRequest: loadRequestRef.current };
  };

  const finishDigestAction = (action: { request: number; loadRequest: number }) => {
    if (digestRequestRef.current !== action.request) return;
    digestBusyRef.current = false;
    setContextActionBusy(false);
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
      if (isCurrentDigestAction(action)) setNotice("현재 digest를 클립보드에 복사했습니다.");
    } catch {
      if (isCurrentDigestAction(action)) setError("digest를 클립보드에 복사하지 못했습니다.");
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
        if (!response.handle) throw new Error("digest handle unavailable");
        const result = await saveDigest(response.handle);
        if (isCurrentDigestAction(action) && result.saved) {
          setNotice("현재 digest를 저장했습니다.");
        }
      } else {
        if (typeof URL.createObjectURL !== "function") throw new Error("download unavailable");
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
        if (isCurrentDigestAction(action)) setNotice("현재 digest를 브라우저 미리보기로 다운로드했습니다.");
      }
    } catch {
      if (isCurrentDigestAction(action)) {
        setError(isTauri() ? "digest를 저장하지 못했습니다." : "digest 미리보기를 다운로드하지 못했습니다.");
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
        await refreshDraftHistory();
        setNotice("Knowledge draft를 미리보기로 보냈습니다. 저장 전 내용을 확인하세요.");
      }
    } catch {
      if (isCurrentDigestAction(action)) {
        setError("Knowledge draft를 보내지 못했습니다. 잠시 후 다시 시도하세요.");
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
      const result = await sendDigestToKnowledge(
        digestInputFromResponse(digest),
        entry.handoffId,
      );
      if (isCurrentDigestAction(action) && result.kind === "knowledge-draft/v1") {
        await refreshDraftHistory();
        setNotice("새 Knowledge draft를 만들었습니다. 이전 handoff와 별도의 ID로 다시 확인하세요.");
      }
    } catch {
      if (isCurrentDigestAction(action)) setError("Knowledge draft를 다시 만들지 못했습니다.");
    } finally {
      finishDigestAction(action);
    }
  };

  // Keep keyboard focus inside the modal and return it to the control that
  // opened the dialog. The busy ref is used instead of a state dependency so
  // a progress update cannot tear down and recreate the focus trap.
  useEffect(() => {
    if (!exportDialogOpen) return;
    exportRestoreFocusRef.current = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const dialog = exportDialogRef.current;
    const focusTask = window.setTimeout(() => exportFirstFieldRef.current?.focus(), 0);
    const onKeyDown = (event: KeyboardEvent) => {
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
      if (exportRestoreFocusRef.current?.isConnected) exportRestoreFocusRef.current.focus();
      exportRestoreFocusRef.current = null;
    };
  }, [exportDialogOpen]);

  const restoreDateContextFocus = (
    request: number,
    target: HTMLElement | null,
    value: string | null,
  ) => {
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
        : Array.from(document.querySelectorAll<HTMLElement>("[data-date]")).find((element) =>
          element.dataset.date === value
          && (triggerClass === null || element.classList.contains(triggerClass)),
        );
      if (
        !replacement
        || (replacement instanceof HTMLButtonElement && replacement.disabled)
        || (replacement instanceof HTMLInputElement && replacement.disabled)
      ) return;
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
      if (privacyRules.status === "fulfilled") setPrivacy(privacyRules.value);
      if (ast.status === "fulfilled") setAutoStart(ast.value);
      if (src.status === "fulfilled") setSources(src.value);
      if (history.status === "fulfilled"
        && appMountedRef.current
        && historyRequestRef.current === historyRequest) {
        setDraftHistory(history.value);
      }
      if ([pr, idle, privacyRules, ast, src, history].some((result) => result.status === "rejected")) {
        setError("일부 Life Log 설정을 불러오지 못했습니다.");
      }
    } catch {
      setError("Life Log 설정을 불러오지 못했습니다.");
    }
  }, []);

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
        setError(view === "week" || view === "month"
          ? `local digest를 불러오지 못했습니다.${code ? ` (${code})` : ""}`
          : `Life Log 데이터를 불러오지 못했습니다.${code ? ` (${code})` : ""}`);
      }
    } finally {
      if (loadRequestRef.current === request) setLoading(false);
    }
  }, [view, date, dateStr, digestAppFilter]);

  useEffect(() => {
    void load();
  }, [load]);

  // Settings are app state, not date/view state. Reloading them on every
  // navigation can race with an acknowledged save and overwrite the
  // authoritative rows with an older request.
  useEffect(() => {
    void loadSettings();
  }, [loadSettings]);

  useEffect(() => {
    if (!isTauri()) return;
    const timer = window.setInterval(() => void refreshDraftHistory(), 30_000);
    return () => window.clearInterval(timer);
  }, [refreshDraftHistory]);

  // 타임라인은 추적 중에는 주기적으로 갱신한다 (세션 자동 반영).
  useEffect(() => {
    if (view !== "timeline") return;
    const id = setInterval(() => void load(), 30_000);
    return () => clearInterval(id);
  }, [view, load]);

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
    setProjectSaving(true);
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
    } finally {
      setProjectSaving(false);
    }
  };

  const removeProject = async (p: string) => {
    if (projectSaving) return;
    const next = projects.filter((x) => x !== p);
    setProjectSaving(true);
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
    } finally {
      setProjectSaving(false);
    }
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
  const maxStatDuration = Math.max(1, ...(stats.map((stat) => stat.duration_ms)));
  const summary = view === "day" ? day : range;
  const maxSummaryDuration = Math.max(1, ...(summary?.app_totals.map((app) => app.duration_ms) ?? []));
  const digestAppOptions = useMemo(() => {
    const options = digest?.document.appTotals.map((app) => app.app) ?? [];
    if (digestAppFilter && !options.includes(digestAppFilter)) options.unshift(digestAppFilter);
    return options;
  }, [digest, digestAppFilter]);

  return (
    <div className="app">
      <header className="toolbar">
        <button type="button" className="btn" aria-label="이전 날짜" onClick={() => shift(-1)} disabled={contextActionBusy}>
          ◀
        </button>
        <input
          type="date"
          className="date-input"
          value={dateStr}
          data-date={dateStr}
          aria-label={`${dateStr} 선택된 날짜`}
          disabled={contextActionBusy}
          onChange={(e) => {
            const parsed = parseDateKey(e.currentTarget.value);
            if (parsed) selectDate(parsed);
          }}
          {...dateContextMenu.triggerProps}
        />
        <button type="button" className="btn" aria-label="다음 날짜" onClick={() => shift(1)} disabled={contextActionBusy}>
          ▶
        </button>
        <button type="button" className="btn" onClick={() => selectDate(new Date())} disabled={contextActionBusy}>
          Today
        </button>
        <span className="spacer" />
        {loading && (
          <>
            <span className="loading" role="status" aria-live="polite">Loading...</span>
            <button type="button" className="btn" onClick={() => void cancelCurrentLoad()} aria-label="데이터 불러오기 취소">
              Cancel
            </button>
          </>
        )}
        {(["day", "week", "month", "timeline", "settings"] as const).map((t) => (
          <button
            type="button"
            key={t}
            className={`btn ${view === t ? "active" : ""}`}
            aria-pressed={view === t}
            onClick={() => selectView(t)}
            disabled={contextActionBusy}
          >
            {t[0].toUpperCase() + t.slice(1)}
          </button>
        ))}
        <button type="button" className="btn refresh" onClick={() => void load()} disabled={contextActionBusy}>
          Refresh
        </button>
        <button type="button" className="btn" onClick={openExportDialog} disabled={contextActionBusy || loading}>
          {isTauri() ? "Export range" : "Export preview"}
        </button>
      </header>

      {error && <div className="error" role="alert">{error}</div>}
      {notice && <div className="notice" role="status" aria-live="polite">{notice}</div>}

      {view === "settings" ? (
        <div className="settings">
          <section className="panel">
            <h2>Data sources</h2>
            {sources.length === 0 && <div className="dim">등록된 source가 없습니다.</div>}
            {sources.map((s) => (
              <DataSourceRow
                key={`${s.producer}:v${s.schemaVersion ?? "unknown"}:${s.available ? "ok" : "error"}`}
                source={s}
              />
            ))}
            <div className="dim">source는 devbox 공용 루트의 read-only snapshot을 통해 읽습니다 (다른 앱의 DB를 직접 읽지 않음).</div>
          </section>

          <section className="panel" aria-label="Knowledge draft handoff history">
            <div className="panel-heading-row">
              <div>
                <h2>Knowledge handoff history</h2>
                <div className="dim">상태와 aggregate summary/source reference만 보존합니다. 활동 원문·경로·credential은 저장하지 않습니다.</div>
              </div>
              <button className="btn small" type="button" onClick={() => void refreshDraftHistory()} disabled={contextActionBusy}>Refresh</button>
            </div>
            {draftHistory.length === 0 ? (
              <div className="dim">아직 보낸 draft가 없습니다.</div>
            ) : draftHistory.map((entry) => (
              <div className="handoff-history-row" key={entry.handoffId}>
                <div className="handoff-history-main">
                  <span className={`handoff-status handoff-status-${entry.status}`}>{entry.status}</span>
                  <strong>{entry.summary.startDate} ~ {entry.summary.endDate}</strong>
                  <span className="dim">{entry.summary.period} · {entry.summary.timezone}</span>
                  <span className="dim">{entry.summary.sessionCount} sessions · {fmtDuration(entry.summary.pcUsageMs)} · {entry.summary.gitCommits} commits</span>
                </div>
                <div className="handoff-history-sources">
                  {entry.sources.map((source) => (
                    <span key={source.id} className={source.available ? "source-ok" : "source-error"}>
                      {source.id} · {digestSourceScope(source.scope)}{source.errorCode ? ` · ${source.errorCode}` : ""}
                    </span>
                  ))}
                </div>
                <button
                  className="btn small"
                  type="button"
                  onClick={() => void regenerateDraft(entry)}
                  disabled={!isTauri() || !digest || contextActionBusy || loading}
                >
                  Regenerate
                </button>
              </div>
            ))}
          </section>

          <section className="panel">
            <h2>Git project paths</h2>
            {projects.map((p) => (
              <div key={p} className="git-row">
                <div className="git-project-details">
                  <span className="mono">{p}</span>
                  {projectProbes[p] && (
                    <span className={projectProbes[p].repository ? "project-probe-ok" : "project-probe-error"}>
                      {projectProbes[p].repository
                        ? `Git 저장소 확인됨 · ${projectProbes[p].target === "wsl" ? "WSL" : "Windows"}`
                        : `확인 실패 · ${projectProbes[p].errorCode ?? "git_failed"}`}
                    </span>
                  )}
                </div>
                <div className="git-project-actions">
                  <button className="mini" onClick={() => void checkProject(p)} disabled={projectProbePath !== null || projectSaving}>
                    {projectProbePath === p ? "확인 중…" : "연결 확인"}
                  </button>
                  <button className="mini" onClick={() => void removeProject(p)} disabled={projectSaving} aria-label={`${p} 제거`}>
                    ✕
                  </button>
                </div>
              </div>
            ))}
            <div className="row">
              <input placeholder="C:\projects\devbox 또는 \\wsl$\Ubuntu\home\user\project" value={projectInput} onChange={(e) => setProjectInput(e.currentTarget.value)} onKeyDown={(e) => {
                if (e.key === "Enter") void addProject();
              }} disabled={projectSaving} />
              <button className="btn" onClick={() => void addProject()} disabled={projectSaving || !projectInput.trim()}>
                {projectSaving ? "저장 중…" : "추가"}
              </button>
            </div>
            <div className="dim">연결 확인은 중지된 WSL 배포판을 시작할 수 있습니다. 경로 저장만으로는 배포판을 시작하지 않습니다.</div>
            <div className="dim">활동 추적은 Life Log에 통합되어 있으며, 세션은 자동으로 기록됩니다.</div>
          </section>

          <section className="panel">
            <h2>Idle detection</h2>
            <div className="row">
              <span className="dim">자리를 비운 지 (분):</span>
              <input
                type="number"
                min={1}
                value={Math.round(idleThreshold / 60000)}
                onChange={(e) => {
                  const minutes = Number(e.currentTarget.value);
                  if (Number.isFinite(minutes) && minutes >= 1) {
                    setIdleThresholdState(minutes * 60000);
                    void setIdleThreshold(minutes * 60000);
                  }
                }}
              />
            </div>
            <div className="dim">이 시간 이상 입력이 없으면 해당 구간을 사용 시간에서 제외합니다.</div>
          </section>

          <section className="panel">
            <h2>Auto start</h2>
            {autoStart?.supported ? (
              <label className="row">
                <input
                  type="checkbox"
                  checked={autoStart.enabled}
                  onChange={(e) => {
                    setError(null);
                    setNotice(null);
                    void (async () => {
                      try {
                        const next = await setAutostart(e.currentTarget.checked);
                        setAutoStart(next);
                      } catch (err) {
                        setError(err instanceof Error ? err.message : String(err));
                      }
                    })();
                  }}
                />
                Windows 로그인 시 자동 시작
              </label>
            ) : (
              <div className="dim">이 플랫폼에서는 자동 시작을 지원하지 않습니다.</div>
            )}
          </section>

          <section className="panel">
            <h2>Privacy rules</h2>
            <div className="privacy-row">
              <span className="dim">제외할 프로세스 (쉼표 구분, 정확 일치):</span>
              <input
                value={privacy.excludedProcesses.join(", ")}
                onChange={(e) => {
                  const next = { ...privacy, excludedProcesses: e.currentTarget.value.split(",").map((s) => s.trim()).filter(Boolean) };
                  setPrivacy(next);
                  void setPrivacyRules(next);
                }}
              />
            </div>
            <div className="privacy-row">
              <span className="dim">제목 미저장 정규식 (쉼표 구분):</span>
              <input
                value={privacy.excludedTitlePatterns.join(", ")}
                onChange={(e) => {
                  const next = { ...privacy, excludedTitlePatterns: e.currentTarget.value.split(",").map((s) => s.trim()).filter(Boolean) };
                  setPrivacy(next);
                  void setPrivacyRules(next);
                }}
              />
            </div>
            <div className="privacy-row">
              <span className="dim">제목 치환 정규식 → [redacted] (쉼표 구분):</span>
              <input
                value={privacy.redactTitlePatterns.join(", ")}
                onChange={(e) => {
                  const next = { ...privacy, redactTitlePatterns: e.currentTarget.value.split(",").map((s) => s.trim()).filter(Boolean) };
                  setPrivacy(next);
                  void setPrivacyRules(next);
                }}
              />
            </div>
            <label className="row">
              <input type="checkbox" checked={privacy.maskAllTitles} onChange={(e) => {
                const next = { ...privacy, maskAllTitles: e.currentTarget.checked };
                setPrivacy(next);
                void setPrivacyRules(next);
              }} />
              모든 제목을 저장하지 않음
            </label>
            <div className="row">
              <button className="btn" onClick={() => void (async () => {
                setError(null);
                try {
                  const n = await redactExisting();
                  setNotice(`기존 세션 ${n}개에 규칙을 적용했습니다.`);
                } catch (e) {
                  setError(e instanceof Error ? e.message : String(e));
                }
              })()}>
                기존 세션에 적용
              </button>
            </div>
            <div className="dim">규칙은 DB 저장 전에 적용됩니다. 제외한 원문은 어디에도 남지 않습니다.</div>
          </section>
        </div>
      ) : view === "timeline" ? (
        <div className="timeline">
          <div className="timeline-head">
            <span className={tracking ? "status-on" : "status-off"}>● {tracking ? "Tracking" : "Stopped"}</span>
            <button className={`btn ${tracking ? "danger" : ""}`} onClick={() => void toggleTracking()}>
              {tracking ? "Stop" : "Start"} tracking
            </button>
            <span className="dim">Total: {fmtDuration(sessions.reduce((acc, s) => acc + s.duration_ms, 0))}</span>
          </div>
          {sessions.map((s) => (
            <div key={s.id} className="session">
              <span className="time">{fmtTime(s.start_ts)}</span>
              <span className="app">{shortApp(s.app)}</span>
              <span className="title dim">{s.title || "-"}</span>
              <span className="dur dim">{fmtDuration(s.duration_ms)}</span>
            </div>
          ))}
          {sessions.length === 0 && <div className="empty">No activity recorded this day</div>}

          {stats.length > 0 && (
            <section className="panel">
              <h2>App usage</h2>
              {stats.map((a) => (
                <div key={a.app} className="stat-row">
                  <span className="stat-app">{shortApp(a.app)}</span>
                  <div className="stat-bar">
                    <div className="stat-fill" style={{ width: `${Math.min(100, (a.duration_ms / maxStatDuration) * 100)}%` }} />
                  </div>
                  <span className="stat-dur">{fmtDuration(a.duration_ms)}</span>
                  <span className="dim">{a.sessions} sessions</span>
                </div>
              ))}
            </section>
          )}
        </div>
      ) : (
        <div className="day">
          {summary && (
            <>
              <div className="cards">
                <div className="card">
                  <div className="card-label">PC 사용</div>
                  <div className="card-value">{fmtDuration(summary.pc_usage_ms)}</div>
                </div>
                <div className="card">
                  <div className="card-label">Git commits · 기간 전체</div>
                  <div className="card-value">{summary.git.total_commits}</div>
                </div>
                <div className="card">
                  <div className="card-label">Most active</div>
                  <div className="card-value">{topApp ? shortApp(topApp.app) : summary.app_totals[0] ? shortApp(summary.app_totals[0].app) : "-"}</div>
                </div>
              </div>

              {(view === "day" || view === "week" || view === "month") && (
                <section className="panel digest-panel" aria-busy={loading || contextActionBusy}>
                  <div className="digest-heading">
                    <div>
                      <h2>{view === "day" ? "Daily local digest" : view === "month" ? "Monthly local digest" : "Weekly local digest"}</h2>
                      <p className="dim">결정론적 규칙으로만 계산하며 네트워크·AI·외부 전송을 사용하지 않습니다.</p>
                    </div>
                    <div className="digest-actions">
                      {loading && (
                        <button
                          type="button"
                          className="btn"
                          onClick={() => void cancelCurrentLoad()}
                          aria-label="digest 불러오기 취소"
                        >
                          Cancel
                        </button>
                      )}
                      <button
                        type="button"
                        className="btn"
                        onClick={() => void copyDigest()}
                        disabled={!digest || contextActionBusy || loading}
                      >
                        Copy digest
                      </button>
                      <button
                        type="button"
                        className="btn active"
                        onClick={() => void downloadDigest()}
                        disabled={!digest || contextActionBusy || loading}
                      >
                        {isTauri() ? "Save digest" : "Download preview"}
                      </button>
                      <button
                        type="button"
                        className="btn"
                        onClick={() => void sendDigestDraft()}
                        disabled={!isTauri() || !digest || contextActionBusy || loading}
                        title={isTauri() ? undefined : "Knowledge handoff는 native 데스크톱에서만 사용할 수 있습니다"}
                      >
                        Send to Knowledge
                      </button>
                    </div>
                  </div>
                  {digest ? (
                    <>
                      <div className="digest-toolbar">
                        <label htmlFor="life-log-digest-app-filter">
                          Application filter
                          <select
                            id="life-log-digest-app-filter"
                            value={digestAppFilter ?? ""}
                            onChange={(event) => selectDigestFilter(event.currentTarget.value || null)}
                            disabled={contextActionBusy || loading}
                          >
                            <option value="">All applications</option>
                            {digestAppOptions.map((app) => <option key={app} value={app}>{shortApp(app)}</option>)}
                          </select>
                        </label>
                        <span className="dim scope-note" role="status" aria-live="polite">
                          {digest.origin === "browser-preview"
                            ? "Browser preview only · native local data unavailable · "
                            : "Native local digest · "}
                          {digest.document.range.startDate} ~ {digest.document.range.endDate} · {digest.document.range.timezone} · Git commits use the full requested period and ignore this app filter.
                        </span>
                      </div>
                      {digestActivitySourceNotice(digest.document) && (
                        <p className="activity-source-notice" role="note">
                          {digestActivitySourceNotice(digest.document)}
                        </p>
                      )}
                      <div className="digest-cards">
                        <div className="card">
                          <div className="card-label">PC 사용</div>
                          <div className="card-value">{fmtDuration(digest.document.summary.pcUsageMs)}</div>
                        </div>
                        <div className="card">
                          <div className="card-label">활동일</div>
                          <div className="card-value">{digest.document.summary.activeDays}/{digest.document.summary.totalDays}</div>
                        </div>
                        <div className="card">
                          <div className="card-label">Sessions</div>
                          <div className="card-value">{digest.document.summary.sessionCount}</div>
                        </div>
                        <div className="card">
                          <div className="card-label">Git commits · 기간 전체</div>
                          <div className="card-value">{digest.document.summary.gitCommits}</div>
                        </div>
                        <div className="card activity-card" data-testid="run-summary">
                          <div className="card-label">Run Manager · 기간 전체</div>
                          <div className="card-value">{formatRunSummary(digest.document.summary.run)}</div>
                          <div className="dim">last run {formatNullableTimestamp(digest.document.summary.run?.lastRunAtMs ?? null)}</div>
                        </div>
                        <div className="card activity-card" data-testid="knowledge-summary">
                          <div className="card-label">Knowledge notes · 기간 전체</div>
                          <div className="card-value">{formatKnowledgeSummary(digest.document.summary.knowledge)}</div>
                          <div className="dim">last modified {formatNullableTimestamp(digest.document.summary.knowledge?.lastModifiedAtMs ?? null)}</div>
                        </div>
                      </div>
                      <p className="digest-headline">{digest.document.headline}</p>
                      {digest.document.summary.activeDays === 0 && digest.document.summary.gitCommits === 0 && (
                        <div className="empty">선택한 기간과 필터에 기록된 활동이 없습니다.</div>
                      )}
                      <div className="digest-days" aria-label="일별 digest">
                        {digest.document.daily.map((day) => (
                          <div key={day.date} className={`digest-day ${day.hasActivity ? "" : "empty-day"}`}>
                            <span className="mono">{day.date}</span>
                            <span>{fmtDuration(day.pcUsageMs)}</span>
                            <span className="dim">{day.sessionCount} sessions · {day.gitCommits} commits</span>
                            <span className="dim">{day.topApp ? shortApp(day.topApp) : "-"}</span>
                            <span className="dim daily-activity">{formatDailyActivity(day)}</span>
                          </div>
                        ))}
                      </div>
                      <details className="digest-details">
                        <summary>Sources and aggregation rules</summary>
                        <div className="digest-source-list">
                          {digest.document.sources.map((source) => (
                            <div key={`${digestSourceId(source.id)}:${digestSourceScope(source.scope)}`} className="git-row">
                              <span className="mono">{digestSourceId(source.id)}</span>
                              <span className="source-details">
                                <span className={source.available ? "source-ok" : "source-error"}>
                                  {source.available ? "available" : source.errorCode === "browser_preview_only" ? "browser preview only" : "unavailable"}
                                  {` · ${digestSourceScope(source.scope)}`}
                                </span>
                                <span className={`freshness-badge freshness-${sourceFreshnessState(source.freshnessMs, source.available, source.errorCode)}`}>
                                  {sourceFreshnessLabel(sourceFreshnessState(source.freshnessMs, source.available, source.errorCode))}
                                </span>
                                <span className="source-explanation">{digestSourceExplanation(source)}</span>
                                {source.errorCode && <span className="source-error">error code: {source.errorCode}</span>}
                                {digestSourceDetails(source) && (
                                  <span className="dim">{digestSourceDetails(source)}</span>
                                )}
                              </span>
                            </div>
                          ))}
                        </div>
                        <div className="digest-rules">
                          {Object.entries(digest.document.rules).map(([name, rule]) => (
                            <div key={name} className="digest-rule"><span>{name}</span><span className="dim">{rule}</span></div>
                          ))}
                        </div>
                      </details>
                    </>
                  ) : (
                    <div className="empty">Digest를 준비하는 중입니다…</div>
                  )}
                </section>
              )}

              {view !== "day" && range && (
                <section className="panel">
                  <h2>{range.label} — daily usage</h2>
                  <div className="daily-chart">
                    {range.daily.map((p, index) => {
                      const pointDate = new Date(p.day_ms);
                      const pointDateStr = toDateStr(pointDate);
                      return (
                        <button
                          key={p.day_ms}
                          type="button"
                          className="daily-col"
                          title={`${fmtDay(p.day_ms)}: ${fmtDuration(p.pc_usage_ms)}`}
                          data-date={pointDateStr}
                          aria-label={`${pointDateStr} 날짜`}
                          aria-current={pointDateStr === dateStr ? "date" : undefined}
                          aria-roledescription="일별 사용량"
                          tabIndex={pointDateStr === dateStr ? 0 : -1}
                          ref={(element) => { dailyChartRefs.current[index] = element; }}
                          onKeyDown={(event) => onDailyChartKeyDown(event, index, range.daily)}
                          onClick={() => selectDate(pointDate)}
                          {...dateContextMenu.triggerProps}
                        >
                          <div className="daily-bar" style={{ height: `${Math.max(2, (p.pc_usage_ms / maxDaily) * 100)}%` }} />
                          <div className="daily-label">{fmtDay(p.day_ms)}</div>
                        </button>
                      );
                    })}
                    {range.daily.length === 0 && <div className="empty">No activity in this period</div>}
                  </div>
                </section>
              )}

              {summary.app_totals.length > 0 && (
                <section className="panel">
                  <h2>App usage</h2>
                  {summary.app_totals.map((a) => (
                    <div key={a.app} className="stat-row">
                      <span className="stat-app">{shortApp(a.app)}</span>
                      <div className="stat-bar">
                        <div className="stat-fill" style={{ width: `${Math.min(100, (a.duration_ms / maxSummaryDuration) * 100)}%` }} />
                      </div>
                      <span className="stat-dur">{fmtDuration(a.duration_ms)}</span>
                    </div>
                  ))}
                </section>
              )}

              {attribution && attribution.profileCount > 0 && (
                <section className="panel">
                  <h2>Project attribution · all applications</h2>
                  {attribution.attributed.map((a) => (
                    <div key={a.projectId} className="git-row">
                      <span className="mono dim">{a.projectId}</span>
                      <span className="git-count">{a.sessions} sessions · {fmtDuration(a.durationMs)}</span>
                    </div>
                  ))}
                  {attribution.unattributed.sessions > 0 && (
                    <div className="git-row">
                      <span className="dim">미귀속</span>
                      <span className="git-count">{attribution.unattributed.sessions} sessions · {fmtDuration(attribution.unattributed.durationMs)}</span>
                    </div>
                  )}
                  <div className="dim">귀속은 창 제목의 프로젝트 이름 매치 기준이며 application filter와 독립적입니다 (가장 긴 이름 우선, 중복 집계 없음).</div>
                </section>
              )}

              {summary.git.projects.length > 0 && (
                <section className="panel">
                  <h2>Git</h2>
                  {summary.git.projects.map((p) => (
                    <div key={p.path} className="git-row">
                      <span className="mono dim">{p.path}</span>
                      <span className="git-count">{p.commits} commits</span>
                    </div>
                  ))}
                </section>
              )}
            </>
          )}
        </div>
      )}
      {exportDialogOpen && (
        <div className="export-backdrop" role="presentation">
          <section
            className="export-dialog"
            ref={exportDialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="life-log-export-title"
            aria-describedby="life-log-export-description"
            aria-busy={contextActionBusy}
            tabIndex={-1}
          >
            <h2 id="life-log-export-title">Life Log export</h2>
            <p id="life-log-export-description" className="dim">
              {isTauri()
                ? "선택한 기간의 활동·Git·검증된 local source 요약을 파일로 저장합니다."
                : "브라우저 미리보기는 로컬 DB·Git·snapshot을 포함하지 않습니다."}
            </p>
            <div className="export-fields">
              <label htmlFor="life-log-export-start">
                시작 날짜
                <input id="life-log-export-start" ref={exportFirstFieldRef} type="date" value={exportStartDate} onChange={(event) => setExportStartDate(event.currentTarget.value)} disabled={contextActionBusy} />
              </label>
              <label htmlFor="life-log-export-end">
                종료 날짜
                <input id="life-log-export-end" type="date" value={exportEndDate} onChange={(event) => setExportEndDate(event.currentTarget.value)} disabled={contextActionBusy} />
              </label>
              <label htmlFor="life-log-export-format">
                형식
                <select id="life-log-export-format" value={exportFormat} onChange={(event) => setExportFormat(event.currentTarget.value as ExportFormat)} disabled={contextActionBusy}>
                  <option value="markdown">Markdown</option>
                  <option value="json">JSON</option>
                  <option value="csv">CSV</option>
                </select>
              </label>
            </div>
            <div className="export-actions">
              <button type="button" className="btn" onClick={() => setExportDialogOpen(false)} disabled={contextActionBusy}>취소</button>
              <button type="button" className="btn active" onClick={() => void submitRangeExport()} disabled={contextActionBusy}>
                {contextActionBusy ? "Exporting..." : isTauri() ? "저장" : "미리보기 다운로드"}
              </button>
            </div>
          </section>
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
