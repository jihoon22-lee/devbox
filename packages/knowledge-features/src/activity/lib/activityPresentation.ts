import {
  type ExportDayBoundary,
  type ExportInput,
  type ExportFormat,
  type DigestInput,
  type DigestDay,
  type DigestPeriod,
  type DigestResponse,
  type KnowledgeDigest,
  type RunDigest,
} from "../api";
import { parseDateKey } from "./contextMenu";
import type { DaySummary, RangeSummary } from "../types";

export function fmtDuration(ms: number): string {
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h}시간 ${m}분` : `${m}분`;
}

export function shortApp(app: string): string {
  return Array.from(app.replace(/\.exe$/i, ""))
    .slice(0, 22)
    .join("");
}

export function safeNativeErrorCode(error: unknown): string | null {
  const value = error instanceof Error ? error.message : String(error ?? "");
  return /^[a-z][a-z0-9_]{0,63}$/.test(value) ? value : null;
}

export function toDateStr(d: Date): string {
  return `${String(d.getFullYear()).padStart(4, "0")}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

export function fmtDay(dayMs: number): string {
  const d = new Date(dayMs);
  return `${d.getMonth() + 1}/${d.getDate()}`;
}

export function fmtTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function formatNullableCount(value: number | null): string {
  return value == null ? "—" : value.toLocaleString();
}

export function formatNullableTimestamp(value: number | null): string {
  return value == null ? "—" : new Date(value).toLocaleString();
}

export function formatRunSummary(run: RunDigest | null): string {
  return run == null ? "—" : `${formatNullableCount(run.succeeded)}건 성공 · ${formatNullableCount(run.failed)}건 실패`;
}

export function formatKnowledgeSummary(knowledge: KnowledgeDigest | null): string {
  return knowledge == null ? "—" : `${formatNullableCount(knowledge.notesModified)}건 수정`;
}

export function formatDailyActivity(
  day: Pick<DigestDay, "runSucceeded" | "runFailed" | "knowledgeNotesModified">,
): string {
  const runs =
    day.runSucceeded == null || day.runFailed == null
      ? "Run Manager —"
      : `Run Manager ${formatNullableCount(day.runSucceeded)}/${formatNullableCount(day.runFailed)}`;
  const notes =
    day.knowledgeNotesModified == null
      ? "Knowledge —"
      : `Knowledge ${formatNullableCount(day.knowledgeNotesModified)}건`;
  return `${runs} · ${notes}`;
}

export type ViewTab = "day" | "week" | "month" | "timeline" | "settings";

export const VIEW_LABELS: Record<ViewTab, string> = {
  day: "일",
  week: "주",
  month: "월",
  timeline: "타임라인",
  settings: "설정",
};

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

export function periodDateKeys(date: Date, period: DigestPeriod): { startDate: string; endDate: string } {
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

export function buildDigestInput(date: Date, period: DigestPeriod, app: string | null = null): DigestInput | null {
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

export function rangeFromDigest(response: DigestResponse, label: string): RangeSummary {
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
        error_code: project.errorCode,
        projectAssociation: response.projectAssociations?.[project.path],
      })),
      total_commits: response.document.git.totalCommits,
    },
    daily: response.document.daily.map((day) => ({
      day_ms: day.startMs,
      pc_usage_ms: day.pcUsageMs,
    })),
  };
}

export function dayFromDigest(response: DigestResponse): DaySummary {
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
        error_code: project.errorCode,
        projectAssociation: response.projectAssociations?.[project.path],
      })),
      total_commits: response.document.git.totalCommits,
    },
  };
}

export function digestSourceDetails(source: DigestResponse["document"]["sources"][number]): string | null {
  const details: string[] = [];
  if (Number.isSafeInteger(source.schemaVersion) && source.schemaVersion != null && source.schemaVersion > 0) {
    details.push(`schema v${source.schemaVersion}`);
  }
  if (Number.isSafeInteger(source.snapshotVersion) && source.snapshotVersion != null && source.snapshotVersion > 0) {
    details.push(`snapshot v${source.snapshotVersion}`);
  }
  if (
    source.producerVersion &&
    /^[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?(?:\+[A-Za-z0-9.-]+)?$/.test(source.producerVersion)
  ) {
    details.push(source.producerVersion);
  }
  if (source.generatedAt && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(source.generatedAt)) {
    details.push(source.generatedAt);
  }
  if (Number.isSafeInteger(source.freshnessMs) && source.freshnessMs != null && source.freshnessMs >= 0) {
    details.push(`${fmtDuration(source.freshnessMs)} 전`);
  }
  if (source.view === "activity" || source.view === "legacy-data" || source.view === "daily-activity")
    details.push(source.view);
  return details.length > 0 ? details.join(" · ") : null;
}

export function digestSourceId(value: string): string {
  return ["life-log", "git", "run-manager", "knowledge-base"].includes(value) ? value : "알 수 없는 소스";
}

export function digestSourceScope(value: string): string {
  return [
    "live-local",
    "requested-range",
    "requested-range-partial",
    "latest-snapshot-out-of-range",
    "browser-preview-only",
    "unavailable",
  ].includes(value)
    ? value
    : "범위 없음";
}

export function sourceFreshnessState(
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

export function sourceFreshnessLabel(state: ReturnType<typeof sourceFreshnessState>): string {
  return {
    fresh: "최신",
    stale: "오래됨",
    expired: "만료됨",
    unknown: "최신 여부 알 수 없음",
    error: "오류",
  }[state];
}

export const FIXED_SOURCE_EXPLANATIONS = {
  snapshot_range_partial:
    "선택한 기간의 일부 daily snapshot만 일치해 나머지 native 지표는 사용 불가로 표시하며 최신값으로 대체하지 않습니다.",
  snapshot_range_unavailable:
    "선택한 기간에 일치하는 daily snapshot이 없어 native 지표는 사용 불가로 표시하며 최신값으로 대체하지 않습니다.",
  snapshot_boundary_mismatch:
    "daily snapshot의 날짜·시간대 경계가 요청 범위와 일치하지 않아 native 지표는 사용 불가로 표시하며 최신값으로 대체하지 않습니다.",
  snapshot_stale: "daily snapshot이 오래되어 native 지표는 사용 불가로 표시하며 최신값으로 대체하지 않습니다.",
} as const;

export type DigestSource = DigestResponse["document"]["sources"][number];

export type SourceFreshness = ReturnType<typeof sourceFreshnessState>;

export function fixedSourceExplanation(
  source: Pick<DigestSource, "scope" | "errorCode" | "available" | "freshnessMs"> & {
    freshnessState?: SourceFreshness;
  },
): string | null {
  if (source.errorCode && source.errorCode in FIXED_SOURCE_EXPLANATIONS) {
    return FIXED_SOURCE_EXPLANATIONS[source.errorCode as keyof typeof FIXED_SOURCE_EXPLANATIONS];
  }
  if (source.scope === "requested-range-partial") {
    return FIXED_SOURCE_EXPLANATIONS.snapshot_range_partial;
  }
  const freshness =
    source.freshnessState ?? sourceFreshnessState(source.freshnessMs, source.available, source.errorCode);
  if (freshness === "stale" || freshness === "expired") {
    return FIXED_SOURCE_EXPLANATIONS.snapshot_stale;
  }
  return null;
}

export function digestSourceExplanation(source: DigestResponse["document"]["sources"][number]): string {
  const fixed = fixedSourceExplanation(source);
  if (fixed) return fixed;
  if (source.scope === "browser-preview-only") {
    return "브라우저 미리보기에서는 native DB와 local snapshot을 읽지 않습니다.";
  }
  if (source.id === "life-log") return "Life Log 로컬 DB를 선택한 날짜 범위와 필터로 집계합니다.";
  if (source.id === "git") return "설정된 프로젝트의 read-only Git count를 요청 범위로 제한합니다.";
  if (source.id === "run-manager")
    return "Run Manager 최신 snapshot은 provenance로만 표시하며 활동 통계에 합치지 않습니다.";
  if (source.id === "knowledge-base") return "Knowledge 최신 snapshot은 provenance로만 표시하며 원문을 읽지 않습니다.";
  return "이 source는 통계에 조용히 합치지 않도록 별도로 표시됩니다.";
}

export function digestActivitySourceNotice(document: DigestResponse["document"]): string | null {
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

export function buildExportInput(startDate: string, endDate: string, format: ExportFormat): ExportInput | null {
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
