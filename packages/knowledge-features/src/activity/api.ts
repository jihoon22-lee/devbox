import { typedCall } from "../typed";
import type { KnowledgeActivityCall } from "../generated/KnowledgeActivityCall";
import type { ActivityResults } from "../generated/activity-results";
export const activityCall = typedCall<KnowledgeActivityCall, ActivityResults>("knowledge.activity");
import { isTauri } from "./lib/isTauri";
import type { AppTotal, DaySummary, RangeSummary, Session } from "./types";

/** Export and digest documents share the same versioned activity contract. */
export const LIFE_LOG_SCHEMA_VERSION = 2 as const;
export const EXPORT_SCHEMA_VERSION = LIFE_LOG_SCHEMA_VERSION;
export const DIGEST_SCHEMA_VERSION = LIFE_LOG_SCHEMA_VERSION;
export const BROWSER_PREVIEW_SCOPE = "browser-preview-only" as const;
export const BROWSER_PREVIEW_ERROR_CODE = "browser_preview_only" as const;
export const NATIVE_SOURCE_IDS = ["life-log", "git", "run-manager", "knowledge-base"] as const;

export type ExportFormat = import("../generated/ExportFormat").ExportFormat;
export type ExportOrigin = import("../generated/ExportOrigin").ExportOrigin;

export type ExportDayBoundary = import("../generated/ExportDayBoundary").ExportDayBoundary;

export type ExportInput = import("../generated/ExportInput").ExportInput;

export type RunDigest = import("../generated/RunDigest").RunDigest;

export type KnowledgeDigest = import("../generated/KnowledgeDigest").KnowledgeDigest;

export type ExportDailyDigest = import("../generated/DailyDigest").DailyDigest;

export type ExportSummary = import("../generated/ExportSummary").ExportSummary;

export type ExportSourceMetadata = import("../generated/SourceMetadata").SourceMetadata;

export type ExportDocument = import("../generated/ExportDocument").ExportDocument;

export type RenderedExport = import("../generated/RenderedExport").RenderedExport;

export type SaveExportResult = import("../generated/SaveExportResult").SaveExportResult;

export type DigestPeriod = import("../generated/DigestPeriod").DigestPeriod;

export type DigestFilter = import("../generated/DigestFilter").DigestFilter;

export type DigestInput = import("../generated/DigestInput").DigestInput;

export type DigestRules = import("../generated/DigestRules").DigestRules;

export type DigestDay = import("../generated/DigestDay").DigestDay;

export type DigestSummary = import("../generated/DigestSummary").DigestSummary;

export type DigestDocument = import("../generated/DigestDocument").DigestDocument;

export type DigestResponse = import("../generated/ActivityDigestResponse").ActivityDigestResponse;

export type SaveDigestResult = import("../generated/SaveDigestResult").SaveDigestResult;

export type SendKnowledgeDraftResult = import("../generated/SendKnowledgeDraftResult").SendKnowledgeDraftResult;

export type DraftHandoffStatus = import("../generated/DraftStatus").DraftStatus;

export type KnowledgeDraftSummary = import("../generated/KnowledgeDraftSummary").KnowledgeDraftSummary;

export type KnowledgeDraftHistoryEntry = import("../generated/DraftHistoryEntry").DraftHistoryEntry;

const DAY_MS = 86_400_000;
const MIN_CIVIL_DAY_MS = DAY_MS - 3_600_000;
const MAX_CIVIL_DAY_MS = DAY_MS + 3_600_000;
const MAX_EXPORT_DAYS = 366;
const MAX_EXPORT_BYTES = 4 * 1024 * 1024;
const EXPORT_CSV_HEADER =
  "record_type,date,range_start_date,range_end_date,id,app,title,start_ts_ms,end_ts_ms,duration_ms,project_path,commits,metric,value,source,available,schema_version,snapshot_version,producer_version,generated_at,freshness_ms,view,scope,error_code";
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
  return actual.length === sortedExpected.length && actual.every((key, index) => key === sortedExpected[index]);
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
  return date.getFullYear() === year && date.getMonth() === month - 1 && date.getDate() === day ? date : null;
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
  {
    id: 1,
    app: "chrome.exe",
    title: "GitHub",
    start_ts: new Date(2026, 7, 10, 9, 22).getTime(),
    end_ts: new Date(2026, 7, 10, 9, 41).getTime(),
    duration_ms: 1140000,
  },
  {
    id: 2,
    app: "Code.exe",
    title: "FamilyCard",
    start_ts: new Date(2026, 7, 10, 9, 41).getTime(),
    end_ts: new Date(2026, 7, 10, 10, 8).getTime(),
    duration_ms: 1620000,
  },
  {
    id: 3,
    app: "WindowsTerminal.exe",
    title: "Ubuntu",
    start_ts: new Date(2026, 7, 10, 10, 8).getTime(),
    end_ts: new Date(2026, 7, 10, 10, 42).getTime(),
    duration_ms: 2040000,
  },
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
  const summary = await activityCall("get_day", { date, dayStart, dayEnd });
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
  return activityCall("get_range", { label, dayStart, dayEnd });
}

export async function getTimeline(dayStart: number, dayEnd: number): Promise<Session[]> {
  if (!isTauri()) return MOCK_SESSIONS;
  return activityCall("timeline", { dayStart, dayEnd });
}

export async function getAppStats(start: number, end: number): Promise<AppTotal[]> {
  if (!isTauri()) return MOCK_STATS;
  return activityCall("app_stats", { start, end });
}

export async function startTracking(): Promise<boolean> {
  if (!isTauri()) return true;
  return activityCall("start_tracking", {});
}

export async function stopTracking(): Promise<void> {
  if (!isTauri()) return;
  await activityCall("stop_tracking", {});
}

export async function isTracking(): Promise<boolean> {
  if (!isTauri()) return true;
  return activityCall("is_tracking", {});
}

export async function getProjects(): Promise<string[]> {
  if (!isTauri()) return ["C:\\projects\\devbox"];
  return activityCall("get_projects", {});
}

export async function setProjects(paths: string[]): Promise<string[]> {
  if (!isTauri()) return paths;
  return activityCall("set_projects", { paths });
}

export type ProjectProbe = import("../generated/ProjectProbe").ProjectProbe;

export async function probeProject(path: string): Promise<ProjectProbe> {
  if (!isTauri()) {
    return { path, target: "windows", repository: false, errorCode: "browser_preview_only" };
  }
  return activityCall("probe_project", { path });
}

export async function getIdleThreshold(): Promise<number> {
  if (!isTauri()) return 300000;
  return activityCall("get_idle_threshold", {});
}

export async function setIdleThreshold(thresholdMs: number): Promise<void> {
  if (!isTauri()) return;
  await activityCall("set_idle_threshold", { thresholdMs });
}

export type PrivacyRules = import("../generated/PrivacyRules").PrivacyRules;
export type PrivacyRuleField = import("../generated/RuleField").RuleField;
export type PrivacyRuleProblem = import("../generated/RuleProblem").RuleProblem;
export type InvalidPrivacyRule = import("../generated/InvalidRule").InvalidRule;
export type PrivacyRulesView = import("../generated/PrivacyRulesView").PrivacyRulesView;
export type PrivacySaveResult = import("../generated/PrivacySaveResult").PrivacySaveResult;

export const EMPTY_PRIVACY_RULES: PrivacyRules = {
  excludedProcesses: [],
  excludedTitlePatterns: [],
  redactTitlePatterns: [],
  maskAllTitles: false,
};

export async function getPrivacyRules(): Promise<PrivacyRulesView> {
  if (!isTauri()) return { rules: EMPTY_PRIVACY_RULES, healthy: true };
  return activityCall("get_privacy_rules", {});
}

export async function setPrivacyRules(rules: PrivacyRules): Promise<PrivacySaveResult> {
  if (!isTauri()) return { saved: true, invalid: [] };
  return activityCall("set_privacy_rules", { rules });
}

export async function redactExisting(): Promise<number> {
  if (!isTauri()) return 0;
  return activityCall("redact_existing", {});
}

export type AutostartStatus = import("../generated/AutostartStatus").AutostartStatus;

export async function autostartStatus(): Promise<AutostartStatus> {
  if (!isTauri()) return { supported: true, enabled: false, command: null };
  return activityCall("autostart_status", {});
}

export async function setAutostart(enabled: boolean): Promise<AutostartStatus> {
  if (!isTauri()) return { supported: true, enabled, command: null };
  return activityCall("set_autostart", { enabled });
}

export type SourceStatus = import("../generated/SourceStatus").SourceStatus;

export type KnowledgeActivity = import("../generated/KnowledgeActivity").KnowledgeActivity;

function browserPreviewSourceMetadata(id: (typeof NATIVE_SOURCE_IDS)[number]): ExportSourceMetadata {
  return {
    id,
    available: false,
    schemaVersion: null,
    snapshotVersion: null,
    producerVersion: null,
    generatedAt: null,
    freshnessMs: null,
    view: null,
    scope: BROWSER_PREVIEW_SCOPE,
    errorCode: BROWSER_PREVIEW_ERROR_CODE,
  };
}

function browserPreviewSourceStatuses(): SourceStatus[] {
  return NATIVE_SOURCE_IDS.map((producer) => ({
    producer,
    available: false,
    schemaVersion: null,
    producerVersion: null,
    generatedAt: null,
    freshnessMs: null,
    freshnessState: "unknown",
    scope: BROWSER_PREVIEW_SCOPE,
    errorCode: BROWSER_PREVIEW_ERROR_CODE,
    explanation: "브라우저 미리보기에서는 native DB와 local snapshot을 읽지 않습니다.",
    error: null,
    knowledgeActivity: null,
  }));
}

export async function integrationSources(): Promise<SourceStatus[]> {
  if (!isTauri()) {
    return browserPreviewSourceStatuses();
  }
  return activityCall("integration_sources", {});
}

export type Attribution = import("../generated/ActivityAttribution").ActivityAttribution;

export type AttributionResult = import("../generated/ActivityAttributionResult").ActivityAttributionResult;

export async function projectAttribution(dayStart: number, dayEnd: number): Promise<AttributionResult> {
  if (!isTauri()) {
    return {
      attributed: [{ projectId: "C:\\projects\\devbox", sessions: 5, durationMs: 4 * 3600000 }],
      unattributed: { projectId: "unattributed", sessions: 2, durationMs: 3600000 },
      profileCount: 1,
    };
  }
  return activityCall("project_attribution", { dayStart, dayEnd });
}

/** 명시적인 export action에서만 호출하는 bounded preview 생성. */
function validateBrowserExportInput(input: ExportInput): void {
  if (
    !input ||
    typeof input !== "object" ||
    !hasExactKeys(input, EXPORT_INPUT_KEYS) ||
    typeof input.startDate !== "string" ||
    typeof input.endDate !== "string" ||
    typeof input.timezone !== "string" ||
    !Array.isArray(input.dayBoundaries) ||
    !parseLocalDateKey(input.startDate) ||
    !parseLocalDateKey(input.endDate) ||
    input.startDate > input.endDate ||
    input.dayBoundaries.length < 1 ||
    input.dayBoundaries.length > MAX_EXPORT_DAYS ||
    !Number.isSafeInteger(input.dayStart) ||
    !Number.isSafeInteger(input.dayEnd) ||
    input.dayEnd <= input.dayStart ||
    input.timezone.length > 128 ||
    new TextEncoder().encode(input.timezone).byteLength > 128 ||
    input.timezone.trim() !== input.timezone ||
    [...input.timezone].some(isControlCharacter) ||
    !["markdown", "json", "csv"].includes(input.format) ||
    !Number.isSafeInteger(input.dayEnd - input.dayStart) ||
    input.dayEnd - input.dayStart > DAY_MS * (MAX_EXPORT_DAYS + 1)
  ) {
    throw new Error("브라우저 미리보기 입력이 올바르지 않습니다");
  }
  let previousEnd = input.dayStart;
  let expectedDate = input.startDate;
  for (const boundary of input.dayBoundaries) {
    if (
      !boundary ||
      typeof boundary !== "object" ||
      !hasExactKeys(boundary, DAY_BOUNDARY_KEYS) ||
      typeof boundary.date !== "string" ||
      !parseLocalDateKey(boundary.date) ||
      boundary.date !== expectedDate ||
      !Number.isSafeInteger(boundary.startMs) ||
      !Number.isSafeInteger(boundary.endMs) ||
      boundary.startMs !== previousEnd ||
      boundary.endMs <= boundary.startMs ||
      !Number.isSafeInteger(boundary.endMs - boundary.startMs) ||
      !isCivilDaySpan(boundary.endMs - boundary.startMs)
    ) {
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
              schemaVersion: EXPORT_SCHEMA_VERSION,
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
                sessionDuration:
                  "stored durationMs is retained; a session is assigned by start timestamp and is not clipped to the range",
                dailyBuckets:
                  "daily rows use the supplied local civil-day boundaries; each session belongs to the bucket containing its start timestamp",
                privacy:
                  "current Life Log privacy rules and obvious credential markers are reapplied before aggregation",
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
                runSucceeded: null,
                runFailed: null,
                knowledgeNotesModified: null,
              })),
              sessions: [],
              sources: NATIVE_SOURCE_IDS.map(browserPreviewSourceMetadata),
            },
            null,
            2,
          ) + "\n"
        : input.format === "csv"
          ? `${EXPORT_CSV_HEADER}\r\n${["life-log", "git", "run-manager", "knowledge-base"]
              .map((source) =>
                [
                  "source",
                  "",
                  input.startDate,
                  input.endDate,
                  "",
                  "",
                  "",
                  "",
                  "",
                  "",
                  "",
                  "",
                  "",
                  "",
                  source,
                  "false",
                  "",
                  "",
                  "",
                  "",
                  "",
                  "",
                  "browser-preview-only",
                  "browser_preview_only",
                ]
                  .map(csvPreviewCell)
                  .join(","),
              )
              .join("\r\n")}\r\n`
          : `# Life Log digest preview\n\n- Export schema: ${EXPORT_SCHEMA_VERSION}\n- Browser preview only: native DB, Git, and local snapshots are not included.\n- Range: ${input.startDate} to ${input.endDate}\n- Timezone: ${markdownPreviewCell(input.timezone)}\n- Day boundaries: ${boundarySummary}\n- Run/Knowledge daily metrics: unavailable (native values are not substituted).\n`;
    const byteLength = new TextEncoder().encode(content).byteLength;
    if (byteLength > MAX_EXPORT_BYTES) throw new Error("브라우저 미리보기 결과가 너무 큽니다");
    return {
      origin: "browser-preview",
      format: input.format,
      extension: input.format === "markdown" ? "md" : input.format,
      mimeType:
        input.format === "markdown"
          ? "text/markdown;charset=utf-8"
          : `${input.format === "json" ? "application/json" : "text/csv"};charset=utf-8`,
      byteLength,
      content,
    };
  }
  return activityCall("export_life_log", { input });
}

/** Windows native save dialog + backend atomic write. */
export async function saveLifeLog(input: ExportInput): Promise<SaveExportResult> {
  if (!isTauri()) throw new Error("native export 저장은 데스크톱 앱에서 사용할 수 없습니다");
  return activityCall("save_life_log", { input });
}

function digestError(): Error {
  return new Error("digest 입력이 올바르지 않습니다");
}

function hasSecretMarker(value: string): boolean {
  const lower = value.toLowerCase();
  return [
    "password",
    "passwd",
    "secret",
    "token",
    "access_token",
    "refresh_token",
    "api_key",
    "apikey",
    "client_secret",
    "credential",
    "authorization",
    "bearer ",
    "basic ",
    "sk-",
    "ghp_",
    "gho_",
    "ghs_",
    "ghu_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "npm_",
    "pypi-",
    "akia",
    "ya29.",
    "-----begin ",
  ].some((marker) => lower.includes(marker));
}

/** Shared frontend boundary for native and browser digest requests. */
export function validateDigestInput(input: DigestInput): void {
  if (
    !input ||
    typeof input !== "object" ||
    !hasExactKeys(input, DIGEST_INPUT_KEYS) ||
    !input.filter ||
    typeof input.filter !== "object" ||
    !hasExactKeys(input.filter, DIGEST_FILTER_KEYS) ||
    !["day", "week", "month"].includes(input.period) ||
    (input.filter.app !== null && typeof input.filter.app !== "string")
  ) {
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
  if (
    input.filter.app !== null &&
    input.filter.app !== undefined &&
    (input.filter.app.length === 0 ||
      input.filter.app.length > 256 ||
      new TextEncoder().encode(input.filter.app).byteLength > 256 ||
      [...input.filter.app].some(isControlCharacter) ||
      hasSecretMarker(input.filter.app))
  ) {
    throw digestError();
  }
  const days = input.dayBoundaries.length;
  if (
    (input.period === "day" && (days !== 1 || input.startDate !== input.endDate)) ||
    (input.period === "week" && (days !== 7 || !isMondayDateKey(input.startDate))) ||
    (input.period === "month" &&
      (days < 28 ||
        days > 31 ||
        !input.startDate.endsWith("-01") ||
        input.startDate.slice(0, 7) !== input.endDate.slice(0, 7) ||
        !isMonthEndDateKey(input.endDate)))
  ) {
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
    snapshotScope: "Run Manager and Knowledge daily snapshots are native-only and unavailable in browser preview",
    privacy: "Life Log privacy rules and obvious credential markers are reapplied before aggregation",
    externalProcessing:
      "rule-based local aggregation only; no cloud/local LLM, network, telemetry, or external activity transfer",
  };
}

function markdownPreview(value: string): string {
  return value
    .replace(/[|\\]/g, "\\$&")
    .replace(/`/g, "\\`")
    .replace(/[\r\n]/g, " ");
}

function browserDigestMarkdown(input: DigestInput, document: DigestDocument): string {
  const filter = input.filter.app ?? "all apps";
  const daily = document.daily.map((day) => `| ${day.date} | 0 | 0 | 0 | - | - | - | - |`).join("\n");
  const sources = document.sources
    .map((source) => `| ${source.id} | false | ${source.scope} | ${source.errorCode ?? "-"} |`)
    .join("\n");
  const rules = Object.entries(document.rules)
    .map(([name, value]) => `| ${name} | ${markdownPreview(value)} |`)
    .join("\n");
  return [
    "# Life Log local digest",
    "",
    `- Period: \`${input.period}\``,
    `- Range: \`${input.startDate}\` to \`${input.endDate}\` (date keys inclusive; end timestamp exclusive)`,
    `- Timezone: \`${markdownPreview(input.timezone)}\``,
    `- Filter: ${markdownPreview(filter)}`,
    `- Digest schema: \`${document.schemaVersion}\``,
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
    "| Runs | — |",
    "| Knowledge notes modified | — |",
    "",
    "No activity was recorded in the browser preview.",
    "",
    "## Daily digest",
    "",
    "| Date | PC usage (ms) | Sessions | Git commits | Runs succeeded | Runs failed | Knowledge notes modified | Top app |",
    "| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |",
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
  const sources = NATIVE_SOURCE_IDS.map(browserPreviewSourceMetadata);
  const document: DigestDocument = {
    schemaVersion: DIGEST_SCHEMA_VERSION,
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
      run: null,
      knowledge: null,
    },
    daily: input.dayBoundaries.map((boundary) => ({
      date: boundary.date,
      startMs: boundary.startMs,
      endMs: boundary.endMs,
      pcUsageMs: 0,
      sessionCount: 0,
      gitCommits: 0,
      runSucceeded: null,
      runFailed: null,
      knowledgeNotesModified: null,
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

function isNullableSafeInteger(value: unknown): value is number | null {
  return value === null || (typeof value === "number" && Number.isSafeInteger(value));
}

function isNullableSafeCount(value: unknown): value is number | null {
  return isNullableSafeInteger(value) && (value === null || value >= 0);
}

function isRunDigest(value: unknown): value is RunDigest {
  if (!value || typeof value !== "object") return false;
  const run = value as Partial<RunDigest>;
  return (
    isNullableSafeCount(run.succeeded) &&
    run.succeeded !== null &&
    isNullableSafeCount(run.failed) &&
    run.failed !== null &&
    isNullableSafeInteger(run.lastRunAtMs)
  );
}

function isKnowledgeDigest(value: unknown): value is KnowledgeDigest {
  if (!value || typeof value !== "object") return false;
  const knowledge = value as Partial<KnowledgeDigest>;
  return (
    isNullableSafeCount(knowledge.notesModified) &&
    knowledge.notesModified !== null &&
    isNullableSafeInteger(knowledge.lastModifiedAtMs)
  );
}

/** Keep native responses on the versioned nullable activity contract. */
export function validateDigestResponse(value: unknown): DigestResponse {
  if (!value || typeof value !== "object") {
    throw new Error("digest 응답을 읽을 수 없습니다");
  }
  const response = value as Partial<DigestResponse>;
  const document = response.document;
  if (!document || typeof document !== "object") {
    throw new Error("digest 응답을 읽을 수 없습니다");
  }
  const typedDocument = document as Partial<DigestDocument>;
  const summary = typedDocument.summary;
  const daily = typedDocument.daily;
  if (
    typedDocument.schemaVersion !== DIGEST_SCHEMA_VERSION ||
    !summary ||
    typeof summary !== "object" ||
    !Object.prototype.hasOwnProperty.call(summary, "run") ||
    !Object.prototype.hasOwnProperty.call(summary, "knowledge") ||
    !Array.isArray(daily) ||
    !daily.every((candidate) => {
      if (!candidate || typeof candidate !== "object") return false;
      const day = candidate as Partial<DigestDay>;
      return (
        Object.prototype.hasOwnProperty.call(day, "runSucceeded") &&
        Object.prototype.hasOwnProperty.call(day, "runFailed") &&
        Object.prototype.hasOwnProperty.call(day, "knowledgeNotesModified") &&
        isNullableSafeCount(day.runSucceeded) &&
        isNullableSafeCount(day.runFailed) &&
        isNullableSafeCount(day.knowledgeNotesModified)
      );
    }) ||
    (summary.run !== null && !isRunDigest(summary.run)) ||
    (summary.knowledge !== null && !isKnowledgeDigest(summary.knowledge))
  ) {
    throw new Error("digest 응답을 읽을 수 없습니다");
  }
  return value as DigestResponse;
}

export async function getDigest(input: DigestInput): Promise<DigestResponse> {
  if (!isTauri()) return browserDigest(input);
  validateDigestInput(input);
  return validateDigestResponse(await activityCall("get_digest", { input }));
}

export async function cancelDigest(): Promise<boolean> {
  if (!isTauri()) return false;
  return activityCall("cancel_digest", {});
}

export async function saveDigest(handle: string): Promise<SaveDigestResult> {
  if (!isTauri()) throw new Error("native digest 저장은 데스크톱 앱에서 사용할 수 없습니다");
  if (typeof handle !== "string" || !/^[0-9a-f]{32}$/i.test(handle)) {
    throw new Error("digest 저장 핸들이 만료되었습니다");
  }
  return activityCall("save_digest", { request: { handle } });
}

/** Native-only explicit handoff. Browser preview never publishes or launches. */
export async function sendDigestToKnowledge(
  input: DigestInput,
  regeneratedFrom: string | null = null,
): Promise<SendKnowledgeDraftResult> {
  if (!isTauri()) throw new Error("Knowledge handoff는 데스크톱 앱에서 사용할 수 없습니다");
  validateDigestInput(input);
  return activityCall("send_digest_to_knowledge", { input, regeneratedFrom });
}

export async function knowledgeDraftHistory(): Promise<KnowledgeDraftHistoryEntry[]> {
  if (!isTauri()) return [];
  return activityCall("knowledge_draft_history", {});
}
