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

export interface WorkspaceRunOwnership {
  runId: string;
  profileId: string;
}

export interface WorkbenchOpenTarget {
  id: string;
  displayName: string;
  payloadKind: "path" | "workspace";
}

export type RuntimeSuggestionStatus = "fresh" | "stale" | "expired" | "missing" | "corrupt";

export interface RuntimePortSource {
  distro: string;
  container: string;
  containerState: string;
  target: number;
  protocol: "tcp";
}

export interface RuntimePortSuggestion {
  published: number;
  sources: RuntimePortSource[];
}

export interface RuntimeSuggestions {
  source: string;
  status: RuntimeSuggestionStatus;
  producerVersion: string | null;
  freshnessMs: number | null;
  ports: RuntimePortSuggestion[];
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

export function stopWorkspace(runId: string, profileId: string): Promise<number> {
  if (!isTauri()) return Promise.resolve(0);
  return invoke<number>("stop_workspace", { runId, profileId });
}

export function currentWorkspaceRun(): Promise<WorkspaceRunOwnership | null> {
  if (!isTauri()) return Promise.resolve(null);
  return invoke<WorkspaceRunOwnership | null>("current_workspace_run");
}

/** Reads WSL Desktop's versioned snapshot. It never invokes WSL or Docker. */
export function wslRuntimeSuggestions(): Promise<RuntimeSuggestions> {
  if (!isTauri()) {
    return Promise.resolve({
      source: "WSL Desktop runtime/v1",
      status: "fresh",
      producerVersion: "preview",
      freshnessMs: 0,
      ports: [
        {
          published: 3000,
          sources: [{
            distro: "Ubuntu",
            container: "web",
            containerState: "running",
            target: 3000,
            protocol: "tcp",
          }],
        },
      ],
    });
  }
  return invoke<RuntimeSuggestions>("wsl_runtime_suggestions");
}

export function profileOpenTargets(profileId: string): Promise<WorkbenchOpenTarget[]> {
  if (!isTauri()) {
    return Promise.resolve([
      { id: "code-pad", displayName: "Code Pad", payloadKind: "workspace" },
      { id: "wsl-desktop", displayName: "WSL Desktop", payloadKind: "path" },
    ]);
  }
  return invoke<WorkbenchOpenTarget[]>("profile_open_targets", { profileId });
}

export function profileCopyPath(profileId: string): Promise<string> {
  if (!isTauri()) {
    const profile = MOCK_PROFILES.find((candidate) => candidate.id === profileId);
    const path = profile?.windowsPath ?? profile?.wsl?.path;
    return path ? Promise.resolve(path) : Promise.reject(new Error("프로필 경로가 없습니다"));
  }
  return invoke<string>("profile_copy_path", { profileId });
}

export function openProfileIn(profileId: string, appId: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("open_profile_in", { profileId, appId });
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
