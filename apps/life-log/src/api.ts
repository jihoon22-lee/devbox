import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { DaySummary } from "./types";

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

export async function getDay(date: string, dayStart: number, dayEnd: number): Promise<DaySummary> {
  if (!isTauri()) return mockDay(date);
  return invoke<DaySummary>("get_day", { date, dayStart, dayEnd });
}

export async function getActivityDb(): Promise<string> {
  if (!isTauri()) return "%LOCALAPPDATA%\\Workbench\\activity-timeline\\data.db";
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
