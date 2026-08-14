import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { AppTotal, DaySummary, RangeSummary, Session } from "./types";

const DAY_MS = 86_400_000;

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
