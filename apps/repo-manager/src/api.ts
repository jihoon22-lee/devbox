import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";

export interface RepoEntry {
  path: string;
  canonicalKey: string;
  hasWorktrees: boolean;
}

export interface BranchState {
  current: string;
  ahead: number;
  behind: number;
  dirty: boolean;
  detached: boolean;
}

export interface RepoSnapshot {
  path: string;
  branch: BranchState;
  changes: number;
}

const MOCK_REPOS: RepoEntry[] = [
  { path: "C:\\projects\\devbox", canonicalKey: "win:c:/projects/devbox", hasWorktrees: true },
];

export function scanRoot(root: string): Promise<RepoEntry[]> {
  if (!isTauri()) return Promise.resolve(MOCK_REPOS);
  return invoke<RepoEntry[]>("scan_root", { root });
}

export function repoStatus(path: string): Promise<RepoSnapshot> {
  if (!isTauri()) {
    return Promise.resolve({ path, branch: { current: "main", ahead: 0, behind: 0, dirty: false, detached: false }, changes: 0 });
  }
  return invoke<RepoSnapshot>("repo_status", { path });
}

export function worktrees(path: string): Promise<string[]> {
  if (!isTauri()) return Promise.resolve(["C:\\projects\\devbox", "C:\\projects\\devbox-wt"]);
  return invoke<string[]>("worktrees", { path });
}

export function createWorktree(repoPath: string, branch: string, targetDir: string): Promise<{ path: string }> {
  if (!isTauri()) return Promise.resolve({ path: targetDir });
  return invoke<{ path: string }>("create_worktree", { repoPath, branch, targetDir });
}

export function worktreeClean(path: string): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(true);
  return invoke<boolean>("worktree_clean", { path });
}

export function openIn(appName: string, path: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("open_in", { appName, path });
}
