import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { isTauri } from "./lib/isTauri";
import type {
  ContainerInfo,
  DashboardSnapshot,
  DistroInfo,
  MultiplexerAvailability,
  MultiplexerKind,
  OpenRequest,
  WorkspaceProfile,
} from "./types";

export interface SessionInfo {
  id: string;
  distro: string;
}

export interface TerminalOutput {
  session_id: string;
  data: string;
}

const MOCK_DISTROS: DistroInfo[] = [
  { name: "Ubuntu", version: 2, default: true, state: "Running" },
  { name: "docker-desktop", version: 2, default: false, state: "Stopped" },
];

const MOCK_CONTAINERS: ContainerInfo[] = [
  { id: "abc123", name: "postgres", image: "postgres:16", status: "Up 2 hours", ports: "0.0.0.0:5432->5432/tcp" },
  { id: "def456", name: "redis", image: "redis:7", status: "Up 2 hours", ports: "0.0.0.0:6379->6379/tcp" },
  { id: "ghi789", name: "nginx", image: "nginx:1.25", status: "Exited (0) 3 days ago", ports: "80/tcp" },
];
let nextMockSession = 0;

let mockDashboardRevision = 0;

export async function getDashboardSnapshot(): Promise<DashboardSnapshot> {
  if (isTauri()) return invoke<DashboardSnapshot>("dashboard_snapshot");
  mockDashboardRevision += 1;
  return {
    revision: mockDashboardRevision,
    capturedAtMs: Date.now(),
    staleAfterMs: 30_000,
    distros: MOCK_DISTROS.map((distro) => ({
      ...distro,
      terminalCount: 0,
      dockerAvailability: distro.state.toLowerCase() === "running" ? "available" : "notQueried",
      containers: distro.default ? MOCK_CONTAINERS : [],
      resource: distro.state.toLowerCase() === "running"
        ? {
            cpuPercent: 12,
            memoryUsedBytes: 512 * 1024 * 1024,
            memoryTotalBytes: 2 * 1024 * 1024 * 1024,
            diskUsedBytes: 8 * 1024 * 1024 * 1024,
            diskTotalBytes: 64 * 1024 * 1024 * 1024,
          }
        : null,
    })),
  };
}

export async function listDistros(): Promise<DistroInfo[]> {
  if (!isTauri()) return MOCK_DISTROS;
  return invoke<DistroInfo[]>("list_distros");
}

export async function getWindowsBuildNumber(): Promise<number | null> {
  if (!isTauri()) return null;
  return invoke<number | null>("windows_build_number");
}

export async function dockerPs(distro: string): Promise<ContainerInfo[]> {
  if (!isTauri()) return MOCK_CONTAINERS;
  return invoke<ContainerInfo[]>("docker_ps", { distro });
}

export async function dockerAction(distro: string, containerId: string, action: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("docker_action", { distro, containerId, action });
}

/** Publish a validated WSL file adapter handoff. Browser preview never writes
 * a pending envelope or launches another app. */
export async function openWslFileInLogLens(distro: string, wslPath: string): Promise<void> {
  if (!isTauri()) throw new Error("Log Lens handoff는 데스크톱 앱에서만 사용할 수 있습니다");
  await invoke("open_wsl_file_in_log_lens", { distro, wslPath });
}

/** Publish a validated fixed journal adapter handoff. */
export async function openWslJournalInLogLens(distro: string, unit: string | null): Promise<void> {
  if (!isTauri()) throw new Error("Log Lens handoff는 데스크톱 앱에서만 사용할 수 있습니다");
  await invoke("open_wsl_journal_in_log_lens", { distro, unit });
}

export interface StartedSession {
  sessionId: string;
  resumed: boolean;
  multiplexer: MultiplexerKind;
}

export async function startSession(
  distro: string,
  cwd: string | undefined,
  paneKey: string,
  multiplexer: MultiplexerKind,
): Promise<StartedSession> {
  if (!isTauri()) {
    nextMockSession += 1;
    return { sessionId: `mock-${Date.now()}-${nextMockSession}`, resumed: false, multiplexer: "native" };
  }
  return invoke<StartedSession>("start_session", {
    distro,
    cwd: cwd ?? null,
    paneKey,
    multiplexer,
  });
}

export async function detectMultiplexers(distro: string): Promise<MultiplexerAvailability[]> {
  if (!isTauri()) return [
    { kind: "native", status: "available", version: null, source: null },
    { kind: "tmux", status: "missing", version: null, source: null },
    { kind: "zellij", status: "missing", version: null, source: null },
  ];
  return invoke<MultiplexerAvailability[]>("detect_multiplexers", { distro });
}

let mockProfiles: WorkspaceProfile[] = [];

export async function listWorkspaceProfiles(): Promise<WorkspaceProfile[]> {
  if (!isTauri()) return mockProfiles;
  return invoke<WorkspaceProfile[]>("list_workspace_profiles");
}

export async function saveWorkspaceProfile(profile: WorkspaceProfile): Promise<WorkspaceProfile> {
  if (!isTauri()) {
    const saved = { ...profile, id: profile.id || `profile-${Date.now()}` };
    mockProfiles = [...mockProfiles.filter((item) => item.id !== saved.id), saved];
    return saved;
  }
  return invoke<WorkspaceProfile>("save_workspace_profile", { profile });
}

export async function deleteWorkspaceProfile(id: string): Promise<void> {
  if (!isTauri()) {
    mockProfiles = mockProfiles.filter((profile) => profile.id !== id);
    return;
  }
  await invoke("delete_workspace_profile", { id });
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

/**
 * Takes (and clears) the inbound open request left by a cold-start argv parse
 * or a single-instance relaunch, if any. `null` when nothing is pending.
 * Clearing on take means a page reload does not re-trigger the same open
 * (`docs/superpowers/specs/2026-08-17-app-interop-design.md` §3).
 */
export async function takePendingOpen(): Promise<OpenRequest | null> {
  if (!isTauri()) return null;
  return invoke<OpenRequest | null>("take_pending_open");
}

/** Fired when an already-running instance is relaunched with argv (§3). */
export async function onOpenRequest(cb: (payload: OpenRequest) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<OpenRequest>("devbox://open", (e) => cb(e.payload));
}

export async function readClipboardText(): Promise<string> {
  if (!isTauri()) return navigator.clipboard.readText();
  return readText();
}

/** URL 검증은 호출자가 먼저 수행한다. 이 경계는 운영체제 기본 브라우저 실행만 소유한다. */
export async function openTerminalLink(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  await openUrl(url);
}
