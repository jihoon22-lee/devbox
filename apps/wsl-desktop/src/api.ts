import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isTauri } from "./lib/isTauri";
import type { ContainerInfo, DistroInfo } from "./types";

export interface SessionInfo {
  id: string;
  distro: string;
}

export interface TerminalOutput {
  session_id: string;
  data: string;
}

const MOCK_DISTROS: DistroInfo[] = [
  { name: "Ubuntu", version: 2, default: true },
  { name: "docker-desktop", version: 2, default: false },
];

const MOCK_CONTAINERS: ContainerInfo[] = [
  { id: "abc123", name: "postgres", image: "postgres:16", status: "Up 2 hours", ports: "0.0.0.0:5432->5432/tcp" },
  { id: "def456", name: "redis", image: "redis:7", status: "Up 2 hours", ports: "0.0.0.0:6379->6379/tcp" },
  { id: "ghi789", name: "nginx", image: "nginx:1.25", status: "Exited (0) 3 days ago", ports: "80/tcp" },
];

export async function listDistros(): Promise<DistroInfo[]> {
  if (!isTauri()) return MOCK_DISTROS;
  return invoke<DistroInfo[]>("list_distros");
}

export async function dockerPs(distro: string): Promise<ContainerInfo[]> {
  if (!isTauri()) return MOCK_CONTAINERS;
  return invoke<ContainerInfo[]>("docker_ps", { distro });
}

export async function dockerAction(distro: string, containerId: string, action: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("docker_action", { distro, containerId, action });
}

export async function startSession(distro: string, cwd?: string): Promise<string> {
  if (!isTauri()) return "mock-" + Date.now();
  return invoke<string>("start_session", { distro, cwd: cwd ?? null });
}

export async function writeSession(sessionId: string, data: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("write_session", { sessionId, data });
}

/** PTY 리더 스레드를 spawn한다. start_session이 아니라 이 호출이 방출을 시작시키므로,
 * TermPane이 registerWrite 직후 정확히 한 번 호출해야 마운트 사이의 출력이 유실되지
 * 않는다. */
export async function attachSession(sessionId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("attach_session", { sessionId });
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
