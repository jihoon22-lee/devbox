import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { DaySummary, RangeSummary } from "./types";

const DAY_MS = 86_400_000;

function mockDay(date: string): DaySummary {
  return {
    date,
    pc_usage_ms: 7 * 3600_000 + 21 * 60_000,
    app_totals: [
      { app: "Code.exe", duration_ms: 3 * 3600_000 + 42 * 60_000 },
      { app: "chrome.exe", duration_ms: 2 * 3600_000 + 13 * 60_000 },
      { app: "WindowsTerminal.exe", duration_ms: 1 * 3600_000 + 24 * 60_000 },
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

export async function getActivityDb(): Promise<string> {
  if (!isTauri()) return "%LOCALAPPDATA%\\com.workbench.activitytimeline\\data.db";
  return invoke<string>("get_activity_db");
}

export async function setActivityDb(path: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("set_activity_db", { path });
}

export async function getProjects(): Promise<string[]> {
  if (!isTauri()) return ["C:\\projects\\devbox"];
  return invoke<string[]>("get_projects");
}

export async function setProjects(paths: string[]): Promise<void> {
  if (!isTauri()) return;
  await invoke("set_projects", { paths });
}
