import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isTauri } from "./lib/isTauri";

export interface SessionInfo {
  id: string;
  distro: string;
}

export interface TerminalOutput {
  session_id: string;
  data: string;
}

export async function listDistros(): Promise<string[]> {
  if (!isTauri()) return ["Ubuntu"];
  return invoke<string[]>("list_distros");
}

export async function startSession(distro: string, cwd?: string): Promise<string> {
  if (!isTauri()) return "mock-" + Date.now();
  return invoke<string>("start_session", { distro, cwd: cwd ?? null });
}

export async function writeSession(sessionId: string, data: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("write_session", { sessionId, data });
}

/** 활성 탭의 세션 id만 넘겨야 한다 — 등록된 모든 세션이 아니라 대상만 받는다. */
export async function broadcast(sessionIds: string[], data: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("broadcast", { sessionIds, data });
}

export async function resizeSession(sessionId: string, rows: number, cols: number): Promise<void> {
  if (!isTauri()) return;
  await invoke("resize_session", { sessionId, rows, cols });
}

export async function closeSession(sessionId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("close_session", { sessionId });
}

export async function listSessions(): Promise<SessionInfo[]> {
  if (!isTauri()) return [];
  return invoke<SessionInfo[]>("list_sessions");
}

export async function onTerminalOutput(cb: (payload: TerminalOutput) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<TerminalOutput>("terminal-output", (e) => cb(e.payload));
}

export async function onTerminalClosed(cb: (payload: TerminalOutput) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<TerminalOutput>("terminal-closed", (e) => cb(e.payload));
}
