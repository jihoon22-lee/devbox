import { invoke } from "@tauri-apps/api/core";
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
