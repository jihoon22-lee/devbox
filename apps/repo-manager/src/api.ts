import { invoke } from "@tauri-apps/api/core";
import catalogJson from "../../catalog.json";
import { isTauri } from "./lib/isTauri";

export interface RepoEntry {
  path: string;
  canonicalKey: string;
  hasWorktrees: boolean;
}

export interface ScanResult {
  repos: RepoEntry[];
  /** 탐색 깊이·방문 상한에 걸려 일부 디렉터리를 건너뛰었으면 true. */
  truncated: boolean;
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

export type DependencyEcosystem = "cargo" | "pnpm" | "npm" | "python" | "gradle";
export type DependencySourceStatus =
  | "ready"
  | "missingLockfile"
  | "staleLockfile"
  | "invalid"
  | "unsupported";

export interface DependencySource {
  ecosystem: DependencyEcosystem;
  path: string;
  status: DependencySourceStatus;
  manifestCount: number;
  lockfileCount: number;
  packageCount: number;
  directCount: number;
}

export interface DependencyPackage {
  id: string;
  ecosystem: DependencyEcosystem;
  name: string;
  version: string;
  direct: boolean;
  dependencies: string[];
}

export interface DuplicateDependency {
  ecosystem: DependencyEcosystem;
  name: string;
  versions: string[];
}

export interface DependencyReport {
  revision: string;
  sources: DependencySource[];
  packages: DependencyPackage[];
  duplicates: DuplicateDependency[];
  packageCount: number;
  directCount: number;
  transitiveCount: number;
  unresolvedDependencyCount: number;
  missingLockfileCount: number;
  staleLockfileCount: number;
  unsupportedCount: number;
  invalidCount: number;
  truncated: boolean;
  summaryPublished: boolean;
}

export const DEPENDENCY_LENS_ERROR = "Dependency Lens 분석을 완료하지 못했습니다.";
export const DEPENDENCY_ENRICHMENT_ERROR = "Dependency Lens 원격 정보를 불러오지 못했습니다.";
export const DEPENDENCY_ENRICHMENT_BUSY = "다른 Dependency Lens 분석 또는 원격 조회가 진행 중입니다.";
export const DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED = "전송 내용을 다시 검토해 주세요.";

export type EnrichmentService = "osv" | "depsDev";
export type EnrichmentValueState = "fresh" | "cached" | "stale" | "failed" | "notRequested";

export interface EnrichmentSelection {
  osv: boolean;
  depsDev: boolean;
}

export interface EnrichmentCoordinatePreview {
  ecosystem: string;
  name: string;
  version: string;
  direct: boolean;
  localPackageCount: number;
}

export interface EnrichmentServicePreview {
  service: EnrichmentService;
  host: string;
  transmitted: EnrichmentCoordinatePreview[];
  cachedCount: number;
  staleFallbackCount: number;
  omittedCount: number;
  requestCount: number;
}

export interface DependencyEnrichmentPreview {
  token: string;
  revision: string;
  expiresAtMs: number;
  services: EnrichmentServicePreview[];
  localPackageCount: number;
}

export interface OsvEnrichmentValue {
  state: EnrichmentValueState;
  fetchedAtMs: number | null;
  ageMs: number | null;
  advisoryIds: string[];
  truncated: boolean;
}

export interface DepsDevEnrichmentValue {
  state: EnrichmentValueState;
  fetchedAtMs: number | null;
  ageMs: number | null;
  licenses: string[];
  defaultVersion: string | null;
  deprecated: boolean;
  advisoryIds: string[];
  versionFound: boolean;
  packageFound: boolean;
}

export interface DependencyEnrichmentEntry {
  packageIds: string[];
  osv: OsvEnrichmentValue;
  depsDev: DepsDevEnrichmentValue;
}

export interface EnrichmentServiceSummary {
  service: EnrichmentService;
  targetCount: number;
  transmittedCount: number;
  cachedCount: number;
  staleCount: number;
  failedCount: number;
  omittedCount: number;
}

export interface DependencyEnrichmentReport {
  revision: string;
  completedAtMs: number;
  localAuthoritative: boolean;
  cachePersisted: boolean;
  entries: DependencyEnrichmentEntry[];
  services: EnrichmentServiceSummary[];
}

export type GitSafetyIssue =
  | "dirty"
  | "detached"
  | "noUpstream"
  | "diverged"
  | "rebaseInProgress"
  | "mergeInProgress";

export interface GitSafetySnapshot {
  branch: string;
  upstream: string | null;
  ahead: number;
  behind: number;
  dirty: boolean;
  detached: boolean;
  noUpstream: boolean;
  diverged: boolean;
  rebaseInProgress: boolean;
  mergeInProgress: boolean;
  safe: boolean;
  issues: GitSafetyIssue[];
}

export const GIT_VIEW_ERROR = "Git history 또는 diff를 불러올 수 없습니다.";
export const GIT_SAFETY_ERROR = "Git 상태를 확인하지 못했습니다.";

export interface CommitSummary {
  id: string;
  shortId: string;
  parents: string[];
  authoredAt: string;
  author: string;
  authorEmail: string;
  subject: string;
}

export interface HistoryResult {
  entries: CommitSummary[];
  hasMore: boolean;
}

export interface CommitDetail {
  id: string;
  parents: string[];
  authoredAt: string;
  author: string;
  authorEmail: string;
  subject: string;
  body: string;
}

export interface DiffFile {
  path: string;
  oldPath: string | null;
  status: "modified" | "added" | "deleted" | "renamed";
  binary: boolean;
  patch: string;
  truncated: boolean;
}

export interface DiffResult {
  scope: "workingTree" | "commit";
  commitId: string | null;
  files: DiffFile[];
  truncated: boolean;
}

export type ChangeKind = "modified" | "added" | "deleted" | "renamed" | "copied" | "untracked" | "conflict";

export interface ChangeEntry {
  path: string;
  oldPath: string | null;
  indexStatus: string;
  worktreeStatus: string;
  kind: ChangeKind;
  staged: boolean;
  unstaged: boolean;
}

export const GIT_MUTATION_ERROR = "Git 변경 사항을 적용하지 못했습니다.";

export const GIT_REMOTE_ERROR = "Git 원격 작업을 실행하지 못했습니다.";
export const GIT_REMOTE_CANCELLED = "Git 원격 작업을 취소했습니다.";
export const GIT_REMOTE_BUSY = "이미 다른 Git 작업이 진행 중입니다.";
export const GIT_REMOTE_STATE_CHANGED = "저장소 상태가 변경되어 Git 원격 작업을 실행하지 않았습니다.";

export interface RemoteState {
  currentBranch: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  dirty: boolean;
  detached: boolean;
  diverged: boolean;
  operationInProgress: boolean;
}

export interface RepoOpenTarget {
  id: string;
  displayName: string;
  payloadKind: "path" | "workspace";
}

export type OpenTarget =
  | { kind: "path"; path: string; line: number | null; column: number | null }
  | { kind: "profile"; id: string }
  | { kind: "workspace"; path: string }
  | { kind: "query"; text: string }
  | { kind: "task"; id: string }
  | { kind: "install"; appId: string };

export interface OpenRequest {
  target: OpenTarget;
  from: string | null;
}

const MOCK_RESULT: ScanResult = {
  repos: [{ path: "C:\\projects\\devbox", canonicalKey: "win:c:/projects/devbox", hasWorktrees: true }],
  truncated: false,
};

const MOCK_HISTORY: HistoryResult = {
  entries: [
    {
      id: "0123456789abcdef0123456789abcdef01234567",
      shortId: "0123456789ab",
      parents: [],
      authoredAt: "2026-08-27T09:00:00+09:00",
      author: "devbox fixture",
      authorEmail: "fixture@example.test",
      subject: "Initial repository fixture",
    },
  ],
  hasMore: false,
};

const MOCK_DETAIL: CommitDetail = {
  ...MOCK_HISTORY.entries[0],
  body: "Initial repository fixture\n",
};

const MOCK_DIFF: DiffResult = {
  scope: "workingTree",
  commitId: null,
  files: [],
  truncated: false,
};

const MOCK_SAFETY: GitSafetySnapshot = {
  branch: "main",
  upstream: "origin/main",
  ahead: 0,
  behind: 0,
  dirty: false,
  detached: false,
  noUpstream: false,
  diverged: false,
  rebaseInProgress: false,
  mergeInProgress: false,
  safe: true,
  issues: [],
};

const MOCK_CHANGES: ChangeEntry[] = [
  {
    path: "src/App.tsx",
    oldPath: null,
    indexStatus: " ",
    worktreeStatus: "M",
    kind: "modified",
    staged: false,
    unstaged: true,
  },
];

const MOCK_REMOTE_STATE: RemoteState = {
  currentBranch: "main",
  upstream: "origin/main",
  ahead: 0,
  behind: 0,
  dirty: false,
  detached: false,
  diverged: false,
  operationInProgress: false,
};

const MOCK_DEPENDENCY_REPORT: DependencyReport = {
  revision: `sha256:${"a".repeat(64)}`,
  sources: [
    {
      ecosystem: "cargo",
      path: "Cargo.lock",
      status: "ready",
      manifestCount: 1,
      lockfileCount: 1,
      packageCount: 2,
      directCount: 1,
    },
  ],
  packages: [
    {
      id: "cargo:serde@1.0.0",
      ecosystem: "cargo",
      name: "serde",
      version: "1.0.0",
      direct: true,
      dependencies: ["cargo:serde-core@1.0.0"],
    },
    {
      id: "cargo:serde-core@1.0.0",
      ecosystem: "cargo",
      name: "serde-core",
      version: "1.0.0",
      direct: false,
      dependencies: [],
    },
  ],
  duplicates: [],
  packageCount: 2,
  directCount: 1,
  transitiveCount: 1,
  unresolvedDependencyCount: 0,
  missingLockfileCount: 0,
  staleLockfileCount: 0,
  unsupportedCount: 0,
  invalidCount: 0,
  truncated: false,
  summaryPublished: true,
};

const MOCK_ENRICHMENT_PREVIEW: DependencyEnrichmentPreview = {
  token: "b".repeat(64),
  revision: MOCK_DEPENDENCY_REPORT.revision,
  expiresAtMs: Date.now() + 5 * 60 * 1_000,
  localPackageCount: MOCK_DEPENDENCY_REPORT.packageCount,
  services: [
    {
      service: "osv",
      host: "api.osv.dev",
      transmitted: [{
        ecosystem: "crates.io",
        name: "serde",
        version: "1.0.0",
        direct: true,
        localPackageCount: 1,
      }],
      cachedCount: 0,
      staleFallbackCount: 0,
      omittedCount: 0,
      requestCount: 1,
    },
    {
      service: "depsDev",
      host: "api.deps.dev",
      transmitted: [{
        ecosystem: "CARGO",
        name: "serde",
        version: "1.0.0",
        direct: true,
        localPackageCount: 1,
      }],
      cachedCount: 0,
      staleFallbackCount: 0,
      omittedCount: 0,
      requestCount: 2,
    },
  ],
};

const MOCK_ENRICHMENT_REPORT: DependencyEnrichmentReport = {
  revision: MOCK_DEPENDENCY_REPORT.revision,
  completedAtMs: Date.now(),
  localAuthoritative: true,
  cachePersisted: true,
  entries: [{
    packageIds: ["cargo:serde@1.0.0"],
    osv: {
      state: "fresh",
      fetchedAtMs: Date.now(),
      ageMs: 0,
      advisoryIds: [],
      truncated: false,
    },
    depsDev: {
      state: "fresh",
      fetchedAtMs: Date.now(),
      ageMs: 0,
      licenses: ["MIT OR Apache-2.0"],
      defaultVersion: "1.0.0",
      deprecated: false,
      advisoryIds: [],
      versionFound: true,
      packageFound: true,
    },
  }],
  services: [
    {
      service: "osv",
      targetCount: 1,
      transmittedCount: 1,
      cachedCount: 0,
      staleCount: 0,
      failedCount: 0,
      omittedCount: 0,
    },
    {
      service: "depsDev",
      targetCount: 1,
      transmittedCount: 1,
      cachedCount: 0,
      staleCount: 0,
      failedCount: 0,
      omittedCount: 0,
    },
  ],
};

const MOCK_CATALOG_APPS = catalogJson.apps as Array<{
  id: string;
  displayName: string;
  accepts: string[];
}>;

const MOCK_OPEN_TARGETS: RepoOpenTarget[] = MOCK_CATALOG_APPS
  .filter((app) => app.id !== "repo-manager" && app.accepts.includes("path"))
  .map((app) => ({
    id: app.id,
    displayName: app.displayName,
    payloadKind: app.accepts.includes("workspace") ? "workspace" : "path",
  }));

export function scanRoot(root: string): Promise<ScanResult> {
  if (!isTauri()) return Promise.resolve(MOCK_RESULT);
  return invoke<ScanResult>("scan_root", { root });
}

export function prepareInboundRepository(path: string): Promise<RepoEntry> {
  if (!isTauri()) {
    const normalized = path.replace(/\\/g, "/").replace(/\/+$/u, "");
    return Promise.resolve({
      path,
      canonicalKey: /^[a-zA-Z]:\//u.test(normalized)
        ? `win:${normalized.toLowerCase()}`
        : normalized,
      hasWorktrees: false,
    });
  }
  return invoke<RepoEntry>("prepare_inbound_repository", { path });
}

export async function takePendingOpen(): Promise<OpenRequest | null> {
  if (!isTauri()) return null;
  return invoke<OpenRequest | null>("take_pending_open");
}

export async function onOpenRequest(cb: (request: OpenRequest) => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen<OpenRequest>("devbox://open", (event) => cb(event.payload));
}

export function repoStatus(path: string): Promise<RepoSnapshot> {
  if (!isTauri()) {
    return Promise.resolve({ path, branch: { current: "main", ahead: 0, behind: 0, dirty: false, detached: false }, changes: 0 });
  }
  return invoke<RepoSnapshot>("repo_status", { path });
}

export function repoPreflight(path: string): Promise<GitSafetySnapshot> {
  if (!isTauri()) return Promise.resolve({ ...MOCK_SAFETY });
  return invoke<GitSafetySnapshot>("repo_preflight", { request: { path } });
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

export const GIT_CLEANUP_ERROR = "Git 정리 작업을 실행하지 못했습니다.";
export const GIT_CLEANUP_CANCELLED = "Git 정리 작업을 취소했습니다.";
export const GIT_CLEANUP_BUSY = "이미 다른 Git 작업이 진행 중입니다.";
export const GIT_CLEANUP_STATE_CHANGED = "저장소 상태가 변경되어 Git 정리를 실행하지 않았습니다.";

export interface BranchCleanupEntry {
  name: string;
  head: string;
  upstream: string | null;
  lastCommitUnix: number;
  current: boolean;
  checkedOut: boolean;
  protected: boolean;
  merged: boolean;
  stale: boolean;
  candidate: boolean;
  eligible: boolean;
  reasons: string[];
  blocked: string[];
}

export interface WorktreeCleanupEntry {
  path: string;
  head: string | null;
  branch: string | null;
  isMain: boolean;
  bare: boolean;
  locked: boolean;
  prunable: boolean;
  dirty: boolean;
  untracked: boolean;
  ignored: boolean;
  candidate: boolean;
  eligible: boolean;
  reasons: string[];
  blocked: string[];
}

export interface CleanupPreview {
  revision: string;
  currentBranch: string | null;
  currentHead: string | null;
  branches: BranchCleanupEntry[];
  worktrees: WorktreeCleanupEntry[];
}

export interface CleanupItemResult {
  kind: "branch" | "worktree";
  target: string;
  outcome: "removed" | "blocked" | "failed";
  reason: string | null;
}

export interface CleanupResult {
  previewRevision: string;
  attempted: number;
  removed: number;
  items: CleanupItemResult[];
}

const MOCK_CLEANUP_PREVIEW: CleanupPreview = {
  revision: "cleanup-0123456789abcdef",
  currentBranch: "main",
  currentHead: "0123456789abcdef0123456789abcdef01234567",
  branches: [
    {
      name: "main",
      head: "0123456789abcdef0123456789abcdef01234567",
      upstream: "origin/main",
      lastCommitUnix: 0,
      current: true,
      checkedOut: true,
      protected: true,
      merged: true,
      stale: false,
      candidate: true,
      eligible: false,
      reasons: ["mergedIntoCurrent"],
      blocked: ["currentBranch", "mainBranch", "checkedOut"],
    },
  ],
  worktrees: [
    {
      path: "C:\\projects\\devbox",
      head: "0123456789abcdef0123456789abcdef01234567",
      branch: "main",
      isMain: true,
      bare: false,
      locked: false,
      prunable: false,
      dirty: false,
      untracked: false,
      ignored: false,
      candidate: false,
      eligible: false,
      reasons: ["primaryWorktree"],
      blocked: ["mainWorktree", "currentWorktree"],
    },
  ],
};

export function repoCleanupPreview(path: string, operationId: string): Promise<CleanupPreview> {
  if (!isTauri()) return Promise.resolve({ ...MOCK_CLEANUP_PREVIEW });
  return invoke<CleanupPreview>("repo_cleanup_preview", { request: { path, operationId } });
}

export function repoCleanup(
  path: string,
  branchNames: string[],
  worktreePaths: string[],
  previewRevision: string,
  operationId: string,
): Promise<CleanupResult> {
  if (!isTauri()) {
    return Promise.resolve({
      previewRevision,
      attempted: branchNames.length + worktreePaths.length,
      removed: branchNames.length + worktreePaths.length,
      items: [
        ...branchNames.map((target) => ({ kind: "branch" as const, target, outcome: "removed" as const, reason: null })),
        ...worktreePaths.map((target) => ({ kind: "worktree" as const, target, outcome: "removed" as const, reason: null })),
      ],
    });
  }
  return invoke<CleanupResult>("repo_cleanup", {
    request: { path, branchNames, worktreePaths, previewRevision, operationId },
  });
}

export function repoCleanupCancel(operationId: string): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("repo_cleanup_cancel", { request: { operationId } });
}

export function repoHistory(path: string, limit: number): Promise<HistoryResult> {
  if (!isTauri()) {
    return Promise.resolve({
      entries: MOCK_HISTORY.entries.slice(0, limit),
      hasMore: MOCK_HISTORY.entries.length > limit,
    });
  }
  return invoke<HistoryResult>("repo_history", { request: { path, limit } });
}

export function repoCommitDetail(path: string, commitId: string): Promise<CommitDetail> {
  if (!isTauri()) return Promise.resolve(MOCK_DETAIL);
  return invoke<CommitDetail>("repo_commit_detail", { request: { path, commitId } });
}

export function repoDiff(path: string, commitId: string | null): Promise<DiffResult> {
  if (!isTauri()) {
    return Promise.resolve({ ...MOCK_DIFF, commitId, scope: commitId ? "commit" : "workingTree" });
  }
  return invoke<DiffResult>("repo_diff", { request: { path, commitId } });
}

export function dependencyInventory(path: string): Promise<DependencyReport> {
  if (!isTauri()) {
    return Promise.resolve({
      ...MOCK_DEPENDENCY_REPORT,
      sources: MOCK_DEPENDENCY_REPORT.sources.map((source) => ({ ...source })),
      packages: MOCK_DEPENDENCY_REPORT.packages.map((dependency) => ({
        ...dependency,
        dependencies: [...dependency.dependencies],
      })),
      duplicates: MOCK_DEPENDENCY_REPORT.duplicates.map((duplicate) => ({
        ...duplicate,
        versions: [...duplicate.versions],
      })),
    });
  }
  return invoke<DependencyReport>("dependency_inventory", { request: { path } });
}

export function dependencyEnrichmentPreview(
  path: string,
  services: EnrichmentSelection,
  forceRefresh: boolean,
): Promise<DependencyEnrichmentPreview> {
  if (!isTauri()) {
    return Promise.resolve({
      ...MOCK_ENRICHMENT_PREVIEW,
      expiresAtMs: Date.now() + 5 * 60 * 1_000,
      services: MOCK_ENRICHMENT_PREVIEW.services
        .filter((service) => service.service === "osv" ? services.osv : services.depsDev)
        .map((service) => ({
          ...service,
          transmitted: service.transmitted.map((coordinate) => ({ ...coordinate })),
          cachedCount: forceRefresh ? 0 : service.cachedCount,
        })),
    });
  }
  return invoke<DependencyEnrichmentPreview>("dependency_enrichment_preview", {
    request: { path, services, forceRefresh },
  });
}

export function dependencyEnrichmentExecute(
  path: string,
  previewToken: string,
): Promise<DependencyEnrichmentReport> {
  if (!isTauri()) {
    return Promise.resolve({
      ...MOCK_ENRICHMENT_REPORT,
      completedAtMs: Date.now(),
      entries: MOCK_ENRICHMENT_REPORT.entries.map((entry) => ({
        packageIds: [...entry.packageIds],
        osv: { ...entry.osv, advisoryIds: [...entry.osv.advisoryIds] },
        depsDev: {
          ...entry.depsDev,
          licenses: [...entry.depsDev.licenses],
          advisoryIds: [...entry.depsDev.advisoryIds],
        },
      })),
      services: MOCK_ENRICHMENT_REPORT.services.map((service) => ({ ...service })),
    });
  }
  return invoke<DependencyEnrichmentReport>("dependency_enrichment_execute", {
    request: { path, previewToken },
  });
}

export function repoChanges(path: string): Promise<ChangeEntry[]> {
  if (!isTauri()) return Promise.resolve(MOCK_CHANGES.map((change) => ({ ...change })));
  return invoke<ChangeEntry[]>("repo_changes", { request: { path } });
}

export function repoStage(path: string, paths: string[], operationId: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("repo_stage", { request: { path, paths, operationId } });
}

export function repoUnstage(path: string, paths: string[], operationId: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("repo_unstage", { request: { path, paths, operationId } });
}

export function repoCommit(path: string, message: string, operationId: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("repo_commit", { request: { path, message, operationId } });
}

export function repoLocalCancel(operationId: string): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("repo_local_cancel", { request: { operationId } });
}

export function repoRemoteStatus(path: string): Promise<RemoteState> {
  if (!isTauri()) return Promise.resolve({ ...MOCK_REMOTE_STATE });
  return invoke<RemoteState>("repo_remote_status", { request: { path } });
}

export function repoFetch(path: string, operationId: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("repo_fetch", { request: { path, operationId } });
}

export function repoPull(path: string, operationId: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("repo_pull", { request: { path, operationId } });
}

export function repoPush(path: string, operationId: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("repo_push", { request: { path, operationId } });
}

export function repoRemoteCancel(operationId: string): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("repo_remote_cancel", { request: { operationId } });
}

export function openTargets(): Promise<RepoOpenTarget[]> {
  if (!isTauri()) return Promise.resolve(MOCK_OPEN_TARGETS);
  return invoke<RepoOpenTarget[]>("open_targets");
}

export function openIn(appId: string, path: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("open_in", { appId, path });
}

export function repositoryCopyPath(path: string): Promise<string> {
  if (!isTauri()) return Promise.resolve(path);
  return invoke<string>("repository_copy_path", { path });
}

export function openRepositoryFolder(path: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("open_repository_folder", { path });
}
