import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { AppTotal, Session } from "./types";

const MOCK_SESSIONS: Session[] = [
  { id: 1, app: "chrome.exe", title: "GitHub", start_ts: new Date(2026, 7, 10, 9, 22).getTime(), end_ts: new Date(2026, 7, 10, 9, 41).getTime(), duration_ms: 1140000 },
  { id: 2, app: "Code.exe", title: "FamilyCard", start_ts: new Date(2026, 7, 10, 9, 41).getTime(), end_ts: new Date(2026, 7, 10, 10, 8).getTime(), duration_ms: 1620000 },
  { id: 3, app: "WindowsTerminal.exe", title: "Ubuntu", start_ts: new Date(2026, 7, 10, 10, 8).getTime(), end_ts: new Date(2026, 7, 10, 10, 42).getTime(), duration_ms: 2040000 },
  { id: 4, app: "chrome.exe", title: "ChatGPT", start_ts: new Date(2026, 7, 10, 10, 42).getTime(), end_ts: new Date(2026, 7, 10, 11, 5).getTime(), duration_ms: 1380000 },
];

const MOCK_STATS: AppTotal[] = [
  { app: "Code.exe", duration_ms: 10080000, sessions: 8 },
  { app: "chrome.exe", duration_ms: 7980000, sessions: 12 },
  { app: "WindowsTerminal.exe", duration_ms: 5040000, sessions: 6 },
];

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
