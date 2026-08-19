import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isTauri } from "./lib/isTauri";

export interface ProjectProfile {
  id: string;
  name: string;
  windowsPath: string | null;
  wsl: { distro: string; path: string } | null;
  gitRoot: string | null;
  expectedPorts: number[];
  runManagerServiceIds: string[];
}

export interface HealthItem {
  name: string;
  ok: boolean;
  detail: string;
}

export interface ProjectHealth {
  profileId: string;
  items: HealthItem[];
}

export interface RunStep {
  name: string;
  ok: boolean;
  detail: string;
}

export interface WorkspaceRun {
  runId: string;
  profileId: string;
  steps: RunStep[];
  startedPids: number[];
}

const MOCK_PROFILES: ProjectProfile[] = [
  { id: "p-1", name: "devbox", windowsPath: "C:\\projects\\devbox", wsl: { distro: "Ubuntu", path: "/mnt/e/projects/devbox" }, gitRoot: "C:\\projects\\devbox", expectedPorts: [1420], runManagerServiceIds: ["devbox-dev"] },
];

export function listProfiles(): Promise<ProjectProfile[]> {
  if (!isTauri()) return Promise.resolve(MOCK_PROFILES);
  return invoke<ProjectProfile[]>("list_profiles");
}

export function createProfile(profile: ProjectProfile): Promise<ProjectProfile> {
  if (!isTauri()) return Promise.resolve(profile);
  return invoke<ProjectProfile>("create_profile", { profile });
}

export function updateProfile(profile: ProjectProfile): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("update_profile", { profile });
}

export function deleteProfile(id: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("delete_profile", { id });
}

export function projectHealth(profileId: string): Promise<ProjectHealth> {
  if (!isTauri()) {
    return Promise.resolve({
      profileId,
      items: [
        { name: "git", ok: true, detail: "C:\\projects\\devbox · main · 0 changes" },
        { name: "wsl", ok: true, detail: "Ubuntu · /mnt/e/projects/devbox" },
        { name: "ports", ok: true, detail: "전부 open: 1420" },
        { name: "services", ok: true, detail: "서비스 전부 실행 중" },
      ],
    });
  }
  return invoke<ProjectHealth>("project_health", { profileId });
}

export function startWorkspace(profileId: string): Promise<WorkspaceRun> {
  if (!isTauri()) {
    return Promise.resolve({ runId: "r-1", profileId, steps: [], startedPids: [1] });
  }
  return invoke<WorkspaceRun>("start_workspace", { profileId });
}

export function stopWorkspace(runId: string): Promise<number> {
  if (!isTauri()) return Promise.resolve(0);
  return invoke<number>("stop_workspace", { runId });
}

// ── applink — inbound cross-app open requests ───────────────────────
// docs/superpowers/specs/2026-08-17-app-interop-design.md §1.2/§3

/**
 * `Option` fields serialize as `null`, never omitted, so every optional field
 * here is typed `| null` rather than `?:` (matches `crates/applink::OpenTarget`).
 */
export type OpenTarget =
  | { kind: "path"; path: string; line: number | null; column: number | null }
  | { kind: "profile"; id: string }
  | { kind: "workspace"; path: string }
  | { kind: "query"; text: string };

export interface OpenRequest {
  target: OpenTarget;
  from: string | null;
}

/**
 * Takes (and clears) the inbound open request left by a cold-start argv parse
 * or a single-instance relaunch, if any. `null` when nothing is pending.
 * Clearing on take means a page reload does not re-trigger the same open (§3).
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
