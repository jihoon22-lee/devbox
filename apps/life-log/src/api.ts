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

const DAY_MS = 86_400_000;
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

export async function setProjects(paths: string[]): Promise<void> {
  if (!isTauri()) return;
  await invoke("set_projects", { paths });
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
        || boundary.endMs - boundary.startMs > DAY_MS * 2) {
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
