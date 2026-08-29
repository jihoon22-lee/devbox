import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { AppTotal, DaySummary, RangeSummary, Session } from "./types";

export type ExportFormat = "markdown" | "json" | "csv";
export type ExportOrigin = "native" | "browser-preview";

export interface ExportDayBoundary {
  date: string;
  startMs: number;
  endMs: number;
}

export interface ExportInput {
  startDate: string;
  endDate: string;
  timezone: string;
  dayStart: number;
  dayEnd: number;
  dayBoundaries: ExportDayBoundary[];
  format: ExportFormat;
}

export interface RenderedExport {
  origin: ExportOrigin;
  format: ExportFormat;
  extension: string;
  mimeType: string;
  byteLength: number;
  content: string;
}

export interface SaveExportResult {
  saved: boolean;
  format: ExportFormat;
  byteLength: number;
}

export type DigestPeriod = "day" | "week" | "month";

export interface DigestFilter {
  app: string | null;
}

export interface DigestInput {
  startDate: string;
  endDate: string;
  timezone: string;
  dayStart: number;
  dayEnd: number;
  dayBoundaries: ExportDayBoundary[];
  period: DigestPeriod;
  filter: DigestFilter;
}

export interface DigestRules {
  sessionWindow: string;
  sessionDuration: string;
  dailyBuckets: string;
  appFilter: string;
  appTotals: string;
  gitCommits: string;
  snapshotScope: string;
  privacy: string;
  externalProcessing: string;
}

export interface DigestDay {
  date: string;
  startMs: number;
  endMs: number;
  pcUsageMs: number;
  sessionCount: number;
  gitCommits: number;
  topApp: string | null;
  hasActivity: boolean;
}

export interface DigestSummary {
  pcUsageMs: number;
  sessionCount: number;
  activeDays: number;
  totalDays: number;
  averageDailyUsageMs: number;
  topApp: string | null;
  gitCommits: number;
}

export interface DigestDocument {
  schemaVersion: number;
  period: DigestPeriod;
  range: {
    startDate: string;
    endDate: string;
    timezone: string;
    startMs: number;
    endMs: number;
    dayBoundaries: ExportDayBoundary[];
  };
  filter: DigestFilter;
  rules: DigestRules;
  headline: string;
  summary: DigestSummary;
  daily: DigestDay[];
  appTotals: Array<{
    app: string;
    durationMs: number;
    sessions: number;
  }>;
  git: {
    projects: Array<{
      path: string;
      commits: number;
      errorCode: string | null;
    }>;
    totalCommits: number;
    errorCodes: string[];
  };
  sources: Array<{
    id: string;
    available: boolean;
    schemaVersion: number | null;
    snapshotVersion: number | null;
    producerVersion: string | null;
    generatedAt: string | null;
    freshnessMs: number | null;
    view: string | null;
    scope: string;
    errorCode: string | null;
  }>;
}

export interface DigestResponse {
  origin: ExportOrigin;
  document: DigestDocument;
  markdown: string;
  /** Native responses expose a short-lived server-owned save handle. */
  handle?: string | null;
}

export interface SaveDigestResult {
  saved: boolean;
  byteLength: number;
}

export interface SendKnowledgeDraftResult {
  id: string;
  kind: "knowledge-draft/v1";
  expiresAtMs: number;
  historyId: string;
}

export type DraftHandoffStatus = "pending" | "sent" | "consumed" | "expired";

export interface KnowledgeDraftSummary {
  period: "day" | "week" | "month";
  startDate: string;
  endDate: string;
  timezone: string;
  filter: string | null;
  pcUsageMs: number;
  sessionCount: number;
  activeDays: number;
  totalDays: number;
  averageDailyUsageMs: number;
  gitCommits: number;
  topApp: string | null;
}

export interface KnowledgeDraftHistoryEntry {
  handoffId: string;
  kind: "knowledge-draft/v1";
  status: DraftHandoffStatus;
  summary: KnowledgeDraftSummary;
  sources: DigestDocument["sources"];
  createdAtMs: number;
  updatedAtMs: number;
  expiresAtMs: number;
  regeneratedFrom: string | null;
}

const DAY_MS = 86_400_000;
const MIN_CIVIL_DAY_MS = DAY_MS - 3_600_000;
const MAX_CIVIL_DAY_MS = DAY_MS + 3_600_000;
const MAX_EXPORT_DAYS = 366;
const MAX_EXPORT_BYTES = 4 * 1024 * 1024;
const EXPORT_CSV_HEADER = "record_type,date,range_start_date,range_end_date,id,app,title,start_ts_ms,end_ts_ms,duration_ms,project_path,commits,metric,value,source,available,schema_version,snapshot_version,producer_version,generated_at,freshness_ms,view,scope,error_code";
const EXPORT_INPUT_KEYS = [
  "dayBoundaries",
  "dayEnd",
  "dayStart",
  "endDate",
  "format",
  "startDate",
  "timezone",
] as const;
const DAY_BOUNDARY_KEYS = ["date", "endMs", "startMs"] as const;
const DIGEST_INPUT_KEYS = [
  "dayBoundaries",
  "dayEnd",
  "dayStart",
  "endDate",
  "filter",
  "period",
  "startDate",
  "timezone",
] as const;
const DIGEST_FILTER_KEYS = ["app"] as const;

function hasExactKeys(value: object, expected: readonly string[]): boolean {
  const actual = Object.keys(value).sort();
  const sortedExpected = [...expected].sort();
  return actual.length === sortedExpected.length
    && actual.every((key, index) => key === sortedExpected[index]);
}

function parseLocalDateKey(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (year < 1) return null;
  const date = new Date(0);
  date.setHours(0, 0, 0, 0);
  date.setFullYear(year, month - 1, day);
  return date.getFullYear() === year
    && date.getMonth() === month - 1
    && date.getDate() === day
    ? date
    : null;
}

function nextLocalDateKey(value: string): string {
  const date = parseLocalDateKey(value)!;
  date.setDate(date.getDate() + 1);
  return `${String(date.getFullYear()).padStart(4, "0")}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function isMondayDateKey(value: string): boolean {
  return parseLocalDateKey(value)?.getDay() === 1;
}

function isMonthEndDateKey(value: string): boolean {
  const date = parseLocalDateKey(value);
  if (!date) return false;
  const nextMonth = new Date(0);
  nextMonth.setHours(0, 0, 0, 0);
  nextMonth.setFullYear(date.getFullYear(), date.getMonth() + 1, 0);
  return date.getDate() === nextMonth.getDate();
}

function isCivilDaySpan(value: number): boolean {
  return value === MIN_CIVIL_DAY_MS || value === DAY_MS || value === MAX_CIVIL_DAY_MS;
}

function isControlCharacter(character: string): boolean {
  const code = character.charCodeAt(0);
  return code < 0x20 || (code >= 0x7f && code <= 0x9f);
}

const MOCK_SESSIONS: Session[] = [
  { id: 1, app: "chrome.exe", title: "GitHub", start_ts: new Date(2026, 7, 10, 9, 22).getTime(), end_ts: new Date(2026, 7, 10, 9, 41).getTime(), duration_ms: 1140000 },
  { id: 2, app: "Code.exe", title: "FamilyCard", start_ts: new Date(2026, 7, 10, 9, 41).getTime(), end_ts: new Date(2026, 7, 10, 10, 8).getTime(), duration_ms: 1620000 },
  { id: 3, app: "WindowsTerminal.exe", title: "Ubuntu", start_ts: new Date(2026, 7, 10, 10, 8).getTime(), end_ts: new Date(2026, 7, 10, 10, 42).getTime(), duration_ms: 2040000 },
];

const MOCK_STATS: AppTotal[] = [
  { app: "Code.exe", duration_ms: 10080000, sessions: 8 },
  { app: "chrome.exe", duration_ms: 7980000, sessions: 12 },
  { app: "WindowsTerminal.exe", duration_ms: 5040000, sessions: 6 },
];

function mockDay(date: string): DaySummary {
  return {
    date,
    pc_usage_ms: 7 * 3600_000 + 21 * 60_000,
    app_totals: [
      { app: "Code.exe", duration_ms: 3 * 3600_000 + 42 * 60_000, sessions: 6 },
      { app: "chrome.exe", duration_ms: 2 * 3600_000 + 13 * 60_000, sessions: 9 },
      { app: "WindowsTerminal.exe", duration_ms: 1 * 3600_000 + 24 * 60_000, sessions: 4 },
    ],
    git: {
      projects: [{ path: "C:\\projects\\devbox", commits: 14 }],
      total_commits: 14,
    },
  };
}

function todayStr(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

// 지난 날짜는 변경될 일이 없으므로 세션 동안 캐시한다. 오늘은 항상 새로 조회.
const dayCache = new Map<string, DaySummary>();

export async function getDay(date: string, dayStart: number, dayEnd: number): Promise<DaySummary> {
  if (!isTauri()) return mockDay(date);
  if (date < todayStr() && dayCache.has(date)) return dayCache.get(date)!;
  const summary = await invoke<DaySummary>("get_day", { date, dayStart, dayEnd });
  dayCache.set(date, summary);
  return summary;
}

export async function getRange(label: string, dayStart: number, dayEnd: number): Promise<RangeSummary> {
  if (!isTauri()) {
    const days: RangeSummary["daily"] = [];
    let pc = 0;
    for (let t = dayStart; t < dayEnd; t += DAY_MS) {
      const d = 5 * 3600_000 + (t % 5) * 600_000;
      pc += d;
      days.push({ day_ms: t, pc_usage_ms: d });
    }
    return {
      label,
      pc_usage_ms: pc,
      app_totals: mockDay("x").app_totals,
      git: mockDay("x").git,
      daily: days,
    };
  }
  return invoke<RangeSummary>("get_range", { label, dayStart, dayEnd });
}

export async function getTimeline(dayStart: number, dayEnd: number): Promise<Session[]> {
  if (!isTauri()) return MOCK_SESSIONS;
  return invoke<Session[]>("timeline", { dayStart, dayEnd });
}

export async function getAppStats(start: number, end: number): Promise<AppTotal[]> {
  if (!isTauri()) return MOCK_STATS;
  return invoke<AppTotal[]>("app_stats", { start, end });
}

export async function startTracking(): Promise<boolean> {
  if (!isTauri()) return true;
  return invoke<boolean>("start_tracking");
}

export async function stopTracking(): Promise<void> {
  if (!isTauri()) return;
  await invoke("stop_tracking");
}

export async function isTracking(): Promise<boolean> {
  if (!isTauri()) return true;
  return invoke<boolean>("is_tracking");
}

export async function getProjects(): Promise<string[]> {
  if (!isTauri()) return ["C:\\projects\\devbox"];
  return invoke<string[]>("get_projects");
}

export async function setProjects(paths: string[]): Promise<string[]> {
  if (!isTauri()) return paths;
  return invoke<string[]>("set_projects", { paths });
}

export interface ProjectProbe {
  path: string;
  target: "windows" | "wsl";
  repository: boolean;
  errorCode: string | null;
}

export async function probeProject(path: string): Promise<ProjectProbe> {
  if (!isTauri()) {
    return { path, target: "windows", repository: false, errorCode: "browser_preview_only" };
  }
  return invoke<ProjectProbe>("probe_project", { path });
}

export async function getIdleThreshold(): Promise<number> {
  if (!isTauri()) return 300000;
  return invoke<number>("get_idle_threshold");
}

export async function setIdleThreshold(thresholdMs: number): Promise<void> {
  if (!isTauri()) return;
  await invoke("set_idle_threshold", { thresholdMs });
}

export interface PrivacyRules {
  excludedProcesses: string[];
  excludedTitlePatterns: string[];
  redactTitlePatterns: string[];
  maskAllTitles: boolean;
}

export async function getPrivacyRules(): Promise<PrivacyRules> {
  if (!isTauri()) return { excludedProcesses: [], excludedTitlePatterns: [], redactTitlePatterns: [], maskAllTitles: false };
  return invoke<PrivacyRules>("get_privacy_rules");
}

export async function setPrivacyRules(rules: PrivacyRules): Promise<void> {
  if (!isTauri()) return;
  await invoke("set_privacy_rules", { rules });
}

export async function redactExisting(): Promise<number> {
  if (!isTauri()) return 0;
  return invoke<number>("redact_existing");
}

export interface AutostartStatus {
  supported: boolean;
  enabled: boolean;
  command: string | null;
}

export async function autostartStatus(): Promise<AutostartStatus> {
  if (!isTauri()) return { supported: true, enabled: false, command: null };
  return invoke<AutostartStatus>("autostart_status");
}

export async function setAutostart(enabled: boolean): Promise<AutostartStatus> {
  if (!isTauri()) return { supported: true, enabled, command: null };
  return invoke<AutostartStatus>("set_autostart", { enabled });
}

export interface SourceStatus {
  producer: string;
  available: boolean;
  schemaVersion: number | null;
  producerVersion: string | null;
  generatedAt: string | null;
  freshnessMs: number | null;
  freshnessState?: "fresh" | "stale" | "expired" | "unknown" | "error";
  scope?: string;
  errorCode?: string | null;
  explanation?: string;
  error: string | null;
  knowledgeActivity: KnowledgeActivity | null;
}

export interface KnowledgeActivity {
  notesModifiedToday: number;
  lastModifiedAtMs: number | null;
  identifiedNotes: number;
  identifiersComplete: boolean;
  legacySnapshot: boolean;
}

export async function integrationSources(): Promise<SourceStatus[]> {
  if (!isTauri()) {
    return [
      {
        producer: "knowledge-base",
        available: true,
        schemaVersion: 1,
        producerVersion: "0.5.0",
        generatedAt: new Date().toISOString(),
        freshnessMs: 30_000,
        freshnessState: "fresh",
        scope: "latest-snapshot-out-of-range",
        errorCode: null,
        explanation: "Knowledge의 최신 local snapshot을 provenance로만 표시하며 활동 원문은 읽지 않습니다.",
        error: null,
        knowledgeActivity: {
          notesModifiedToday: 4,
          lastModifiedAtMs: Date.now() - 90_000,
          identifiedNotes: 4,
          identifiersComplete: true,
          legacySnapshot: false,
        },
      },
      {
        producer: "run-manager",
        available: true,
        schemaVersion: 1,
        producerVersion: "0.3.0",
        generatedAt: new Date().toISOString(),
        freshnessMs: 30_000,
        freshnessState: "fresh",
        scope: "latest-snapshot-out-of-range",
        errorCode: null,
        explanation: "Run Manager의 최신 local snapshot을 provenance로만 표시하며 PC 통계에는 합치지 않습니다.",
        error: null,
        knowledgeActivity: null,
      },
    ];
  }
  return invoke<SourceStatus[]>("integration_sources");
}

export interface Attribution {
  projectId: string;
  sessions: number;
  durationMs: number;
}

export interface AttributionResult {
  attributed: Attribution[];
  unattributed: Attribution;
  profileCount: number;
}

export async function projectAttribution(dayStart: number, dayEnd: number): Promise<AttributionResult> {
  if (!isTauri()) {
    return {
      attributed: [{ projectId: "C:\\projects\\devbox", sessions: 5, durationMs: 4 * 3600000 }],
      unattributed: { projectId: "unattributed", sessions: 2, durationMs: 3600000 },
      profileCount: 1,
    };
  }
  return invoke<AttributionResult>("project_attribution", { dayStart, dayEnd });
}

/** 명시적인 export action에서만 호출하는 bounded preview 생성. */
function validateBrowserExportInput(input: ExportInput): void {
  if (!input
      || typeof input !== "object"
      || !hasExactKeys(input, EXPORT_INPUT_KEYS)
      || typeof input.startDate !== "string"
      || typeof input.endDate !== "string"
      || typeof input.timezone !== "string"
      || !Array.isArray(input.dayBoundaries)
      || !parseLocalDateKey(input.startDate)
      || !parseLocalDateKey(input.endDate)
      || input.startDate > input.endDate
      || input.dayBoundaries.length < 1
      || input.dayBoundaries.length > MAX_EXPORT_DAYS
      || !Number.isSafeInteger(input.dayStart)
      || !Number.isSafeInteger(input.dayEnd)
      || input.dayEnd <= input.dayStart
      || input.timezone.length > 128
      || new TextEncoder().encode(input.timezone).byteLength > 128
      || input.timezone.trim() !== input.timezone
      || [...input.timezone].some(isControlCharacter)
      || !["markdown", "json", "csv"].includes(input.format)
      || !Number.isSafeInteger(input.dayEnd - input.dayStart)
      || input.dayEnd - input.dayStart > DAY_MS * (MAX_EXPORT_DAYS + 1)) {
    throw new Error("브라우저 미리보기 입력이 올바르지 않습니다");
  }
  let previousEnd = input.dayStart;
  let expectedDate = input.startDate;
  for (const boundary of input.dayBoundaries) {
    if (!boundary
        || typeof boundary !== "object"
        || !hasExactKeys(boundary, DAY_BOUNDARY_KEYS)
        || typeof boundary.date !== "string"
        || !parseLocalDateKey(boundary.date)
        || boundary.date !== expectedDate
        || !Number.isSafeInteger(boundary.startMs)
        || !Number.isSafeInteger(boundary.endMs)
        || boundary.startMs !== previousEnd
        || boundary.endMs <= boundary.startMs
        || !Number.isSafeInteger(boundary.endMs - boundary.startMs)
        || !isCivilDaySpan(boundary.endMs - boundary.startMs)) {
      throw new Error("브라우저 미리보기 입력이 올바르지 않습니다");
    }
    previousEnd = boundary.endMs;
    expectedDate = nextLocalDateKey(expectedDate);
  }
  if (previousEnd !== input.dayEnd || expectedDate !== nextLocalDateKey(input.endDate)) {
    throw new Error("브라우저 미리보기 입력이 올바르지 않습니다");
  }
}

function csvPreviewCell(value: string): string {
  return /[,"\r\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;
}

function markdownPreviewCell(value: string): string {
  return value.replace(/[|`\\]/g, "\\$&").replace(/[\r\n]/g, " ");
}

export async function exportLifeLog(input: ExportInput): Promise<RenderedExport> {
  if (!isTauri()) {
    validateBrowserExportInput(input);
    const boundarySummary = input.dayBoundaries
      .map((boundary) => `${boundary.date}:${boundary.startMs}-${boundary.endMs}`)
      .join(", ");
    const content =
      input.format === "json"
        ? JSON.stringify(
            {
              schemaVersion: 1,
              origin: "browser-preview",
              range: {
                startDate: input.startDate,
                endDate: input.endDate,
                timezone: input.timezone,
                startMs: input.dayStart,
                endMs: input.dayEnd,
                dayBoundaries: input.dayBoundaries,
              },
              rules: {
                sessionWindow: "start_ts_ms >= range.startMs && start_ts_ms < range.endMs",
                sessionDuration: "stored durationMs is retained; a session is assigned by start timestamp and is not clipped to the range",
                dailyBuckets: "daily rows use the supplied local civil-day boundaries; each session belongs to the bucket containing its start timestamp",
                privacy: "current Life Log privacy rules and obvious credential markers are reapplied before aggregation",
                appTotals: "sanitized sessions grouped by app; duration descending then app byte order",
                gitCommits: "native-only read-only git log; unavailable in browser preview",
                snapshotScope: "native-only validated snapshots; browser preview includes no local snapshot",
              },
              summary: {
                pcUsageMs: 0,
                sessionCount: 0,
                appTotals: [],
                git: { projects: [], totalCommits: 0, errorCodes: ["browser_preview_only"] },
                run: null,
                knowledge: null,
              },
              daily: input.dayBoundaries.map((day) => ({
                date: day.date,
                startMs: day.startMs,
                endMs: day.endMs,
                pcUsageMs: 0,
                sessionCount: 0,
                gitCommits: 0,
              })),
              sessions: [],
              sources: [
                {
                  id: "life-log",
                  available: false,
                  schemaVersion: null,
                  snapshotVersion: null,
                  producerVersion: null,
                  generatedAt: null,
                  freshnessMs: null,
                  view: null,
                  scope: "browser-preview-only",
                  errorCode: "browser_preview_only",
                },
                {
                  id: "git",
                  available: false,
                  schemaVersion: null,
                  snapshotVersion: null,
                  producerVersion: null,
                  generatedAt: null,
                  freshnessMs: null,
                  view: null,
                  scope: "browser-preview-only",
                  errorCode: "browser_preview_only",
                },
                {
                  id: "run-manager",
                  available: false,
                  schemaVersion: null,
                  snapshotVersion: null,
                  producerVersion: null,
                  generatedAt: null,
                  freshnessMs: null,
                  view: null,
                  scope: "browser-preview-only",
                  errorCode: "browser_preview_only",
                },
                {
                  id: "knowledge-base",
                  available: false,
                  schemaVersion: null,
                  snapshotVersion: null,
                  producerVersion: null,
                  generatedAt: null,
                  freshnessMs: null,
                  view: null,
                  scope: "browser-preview-only",
                  errorCode: "browser_preview_only",
                },
              ],
            },
            null,
            2,
          ) + "\n"
        : input.format === "csv"
          ? `${EXPORT_CSV_HEADER}\r\n${[
              "life-log", "git", "run-manager", "knowledge-base",
            ].map((source) => [
              "source", "", input.startDate, input.endDate, "", "", "", "", "", "", "", "", "", "",
              source, "false", "", "", "", "", "", "", "browser-preview-only", "browser_preview_only",
            ].map(csvPreviewCell).join(",")).join("\r\n")}\r\n`
          : `# Life Log digest preview\n\n- Browser preview only: native DB, Git, and local snapshots are not included.\n- Range: ${input.startDate} to ${input.endDate}\n- Timezone: ${markdownPreviewCell(input.timezone)}\n- Day boundaries: ${boundarySummary}\n`;
    const byteLength = new TextEncoder().encode(content).byteLength;
    if (byteLength > MAX_EXPORT_BYTES) throw new Error("브라우저 미리보기 결과가 너무 큽니다");
    return {
      origin: "browser-preview",
      format: input.format,
      extension: input.format === "markdown" ? "md" : input.format,
      mimeType: input.format === "markdown" ? "text/markdown;charset=utf-8" : `${input.format === "json" ? "application/json" : "text/csv"};charset=utf-8`,
      byteLength,
      content,
    };
  }
  return invoke<RenderedExport>("export_life_log", { input });
}

/** Windows native save dialog + backend atomic write. */
export async function saveLifeLog(input: ExportInput): Promise<SaveExportResult> {
  if (!isTauri()) throw new Error("native export 저장은 데스크톱 앱에서 사용할 수 없습니다");
  return invoke<SaveExportResult>("save_life_log", { input });
}

function digestError(): Error {
  return new Error("digest 입력이 올바르지 않습니다");
}

function hasSecretMarker(value: string): boolean {
  const lower = value.toLowerCase();
  return [
    "password", "passwd", "secret", "token", "access_token", "refresh_token",
    "api_key", "apikey", "client_secret", "credential", "authorization",
    "bearer ", "basic ", "sk-", "ghp_", "gho_", "ghs_", "ghu_",
    "github_pat_", "xoxb-", "xoxp-", "npm_", "pypi-", "akia", "ya29.",
    "-----begin ",
  ].some((marker) => lower.includes(marker));
}

/** Shared frontend boundary for native and browser digest requests. */
export function validateDigestInput(input: DigestInput): void {
  if (!input || typeof input !== "object" || !hasExactKeys(input, DIGEST_INPUT_KEYS)
      || !input.filter || typeof input.filter !== "object"
      || !hasExactKeys(input.filter, DIGEST_FILTER_KEYS)
      || !["day", "week", "month"].includes(input.period)
      || (input.filter.app !== null && typeof input.filter.app !== "string")) {
    throw digestError();
  }
  const exportInput: ExportInput = {
    startDate: input.startDate,
    endDate: input.endDate,
    timezone: input.timezone,
    dayStart: input.dayStart,
    dayEnd: input.dayEnd,
    dayBoundaries: input.dayBoundaries,
    format: "json",
  };
  try {
    validateBrowserExportInput(exportInput);
  } catch {
    throw digestError();
  }
  if (input.filter.app !== null && input.filter.app !== undefined
      && (input.filter.app.length === 0 || input.filter.app.length > 256
        || new TextEncoder().encode(input.filter.app).byteLength > 256
        || [...input.filter.app].some(isControlCharacter) || hasSecretMarker(input.filter.app))) {
    throw digestError();
  }
  const days = input.dayBoundaries.length;
  if ((input.period === "day" && (days !== 1 || input.startDate !== input.endDate))
      || (input.period === "week" && (days !== 7 || !isMondayDateKey(input.startDate)))
      || (input.period === "month" && (days < 28 || days > 31
        || !input.startDate.endsWith("-01")
        || input.startDate.slice(0, 7) !== input.endDate.slice(0, 7)
        || !isMonthEndDateKey(input.endDate)))) {
    throw digestError();
  }
}

function browserDigestRules(appFilter: string | null): DigestRules {
  return {
    sessionWindow: "start_ts_ms >= range.startMs && start_ts_ms < range.endMs",
    sessionDuration: "stored durationMs is retained; sessions are assigned by start timestamp and are not clipped",
    dailyBuckets: "the supplied local civil-day boundaries are authoritative; no fixed 24-hour arithmetic is used",
    appFilter: appFilter ? `exact sanitized app \`${appFilter}\` only` : "all sanitized applications",
    appTotals: "sanitized sessions are grouped by app; duration descending then app byte order",
    gitCommits: "native-only read-only bounded Git counts; unavailable in browser preview",
    snapshotScope: "Run Manager and Knowledge latest snapshots are provenance only and unavailable in browser preview",
    privacy: "Life Log privacy rules and obvious credential markers are reapplied before aggregation",
    externalProcessing: "rule-based local aggregation only; no cloud/local LLM, network, telemetry, or external activity transfer",
  };
}

function markdownPreview(value: string): string {
  return value.replace(/[|\\]/g, "\\$&").replace(/`/g, "\\`").replace(/[\r\n]/g, " ");
}

function browserDigestMarkdown(input: DigestInput, document: DigestDocument): string {
  const filter = input.filter.app ?? "all apps";
  const daily = document.daily.map((day) =>
    `| ${day.date} | 0 | 0 | 0 | - |`,
  ).join("\n");
  const sources = document.sources.map((source) =>
    `| ${source.id} | false | ${source.scope} | ${source.errorCode ?? "-"} |`,
  ).join("\n");
  const rules = Object.entries(document.rules).map(([name, value]) =>
    `| ${name} | ${markdownPreview(value)} |`,
  ).join("\n");
  return [
    "# Life Log local digest",
    "",
    `- Period: \`${input.period}\``,
    `- Range: \`${input.startDate}\` to \`${input.endDate}\` (date keys inclusive; end timestamp exclusive)`,
    `- Timezone: \`${markdownPreview(input.timezone)}\``,
    `- Filter: ${markdownPreview(filter)}`,
    "- Browser preview only: native DB, Git, and local snapshots are not included.",
    "",
    "## Summary",
    "",
    "| Metric | Value |",
    "| --- | ---: |",
    "| PC usage (ms) | 0 |",
    "| Sessions | 0 |",
    `| Active days | 0 / ${document.daily.length} |`,
    "| Average daily usage (ms) | 0 |",
    "| Git commits | 0 |",
    "| Top app | - |",
    "",
    "No activity was recorded in the browser preview.",
    "",
    "## Daily digest",
    "",
    "| Date | PC usage (ms) | Sessions | Git commits | Top app |",
    "| --- | ---: | ---: | ---: | --- |",
    daily,
    "",
    "## Applications",
    "",
    "| App | Duration (ms) | Sessions |",
    "| --- | ---: | ---: |",
    "| - | 0 | 0 |",
    "",
    "## Git projects",
    "",
    "| Project | Commits | Error code |",
    "| --- | ---: | --- |",
    "| - | 0 | browser_preview_only |",
    "",
    "## Sources",
    "",
    "| Source | Available | Scope | Error code |",
    "| --- | --- | --- | --- |",
    sources,
    "",
    "## Rules",
    "",
    "| Rule | Definition |",
    "| --- | --- |",
    rules,
    "",
  ].join("\n");
}

function browserDigest(input: DigestInput): DigestResponse {
  validateDigestInput(input);
  const sources = ["life-log", "git", "run-manager", "knowledge-base"].map((id) => ({
    id,
    available: false,
    schemaVersion: null,
    snapshotVersion: null,
    producerVersion: null,
    generatedAt: null,
    freshnessMs: null,
    view: null,
    scope: "browser-preview-only",
    errorCode: "browser_preview_only",
  }));
  const document: DigestDocument = {
    schemaVersion: 1,
    period: input.period,
    range: {
      startDate: input.startDate,
      endDate: input.endDate,
      timezone: input.timezone,
      startMs: input.dayStart,
      endMs: input.dayEnd,
      dayBoundaries: input.dayBoundaries,
    },
    filter: input.filter,
    rules: browserDigestRules(input.filter.app),
    headline: `${input.period} local digest preview: native local data is unavailable`,
    summary: {
      pcUsageMs: 0,
      sessionCount: 0,
      activeDays: 0,
      totalDays: input.dayBoundaries.length,
      averageDailyUsageMs: 0,
      topApp: null,
      gitCommits: 0,
    },
    daily: input.dayBoundaries.map((boundary) => ({
      date: boundary.date,
      startMs: boundary.startMs,
      endMs: boundary.endMs,
      pcUsageMs: 0,
      sessionCount: 0,
      gitCommits: 0,
      topApp: null,
      hasActivity: false,
    })),
    appTotals: [],
    git: { projects: [], totalCommits: 0, errorCodes: ["browser_preview_only"] },
    sources,
  };
  const markdown = browserDigestMarkdown(input, document);
  const byteLength = new TextEncoder().encode(markdown).byteLength;
  if (byteLength > MAX_EXPORT_BYTES) throw new Error("digest 미리보기 결과가 너무 큽니다");
  return { origin: "browser-preview", document, markdown, handle: null };
}

export async function getDigest(input: DigestInput): Promise<DigestResponse> {
  if (!isTauri()) return browserDigest(input);
  validateDigestInput(input);
  return invoke<DigestResponse>("get_digest", { input });
}

export async function cancelDigest(): Promise<boolean> {
  if (!isTauri()) return false;
  return invoke<boolean>("cancel_digest");
}

export async function saveDigest(handle: string): Promise<SaveDigestResult> {
  if (!isTauri()) throw new Error("native digest 저장은 데스크톱 앱에서 사용할 수 없습니다");
  if (typeof handle !== "string" || !/^[0-9a-f]{32}$/i.test(handle)) {
    throw new Error("digest 저장 핸들이 만료되었습니다");
  }
  return invoke<SaveDigestResult>("save_digest", { request: { handle } });
}

/** Native-only explicit handoff. Browser preview never publishes or launches. */
export async function sendDigestToKnowledge(input: DigestInput, regeneratedFrom: string | null = null): Promise<SendKnowledgeDraftResult> {
  if (!isTauri()) throw new Error("Knowledge handoff는 데스크톱 앱에서 사용할 수 없습니다");
  validateDigestInput(input);
  return invoke<SendKnowledgeDraftResult>("send_digest_to_knowledge", { input, regeneratedFrom });
}

export async function knowledgeDraftHistory(): Promise<KnowledgeDraftHistoryEntry[]> {
  if (!isTauri()) return [];
  return invoke<KnowledgeDraftHistoryEntry[]>("knowledge_draft_history");
}
