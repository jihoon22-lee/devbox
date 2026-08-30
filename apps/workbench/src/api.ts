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
  environment: ProjectEnvironment | null;
}

/** Safe, project-independent defaults stored by Workbench's template CRUD. */
export interface ProfileTemplate {
  id: string;
  name: string;
  windowsPath: string | null;
  wsl: { distro: string; path: string } | null;
  gitRoot: string | null;
  expectedPorts: number[];
  runManagerServiceIds: string[];
}

/** A template read snapshot and its opaque native CAS revision. */
export interface ProfileTemplateSnapshot {
  revision: string;
  templates: ProfileTemplate[];
}

export type EnvironmentConflict = "none" | "duplicate" | "reserved" | "duplicateAndReserved";

export interface SecretReference {
  kind: "secret-ref/v1";
  name: string;
}

export interface EnvironmentVariableMetadata {
  name: string;
  source: string;
  conflict: EnvironmentConflict;
  secretReference: SecretReference | null;
}

export interface ProjectEnvironment {
  enabled: boolean;
  source: string;
  revision: string;
  variables: EnvironmentVariableMetadata[];
}

export interface EnvironmentVariablePreview extends EnvironmentVariableMetadata {
  maskedValue: string;
}

export interface ProjectEnvironmentPreview {
  source: string;
  revision: string;
  variables: EnvironmentVariablePreview[];
  hasConflicts: boolean;
}

export interface ProjectEnvironmentPreviewRequest {
  windowsPath: string | null;
  wsl: { distro: string; path: string } | null;
  source: string;
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

export type PreflightStatus = "pass" | "warning" | "failure" | "unavailable";
export type ResourceState =
  | "available"
  | "existing"
  | "workbenchStarted"
  | "notRunning"
  | "missing"
  | "conflict"
  | "unsafe"
  | "unavailable";

export interface ResourceProvenance {
  kind: string;
  id: string;
  state: ResourceState;
}

export interface PreflightItem {
  key: string;
  status: PreflightStatus;
  detail: string;
  resources: ResourceProvenance[];
}

export interface WorkspacePreflight {
  profileId: string;
  ready: boolean;
  items: PreflightItem[];
}

export interface RunStep {
  name: string;
  ok: boolean;
  detail: string;
  status: PreflightStatus;
}

export interface WorkspaceRun {
  runId: string;
  profileId: string;
  steps: RunStep[];
  resourceProvenance: ResourceProvenance[];
  retryCount?: number;
  canRetry?: boolean;
  failedStep?: string | null;
}

export interface WorkspaceRunOwnership {
  runId: string;
  profileId: string;
  retryCount?: number;
  canRetry?: boolean;
  failedStep?: string | null;
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

export type PackageDependencyStatus = "fresh" | "stale" | "expired" | "missing" | "corrupt";

export interface PackageDependencyEcosystem {
  ecosystem: "cargo" | "pnpm" | "npm" | "python" | "gradle";
  packageCount: number;
  directCount: number;
  duplicateCount: number;
}

/** Privacy-safe aggregate published by Repo Manager dependency-summary/v1. */
export interface PackageDependencySummary {
  profileId: string;
  source: string;
  status: PackageDependencyStatus;
  producerVersion: string | null;
  freshnessMs: number | null;
  revision: string | null;
  packageCount: number;
  directCount: number;
  transitiveCount: number;
  duplicateCount: number;
  unresolvedDependencyCount: number;
  missingLockfileCount: number;
  staleLockfileCount: number;
  unsupportedCount: number;
  invalidCount: number;
  truncated: boolean;
  ecosystems: PackageDependencyEcosystem[];
}

export type WorkspaceTaskKind = "process" | "shell";

/** Read-only Run Manager snapshot used by Workbench's task-control panel. */
export interface WorkspaceTaskControl {
  id: string;
  label: string;
  revision: string;
  taskKind: WorkspaceTaskKind;
  trusted: boolean;
  shellTrusted: boolean;
  available: boolean;
  hasDependencies: boolean;
  operationActive: boolean;
}

export type WorkspaceTaskControlAction = "start" | "stop";

export interface WorkspaceTaskControlDispatch {
  requestId: string;
  handoffId: string;
}

export type WorkspaceTaskControlReceiptStatus =
  | "accepted"
  | "rejected"
  | "started"
  | "stopped"
  | "failed";

export interface WorkspaceTaskControlReceipt {
  schemaVersion: number;
  requestId: string;
  taskId: string;
  action: WorkspaceTaskControlAction;
  status: WorkspaceTaskControlReceiptStatus;
  operationId?: string | null;
  failureCode?: string | null;
  createdAt: number;
  updatedAt: number;
}

const MOCK_PROFILES: ProjectProfile[] = [
  { id: "p-1", name: "devbox", windowsPath: "C:\\projects\\devbox", wsl: { distro: "Ubuntu", path: "/mnt/e/projects/devbox" }, gitRoot: "C:\\projects\\devbox", expectedPorts: [1420], runManagerServiceIds: ["devbox-dev"], environment: null },
];

export function listProfiles(): Promise<ProjectProfile[]> {
  if (!isTauri()) return Promise.resolve(MOCK_PROFILES);
  return invoke<ProjectProfile[]>("list_profiles");
}

export function createProfile(profile: ProjectProfile): Promise<ProjectProfile> {
  if (!isTauri()) return Promise.resolve(profile);
  return invoke<ProjectProfile>("create_profile", { profile });
}

export function listProfileTemplates(): Promise<ProfileTemplateSnapshot> {
  if (!isTauri()) return Promise.resolve({ revision: "", templates: [] });
  return invoke<ProfileTemplateSnapshot>("list_profile_templates");
}

export function createProfileTemplate(template: ProfileTemplate): Promise<ProfileTemplate> {
  if (!isTauri()) return Promise.resolve(template);
  return invoke<ProfileTemplate>("create_profile_template", { template });
}

export function updateProfileTemplate(template: ProfileTemplate, expectedRevision: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("update_profile_template", {
    request: { template, expectedRevision },
  });
}

export function deleteProfileTemplate(id: string, expectedRevision: string): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("delete_profile_template", {
    request: { id, expectedRevision },
  });
}

export function createProfileFromTemplate(
  templateId: string | null,
  profile: ProjectProfile,
): Promise<ProjectProfile> {
  if (!isTauri()) return Promise.resolve(profile);
  return invoke<ProjectProfile>("create_profile_from_template", {
    request: { templateId, profile },
  });
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
  const requestId = createOperationRequestId("health");
  activeProjectHealthRequest = { profileId, requestId };
  return invoke<ProjectHealth>("project_health", { profileId, requestId }).finally(() => {
    if (activeProjectHealthRequest?.requestId === requestId) activeProjectHealthRequest = null;
  });
}

/** Cancels the exact health request when navigation leaves its profile. */
export function cancelProjectHealth(profileId: string): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);
  const request = activeProjectHealthRequest;
  if (!request || request.profileId !== profileId) return Promise.resolve(false);
  return invoke<boolean>("cancel_project_health", {
    profileId,
    requestId: request.requestId,
  });
}

function createOperationRequestId(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

let activeProjectHealthRequest: { profileId: string; requestId: string } | null = null;
let activeWorkspacePreflightRequest: { profileId: string; requestId: string } | null = null;
let activeDependencyHealthRequest: { profileId: string; requestId: string } | null = null;

export function workspacePreflight(profileId: string): Promise<WorkspacePreflight> {
  if (!isTauri()) {
    return Promise.resolve({
      profileId,
      ready: true,
      items: [
        {
          key: "required-apps",
          status: "pass",
          detail: "필수 devbox 앱을 사용할 수 있습니다",
          resources: [
            { kind: "app", id: "wsl-desktop:path", state: "available" },
            { kind: "app", id: "code-pad:workspace", state: "available" },
          ],
        },
        {
          key: "wsl-distro",
          status: "pass",
          detail: "설정한 WSL 배포판을 사용할 수 있습니다",
          resources: [{ kind: "distro", id: "wsl-distro", state: "available" }],
        },
        {
          key: "working-directory",
          status: "pass",
          detail: "Workspace 작업 디렉터리를 사용할 수 있습니다",
          resources: [{ kind: "directory", id: "workspace-1", state: "available" }],
        },
        {
          key: "ports",
          status: "pass",
          detail: "예상 TCP port를 사용할 수 있습니다",
          resources: [],
        },
        {
          key: "service-dependencies",
          status: "pass",
          detail: "필요한 Run Manager service가 실행 중입니다",
          resources: [],
        },
      ],
    });
  }
  const requestId = createOperationRequestId("preflight");
  activeWorkspacePreflightRequest = { profileId, requestId };
  return invoke<WorkspacePreflight>("workspace_preflight", { profileId, requestId }).finally(() => {
    if (activeWorkspacePreflightRequest?.requestId === requestId) {
      activeWorkspacePreflightRequest = null;
    }
  });
}

/** Read-only dependency health shares the preflight DTO and provenance. */
export function dependencyHealth(profileId: string): Promise<WorkspacePreflight> {
  if (!isTauri()) return workspacePreflight(profileId);
  const requestId = createOperationRequestId("dependency-health");
  activeDependencyHealthRequest = { profileId, requestId };
  return invoke<WorkspacePreflight>("dependency_health", { profileId, requestId }).finally(() => {
    if (activeDependencyHealthRequest?.requestId === requestId) {
      activeDependencyHealthRequest = null;
    }
  });
}

/** Reads Repo Manager's aggregate snapshot without scanning the project. */
export function packageDependencySummary(profileId: string): Promise<PackageDependencySummary> {
  if (!isTauri()) {
    return Promise.resolve({
      profileId,
      source: "Repo Manager dependency-summary/v1",
      status: "missing",
      producerVersion: null,
      freshnessMs: null,
      revision: null,
      packageCount: 0,
      directCount: 0,
      transitiveCount: 0,
      duplicateCount: 0,
      unresolvedDependencyCount: 0,
      missingLockfileCount: 0,
      staleLockfileCount: 0,
      unsupportedCount: 0,
      invalidCount: 0,
      truncated: false,
      ecosystems: [],
    });
  }
  return invoke<PackageDependencySummary>("package_dependency_summary", { profileId });
}

/** Cancels the exact preflight request currently owned by this renderer. */
export function cancelWorkspacePreflight(profileId: string): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);
  const request = activeWorkspacePreflightRequest;
  if (!request || request.profileId !== profileId) return Promise.resolve(false);
  return invoke<boolean>("cancel_workspace_preflight", {
    profileId,
    requestId: request.requestId,
  });
}

/** Cancels the exact dependency-health request currently owned by this renderer. */
export function cancelDependencyHealth(profileId: string): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);
  const request = activeDependencyHealthRequest;
  if (!request || request.profileId !== profileId) return Promise.resolve(false);
  return invoke<boolean>("cancel_dependency_health", {
    profileId,
    requestId: request.requestId,
  });
}

export function startWorkspace(profileId: string): Promise<WorkspaceRun> {
  if (!isTauri()) {
    return Promise.resolve({ runId: "r-1", profileId, steps: [], resourceProvenance: [] });
  }
  return invoke<WorkspaceRun>("start_workspace", { profileId });
}

/** Retry only the first failed bounded step and its unfinished suffix. */
export function retryWorkspace(runId: string, profileId: string): Promise<WorkspaceRun> {
  if (!isTauri()) {
    return Promise.resolve({
      runId,
      profileId,
      steps: [],
      resourceProvenance: [],
      retryCount: 1,
      canRetry: false,
      failedStep: null,
    });
  }
  return invoke<WorkspaceRun>("retry_workspace", { runId, profileId });
}

/** Cancels the backend transition and its native child/git work. */
export function cancelStartWorkspace(profileId: string): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("cancel_start_workspace", { profileId });
}

export function stopWorkspace(runId: string, profileId: string): Promise<number> {
  if (!isTauri()) return Promise.resolve(0);
  return invoke<number>("stop_workspace", { runId, profileId });
}

export function currentWorkspaceRun(): Promise<WorkspaceRunOwnership | null> {
  if (!isTauri()) return Promise.resolve(null);
  return invoke<WorkspaceRunOwnership | null>("current_workspace_run");
}

/**
 * Reads and parses one project-relative `.env` source in native code. There
 * is no browser approximation: a browser caller cannot inspect the local
 * project file and therefore receives a rejection instead of a fabricated
 * successful preview.
 */
export function previewProjectEnvironment(
  request: ProjectEnvironmentPreviewRequest,
): Promise<ProjectEnvironmentPreview> {
  if (!isTauri()) return Promise.reject(new Error("native preview unavailable"));
  const requestId = createOperationRequestId("preview");
  activeEnvironmentPreviewRequestId = requestId;
  return invoke<ProjectEnvironmentPreview>("preview_project_environment", {
    request: { ...request, requestId },
  }).finally(() => {
    if (activeEnvironmentPreviewRequestId === requestId) activeEnvironmentPreviewRequestId = null;
  });
}

let activeEnvironmentPreviewRequestId: string | null = null;

/** Cancels one native preview request currently owned by this renderer. */
export function cancelProjectEnvironment(requestId = activeEnvironmentPreviewRequestId): Promise<boolean> {
  if (!isTauri() || !requestId) return Promise.resolve(false);
  return invoke<boolean>("cancel_project_environment", { requestId });
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

// ── Run Manager task control ───────────────────────────────────────
// Workbench receives only the privacy-safe task snapshot and opaque handoff
// correlators. Commands, paths, environment values, and process identifiers
// remain owned by Run Manager.

const MOCK_WORKSPACE_TASK_CONTROLS: WorkspaceTaskControl[] = [
  {
    id: "mock-build",
    label: "Build",
    revision: "b".repeat(64),
    taskKind: "process",
    trusted: true,
    shellTrusted: false,
    available: true,
    hasDependencies: false,
    operationActive: false,
  },
];

export function listWorkspaceTaskControls(): Promise<WorkspaceTaskControl[]> {
  if (!isTauri()) return Promise.resolve(MOCK_WORKSPACE_TASK_CONTROLS.map((task) => ({ ...task })));
  return invoke<WorkspaceTaskControl[]>("list_workspace_task_controls");
}

export interface DispatchWorkspaceTaskControlRequest {
  taskId: string;
  action: WorkspaceTaskControlAction;
  expectedRevision: string;
}

export function dispatchWorkspaceTaskControl(
  request: DispatchWorkspaceTaskControlRequest,
): Promise<WorkspaceTaskControlDispatch> {
  if (!isTauri()) {
    return Promise.resolve({
      requestId: `mock-task-control-${Date.now().toString(36)}`,
      handoffId: "mock-handoff",
    });
  }
  return invoke<WorkspaceTaskControlDispatch>("dispatch_workspace_task_control", { ...request });
}

export function getWorkspaceTaskControlReceipt(
  requestId: string,
): Promise<WorkspaceTaskControlReceipt | null> {
  if (!isTauri()) return Promise.resolve(null);
  return invoke<WorkspaceTaskControlReceipt | null>("get_workspace_task_control_receipt", { requestId });
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
  | { kind: "query"; text: string }
  | { kind: "task"; id: string }
  | { kind: "install"; appId: string };

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
