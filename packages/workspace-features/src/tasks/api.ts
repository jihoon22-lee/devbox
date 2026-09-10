import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isTauri } from "./lib/isTauri";
import type {
  CronPreviewItem,
  Job,
  JobInput,
  LogSearchOptions,
  LogSearchResponse,
  LogStream,
  ServiceInput,
  ServiceInstance,
  Run,
  RunHistoryOptions,
  RuntimeStatus,
  StartupShortcutStatus,
  TailResponse,
  WorkspaceTaskApplyResult,
  WorkspaceTaskControlPreview,
  WorkspaceTaskControlReceipt,
  WorkspaceTaskDiagnostics,
  WorkspaceTaskOperation,
  WorkspaceTaskPlan,
  WorkspaceTaskState,
} from "./types";

export interface OpenRequest {
  target:
    | { kind: "task"; id: string }
    | { kind: "handoff"; handoffKind: string; id: string };
  from: string | null;
}

export async function takePendingOpen(): Promise<OpenRequest | null> {
  if (!isTauri()) return null;
  return invoke<OpenRequest | null>("take_pending_open");
}

export function onOpenRequest(handler: () => void): Promise<UnlistenFn> {
  if (!isTauri()) return Promise.resolve(() => undefined);
  return listen<OpenRequest>("devbox://open", () => handler());
}

let mockJobs: Job[] = [];
let mockServices: Job[] = [];
let mockRuns: Record<string, Run[]> = {};
let mockSequence = 0;
let mockWorkspaceTaskOperationSequence = 0;
let mockWorkspaceTaskOperations: WorkspaceTaskOperation[] = [];

export function loadRuntimeStatus(): Promise<RuntimeStatus> {
  if (!isTauri()) {
    return Promise.resolve({
      backgroundLaunch: false,
      schedulerRunning: true,
      shutdownRequested: false,
      databasePath: "%LOCALAPPDATA%\\com.devbox.runmanager\\data.db",
    });
  }
  return invoke<RuntimeStatus>("runtime_status");
}

export function hideMainWindow(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("hide_main_window");
}

export function quitApp(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("quit_app");
}

export function loadStartupShortcutStatus(): Promise<StartupShortcutStatus> {
  if (!isTauri()) {
    return Promise.resolve({
      supported: false,
      enabled: false,
      shortcutPath: "%APPDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\Run Manager.lnk",
    });
  }
  return invoke<StartupShortcutStatus>("startup_shortcut_status");
}

export function setStartupShortcutEnabled(enabled: boolean): Promise<StartupShortcutStatus> {
  if (!isTauri()) return loadStartupShortcutStatus();
  return invoke<StartupShortcutStatus>("set_startup_shortcut_enabled", { enabled });
}

export function listJobs(): Promise<Job[]> {
  if (!isTauri()) return Promise.resolve([...mockJobs]);
  return invoke<Job[]>("list_jobs");
}

export function createJob(input: JobInput): Promise<Job> {
  if (!isTauri()) {
    const now = Date.now();
    const job: Job = {
      id: `mock-${++mockSequence}`,
      kind: "job",
      name: input.name,
      command: input.command,
      cwd: input.cwd,
      targetKind: input.targetKind,
      targetDistro: input.targetDistro,
      cronExpr: input.cronExpr,
      enabled: input.enabled,
      overlapPolicy: input.overlapPolicy,
      catchUp: input.catchUp,
      envConfigured: input.environment.action === "replace" && Object.keys(input.environment.values).length > 0,
      lastEvaluatedAt: input.enabled ? now : null,
      nextQueueSequence: 0,
      restartPolicy: null,
      autoStart: null,
      healthTcpAddress: null,
      healthTcpPort: null,
      healthStartGraceMs: null,
      createdAt: now,
      updatedAt: now,
    };
    mockJobs = [...mockJobs, job];
    return Promise.resolve(job);
  }
  return invoke<Job>("create_job", { input });
}

export function updateJob(id: string, input: JobInput): Promise<Job> {
  if (!isTauri()) {
    const index = mockJobs.findIndex((job) => job.id === id);
    if (index < 0) return Promise.reject(new Error("작업을 찾을 수 없습니다."));
    const current = mockJobs[index];
    const updated: Job = {
      ...current,
      name: input.name,
      command: input.command,
      cwd: input.cwd,
      targetKind: input.targetKind,
      targetDistro: input.targetKind === "wsl" ? input.targetDistro : null,
      cronExpr: input.cronExpr,
      enabled: input.enabled,
      overlapPolicy: input.overlapPolicy,
      catchUp: input.catchUp,
      envConfigured:
        input.environment.action === "clear"
          ? false
          : input.environment.action === "replace"
            ? Object.keys(input.environment.values).length > 0
            : current.envConfigured,
      lastEvaluatedAt:
        current.cronExpr !== input.cronExpr || current.catchUp !== input.catchUp || current.enabled !== input.enabled
          ? Date.now()
          : current.lastEvaluatedAt,
      updatedAt: Date.now(),
    };
    mockJobs = mockJobs.map((job) => (job.id === id ? updated : job));
    return Promise.resolve(updated);
  }
  return invoke<Job>("update_job", { id, input });
}

export function setJobEnabled(id: string, enabled: boolean): Promise<Job> {
  if (!isTauri()) {
    const current = mockJobs.find((job) => job.id === id);
    if (!current) return Promise.reject(new Error("작업을 찾을 수 없습니다."));
    const now = Date.now();
    const updated = {
      ...current,
      enabled,
      lastEvaluatedAt: current.enabled === enabled ? current.lastEvaluatedAt : now,
      updatedAt: now,
    };
    mockJobs = mockJobs.map((job) => (job.id === id ? updated : job));
    return Promise.resolve(updated);
  }
  return invoke<Job>("set_job_enabled", { id, enabled });
}

export function deleteJob(id: string): Promise<boolean> {
  if (!isTauri()) {
    const before = mockJobs.length;
    mockJobs = mockJobs.filter((job) => job.id !== id);
    return Promise.resolve(before !== mockJobs.length);
  }
  return invoke<boolean>("delete_job", { id });
}

export function listServices(): Promise<Job[]> {
  if (!isTauri()) return Promise.resolve([...mockServices]);
  return invoke<Job[]>("list_services");
}

export function getService(id: string): Promise<Job | null> {
  if (!isTauri()) return Promise.resolve(mockServices.find((service) => service.id === id) ?? null);
  return invoke<Job | null>("get_service", { id });
}

export function createService(input: ServiceInput): Promise<Job> {
  if (!isTauri()) {
    const now = Date.now();
    const service: Job = {
      id: `mock-service-${++mockSequence}`,
      kind: "service",
      name: input.name,
      command: input.command,
      cwd: input.cwd,
      targetKind: input.targetKind,
      targetDistro: input.targetDistro,
      envConfigured: input.environment.action === "replace" && Object.keys(input.environment.values).length > 0,
      cronExpr: null,
      enabled: false,
      overlapPolicy: "skip",
      catchUp: false,
      lastEvaluatedAt: null,
      nextQueueSequence: 0,
      restartPolicy: input.restartPolicy,
      autoStart: input.autoStart,
      healthTcpAddress: input.healthTcpAddress,
      healthTcpPort: input.healthTcpPort,
      healthStartGraceMs: 10_000,
      createdAt: now,
      updatedAt: now,
    };
    mockServices = [...mockServices, service];
    return Promise.resolve(service);
  }
  return invoke<Job>("create_service", { input });
}

export function updateService(id: string, input: ServiceInput): Promise<Job> {
  if (!isTauri()) {
    const index = mockServices.findIndex((service) => service.id === id);
    if (index < 0) return Promise.reject(new Error("서비스를 찾을 수 없습니다."));
    const current = mockServices[index];
    const updated: Job = {
      ...current,
      ...input,
      targetDistro: input.targetKind === "wsl" ? input.targetDistro : null,
      envConfigured:
        input.environment.action === "clear"
          ? false
          : input.environment.action === "replace"
            ? Object.keys(input.environment.values).length > 0
            : current.envConfigured,
      healthStartGraceMs: 10_000,
      updatedAt: Date.now(),
    };
    mockServices = mockServices.map((service) => (service.id === id ? updated : service));
    return Promise.resolve(updated);
  }
  return invoke<Job>("update_service", { id, input });
}

export function deleteService(id: string): Promise<boolean> {
  if (!isTauri()) {
    const before = mockServices.length;
    mockServices = mockServices.filter((service) => service.id !== id);
    return Promise.resolve(before !== mockServices.length);
  }
  return invoke<boolean>("delete_service", { id });
}

export function getServiceInstance(id: string): Promise<ServiceInstance | null> {
  if (!isTauri()) {
    return Promise.resolve(mockServiceInstance(id));
  }
  return invoke<ServiceInstance | null>("get_service_instance", { id });
}

export interface ServiceObservability {
  id: string;
  definition: Job;
  instance: ServiceInstance | null;
  current: Run | null;
  currentPid: number | null;
  recent: Run[];
  restartCount: number;
  nextRetryAt: number | null;
}

export interface DefinitionExport {
  schemaVersion: number;
  exportedAt: string;
  jobs: Job[];
  services: Job[];
}

export function getServiceObservability(id: string): Promise<ServiceObservability | null> {
  if (!isTauri()) {
    const inst = mockServiceInstance(id);
    const current: Run = {
      id: "run-1",
      jobId: id,
      scheduledAt: Date.now() - 3600000,
      occurrenceWallKey: null,
      queueSequence: 1,
      startedAt: Date.now() - 3600000,
      endedAt: null,
      exitCode: null,
      status: "running",
      logsAvailable: true,
      failureCode: null,
      createdAt: Date.now() - 3600000,
    };
    return Promise.resolve({
      id,
      definition: inst as unknown as Job,
      instance: inst,
      current,
      currentPid: 12345,
      recent: [],
      restartCount: 1,
      nextRetryAt: null,
    });
  }
  return invoke<ServiceObservability | null>("service_observability", { id });
}

export function exportDefinitions(): Promise<DefinitionExport | null> {
  if (!isTauri()) {
    return Promise.resolve(null);
  }
  return invoke<DefinitionExport | null>("export_definitions");
}

export interface ImportItem {
  id: string;
  name: string;
  kind: "job" | "service";
  status: "new" | "conflict";
  detail: string;
  cwd: string | null;
  environmentKeys: string[];
  requiresConfirmation: boolean;
}

export interface ImportPlan {
  schemaVersion: number;
  revision: string;
  items: ImportItem[];
}

export function importDefinitions(json: string): Promise<ImportPlan> {
  if (!isTauri()) {
    return Promise.resolve({ schemaVersion: 1, revision: "", items: [] });
  }
  return invoke<ImportPlan>("import_definitions", { json });
}

export function applyImport(json: string, selected: string[], revision?: string): Promise<number> {
  if (!isTauri()) return Promise.resolve(0);
  return invoke<number>("apply_import", { json, selected, revision: revision ?? null });
}

export type ProjectImportSource = "package-script" | "cargo-target";

export interface ProjectImportFile {
  path: string;
  bytes: number;
}

export interface ProjectImportItem {
  id: string;
  name: string;
  command: string;
  kind: "job";
  status: "new" | "conflict";
  source: ProjectImportSource;
  sourceName: string;
  sourcePath: string;
  cwd: string;
  environmentKeys: string[];
  requiresConfirmation: boolean;
  detail: string;
}

export interface ProjectImportPlan {
  schemaVersion: number;
  sourceRoot: string;
  revision: string;
  files: ProjectImportFile[];
  items: ProjectImportItem[];
}

export interface ProjectImportApplyResult {
  created: number;
  skippedConflicts: number;
}

function createImportOperationId(prefix: "preview" | "apply"): string {
  const random = globalThis.crypto?.randomUUID?.();
  return `${prefix}-${random ?? `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`}`;
}

export function previewProjectImport(path: string, operationId = createImportOperationId("preview")): Promise<ProjectImportPlan> {
  if (!isTauri()) {
    return Promise.resolve({
      schemaVersion: 1,
      sourceRoot: path,
      revision: "",
      files: [],
      items: [],
    });
  }
  return invoke<ProjectImportPlan>("preview_project_import", { path, operationId });
}

export function applyProjectImport(
  path: string,
  sourceRoot: string,
  revision: string,
  selected: string[],
  operationId = createImportOperationId("apply"),
): Promise<ProjectImportApplyResult> {
  if (!isTauri()) return Promise.resolve({ created: selected.length, skippedConflicts: 0 });
  return invoke<ProjectImportApplyResult>("apply_project_import", {
    path,
    sourceRoot,
    revision,
    selected,
    operationId,
  });
}

export function cancelProjectImport(operationId: string): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("cancel_project_import", { operationId });
}

function createWorkspaceTaskOperationId(prefix: "preview" | "apply"): string {
  const random = globalThis.crypto?.randomUUID?.();
  return `${prefix}-${random ?? `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`}`;
}

export function previewWorkspaceTaskImport(
  path: string,
  targetKind: WorkspaceTaskPlan["targetKind"],
  targetDistro: string | null = null,
  operationId = createWorkspaceTaskOperationId("preview"),
): Promise<WorkspaceTaskPlan> {
  if (!isTauri()) {
    return Promise.resolve({
      schemaVersion: 1,
      sourceRoot: path,
      sourcePath: ".vscode/tasks.json",
      projectIdentity: "",
      revision: "",
      targetKind,
      targetDistro,
      selectedPlatform: targetKind === "wsl" ? "linux" : "windows",
      items: [],
    });
  }
  return invoke<WorkspaceTaskPlan>("preview_workspace_task_import", {
    path,
    targetKind,
    targetDistro,
    operationId,
  });
}

export function cancelWorkspaceTaskImport(operationId: string): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("cancel_workspace_task_import", { operationId });
}

export function applyWorkspaceTaskImport(
  path: string,
  sourceRoot: string,
  projectIdentity: string,
  revision: string,
  targetKind: WorkspaceTaskPlan["targetKind"],
  targetDistro: string | null | undefined,
  selected: string[],
  operationId = createWorkspaceTaskOperationId("apply"),
): Promise<WorkspaceTaskApplyResult> {
  if (!isTauri()) {
    return Promise.resolve({
      sourceId: "mock-workspace-source",
      created: selected.length,
      updated: 0,
      madeUnavailable: 0,
      skippedConflicts: 0,
    });
  }
  return invoke<WorkspaceTaskApplyResult>("apply_workspace_task_import", {
    path,
    sourceRoot,
    projectIdentity,
    revision,
    targetKind,
    targetDistro: targetDistro ?? null,
    selected,
    operationId,
  });
}

export function listWorkspaceTasks(): Promise<WorkspaceTaskState[]> {
  if (!isTauri()) return Promise.resolve([]);
  return invoke<WorkspaceTaskState[]>("list_workspace_tasks");
}

export function trustWorkspaceTaskSource(sourceId: string, revision: string): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(true);
  return invoke<boolean>("trust_workspace_task_source", { sourceId, revision });
}

/**
 * Grant the separate shell-execution boundary for an exact, already trusted
 * workspace revision. Keep the acknowledgement literal here so callers
 * cannot accidentally reuse the ordinary source-trust action for shell code.
 */
export function trustWorkspaceTaskShellSource(sourceId: string, revision: string): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(true);
  return invoke<boolean>("trust_workspace_task_shell_source", {
    sourceId,
    revision,
    acknowledgement: "execute-shell-tasks",
  });
}

export function runWorkspaceTaskOperation(id: string, failFast: boolean): Promise<WorkspaceTaskOperation> {
  if (!isTauri()) {
    const now = Date.now();
    const operation: WorkspaceTaskOperation = {
      id: `mock-workspace-operation-${++mockWorkspaceTaskOperationSequence}`,
      rootJobId: id,
      sourceId: "mock-workspace-source",
      revision: "",
      status: "succeeded",
      failFast,
      failureCode: null,
      createdAt: now,
      startedAt: now,
      endedAt: now,
      runs: [{
        jobId: id,
        runId: `mock-operation-run-${mockWorkspaceTaskOperationSequence}`,
        layerIndex: 0,
        sequence: 0,
        status: "succeeded",
        failureCode: null,
      }],
    };
    mockWorkspaceTaskOperations = [operation, ...mockWorkspaceTaskOperations].slice(0, 50);
    return Promise.resolve(operation);
  }
  return invoke<WorkspaceTaskOperation>("run_workspace_task_operation", { id, failFast });
}

export function getWorkspaceTaskOperation(operationId: string): Promise<WorkspaceTaskOperation | null> {
  if (!isTauri()) {
    return Promise.resolve(mockWorkspaceTaskOperations.find((operation) => operation.id === operationId) ?? null);
  }
  return invoke<WorkspaceTaskOperation | null>("get_workspace_task_operation", { operationId });
}

export function listWorkspaceTaskOperations(limit = 20): Promise<WorkspaceTaskOperation[]> {
  if (!isTauri()) return Promise.resolve(mockWorkspaceTaskOperations.slice(0, Math.max(1, Math.min(limit, 100))));
  return invoke<WorkspaceTaskOperation[]>("list_workspace_task_operations", { limit });
}

export function stopWorkspaceTaskOperation(operationId: string): Promise<WorkspaceTaskOperation> {
  if (!isTauri()) {
    const operation = mockWorkspaceTaskOperations.find((candidate) => candidate.id === operationId);
    if (!operation) return Promise.reject(new Error("workspace-task-operation-not-found"));
    const stopped: WorkspaceTaskOperation = {
      ...operation,
      status: operation.status === "succeeded" || operation.status === "failed" || operation.status === "cancelled"
        ? operation.status
        : "cancelled",
      endedAt: Date.now(),
      runs: operation.runs.map((run) => (
        run.status === "pending" || run.status === "launching" || run.status === "running"
          ? { ...run, status: "cancelled", failureCode: "workspace-task-operation-cancelled" }
          : run
      )),
    };
    mockWorkspaceTaskOperations = mockWorkspaceTaskOperations.map((candidate) => candidate.id === operationId ? stopped : candidate);
    return Promise.resolve(stopped);
  }
  return invoke<WorkspaceTaskOperation>("stop_workspace_task_operation", { operationId });
}

export function listWorkspaceTaskDiagnostics(runId: string): Promise<WorkspaceTaskDiagnostics> {
  if (!isTauri()) {
    return Promise.resolve({ runId, items: [], truncated: false });
  }
  return invoke<WorkspaceTaskDiagnostics>("list_workspace_task_diagnostics", { runId });
}

export function openWorkspaceTaskDiagnostic(runId: string, diagnosticIndex: number): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("open_workspace_task_diagnostic", { runId, diagnosticIndex });
}

export function previewWorkspaceTaskControl(handoffId: string): Promise<WorkspaceTaskControlPreview> {
  if (!isTauri()) return Promise.reject(new Error("task-control-unavailable"));
  return invoke<WorkspaceTaskControlPreview>("preview_workspace_task_control", { handoffId });
}

export function acceptWorkspaceTaskControl(requestId: string): Promise<WorkspaceTaskControlReceipt> {
  if (!isTauri()) return Promise.reject(new Error("task-control-unavailable"));
  return invoke<WorkspaceTaskControlReceipt>("accept_workspace_task_control", { requestId });
}

export function rejectWorkspaceTaskControl(requestId: string): Promise<WorkspaceTaskControlReceipt> {
  if (!isTauri()) return Promise.reject(new Error("task-control-unavailable"));
  return invoke<WorkspaceTaskControlReceipt>("reject_workspace_task_control", { requestId });
}

export function renewWorkspaceTaskControl(requestId: string): Promise<number> {
  if (!isTauri()) return Promise.reject(new Error("task-control-unavailable"));
  return invoke<number>("renew_workspace_task_control", { requestId });
}

export function listWorkspaceTaskControlReceipts(limit = 20): Promise<WorkspaceTaskControlReceipt[]> {
  if (!isTauri()) return Promise.resolve([]);
  return invoke<WorkspaceTaskControlReceipt[]>("list_workspace_task_control_receipts", { limit });
}

const FRIENDLY_BACKEND_ERRORS: Record<string, string> = {
  "workspace-task-source-untrusted": "이 workspace task의 소스가 아직 승인되지 않았습니다. 현재 revision을 명시적으로 승인한 뒤 활성화하세요.",
  "workspace-task-unavailable": "이 workspace task는 현재 사용할 수 없습니다. 원본을 다시 미리보고 가져오세요.",
  "workspace-task-source-unavailable": "workspace task 원본을 읽을 수 없습니다. 프로젝트 경로와 .vscode/tasks.json을 확인하세요.",
  "workspace-task-source-changed": "원본 tasks.json이 변경되어 승인이 무효화되었습니다. 다시 미리보고 승인하세요.",
  "workspace-task-managed-fields-locked": "workspace task의 이름·명령·작업 디렉터리·대상은 원본이 관리합니다.",
  "workspace-task-environment-key-not-declared": "원본에 선언된 환경변수 키만 입력할 수 있습니다.",
  "workspace-task-configuration-invalid": "workspace task 설정이 올바르지 않아 실행할 수 없습니다.",
  "workspace-task-definition-invalid": "workspace task 설정이 올바르지 않아 실행할 수 없습니다.",
  "workspace-task-shell-untrusted": "이 workspace task의 셸 실행이 아직 승인되지 않았습니다. 위험 내용을 확인한 뒤 셸 실행을 별도로 승인하세요.",
  "workspace-task-shell-confirmation-required": "셸 실행 승인을 완료하지 못했습니다. 안내된 확인 문구로 다시 시도하세요.",
  "workspace-task-shell-not-found": "승인할 셸 workspace task를 찾지 못했습니다. 작업 목록을 다시 불러오세요.",
  "workspace-task-dependency-selection-incomplete": "선택한 task의 선행 dependency가 빠졌습니다. dependency를 함께 선택한 뒤 다시 가져오세요.",
  "workspace-task-dependency-unavailable": "선행 dependency를 사용할 수 없어 이 task를 실행할 수 없습니다.",
  "workspace-task-dependency-cycle": "task dependency에 순환 참조가 있어 실행할 수 없습니다.",
  "workspace-task-dependency-graph-too-large": "task dependency 그래프가 허용된 크기를 초과했습니다.",
  "workspace-task-invalid-dependency": "task dependency 선언이 올바르지 않습니다.",
  "workspace-task-invalid-dependency-order": "task dependency 실행 순서가 parallel 또는 sequence가 아닙니다.",
  "workspace-task-dependency-order-without-dependency": "dependency 없이 실행 순서를 지정할 수 없습니다.",
  "workspace-task-named-problem-matcher-unsupported": "이름으로 지정한 problem matcher는 지원하지 않습니다.",
  "workspace-task-background-problem-matcher-unsupported": "background problem matcher는 지원하지 않습니다.",
  "workspace-task-unsupported-problem-matcher-field": "지원하지 않는 problem matcher 필드가 있습니다.",
  "workspace-task-unsupported-problem-matcher-location": "problem matcher의 파일 위치 설정을 지원하지 않습니다.",
  "workspace-task-invalid-problem-matcher": "problem matcher 형식이 올바르지 않습니다.",
  "workspace-task-orchestration-required": "이 workspace task는 dependency가 있어 orchestration 실행이 필요합니다.",
  "workspace-task-orchestration-manual-only": "dependency가 있는 workspace task는 일정 실행을 지원하지 않습니다. 지금 실행에서 orchestration으로 실행하세요.",
  "workspace-task-not-found": "workspace task를 찾지 못했습니다. 목록을 다시 불러오세요.",
  "workspace-task-operation-active": "이 workspace task에는 이미 실행 중인 orchestration operation이 있습니다.",
  "workspace-task-operation-not-found": "workspace task operation을 찾지 못했습니다. 목록을 다시 불러오세요.",
  "workspace-task-operation-state-changed": "workspace task operation 상태가 바뀌어 요청을 완료하지 못했습니다.",
  "workspace-task-operation-cancelled": "workspace task operation이 취소되었습니다.",
  "workspace-task-operation-interrupted": "앱이 종료되어 workspace task operation이 중단되었습니다.",
  "workspace-task-operation-stop-failed": "workspace task operation을 안전하게 중지하지 못했습니다.",
  "workspace-task-operation-stop-timeout": "workspace task operation 중지가 제한 시간 안에 끝나지 않았습니다.",
  "workspace-task-operation-ownership-changed": "workspace task operation의 실행 소유권이 바뀌어 중지할 수 없습니다.",
  "workspace-task-operation-storage": "workspace task operation 상태를 저장소에서 읽지 못했습니다.",
  "workspace-task-start-failed": "선행 workspace task를 시작하지 못했습니다.",
  "workspace-task-run-failed": "workspace task 실행이 실패했습니다.",
  "workspace-task-run-cancelled": "workspace task 실행이 취소되었습니다.",
  "workspace-task-run-skipped": "workspace task 실행을 건너뛰었습니다.",
  "workspace-task-run-missing": "workspace task 실행 기록을 찾지 못했습니다.",
  "workspace-task-dependency-failed": "선행 dependency가 실패해 operation을 진행하지 못했습니다.",
  "workspace-task-diagnostic-run-active": "실행 중인 child의 diagnostics는 아직 확인할 수 없습니다.",
  "workspace-task-diagnostic-run-not-found": "diagnostics를 확인할 실행 기록을 찾지 못했습니다.",
  "workspace-task-diagnostic-unavailable": "이 실행의 workspace task diagnostics를 사용할 수 없습니다.",
  "workspace-task-diagnostic-matcher-unavailable": "이 workspace task에는 지원되는 problem matcher가 없습니다.",
  "workspace-task-diagnostic-logs-unavailable": "실행 로그를 읽지 못해 diagnostics를 만들 수 없습니다.",
  "workspace-task-diagnostic-selection-invalid": "선택한 diagnostic 항목을 찾지 못했습니다.",
  "workspace-task-diagnostic-path-invalid": "diagnostic 파일 위치가 올바르지 않습니다.",
  "workspace-task-diagnostic-path-unsafe": "안전하지 않은 diagnostic 파일 위치라 열 수 없습니다.",
  "workspace-task-diagnostic-path-unavailable": "diagnostic 파일을 찾을 수 없습니다.",
  "workspace-task-diagnostic-launch-failed": "Code Pad에서 diagnostic 파일을 열지 못했습니다.",
  "workspace-task-diagnostic-storage": "workspace task diagnostics를 읽지 못했습니다.",
  "task-control-invalid": "task-control handoff 형식이 올바르지 않습니다.",
  "task-control-busy": "이미 확인 중인 task-control 요청이 있습니다.",
  "task-control-unavailable": "task-control handoff를 확인할 수 없습니다.",
  "task-control-not-open": "task-control 확인 요청이 만료되었거나 이미 처리되었습니다.",
  "task-control-task-not-found": "task-control 대상 workspace task를 찾지 못했습니다.",
  "task-control-source-changed": "workspace task 원본이 변경되어 요청을 처리할 수 없습니다.",
  "task-control-request-replayed": "이미 처리된 task-control 요청입니다.",
  "task-control-claim-failed": "task-control 요청 소유권을 확인하지 못했습니다.",
  "task-control-operation-not-active": "중지할 활성 workspace task operation이 없습니다.",
  "task-control-user-rejected": "task-control 요청을 거절했습니다.",
  "task-control-request-id-invalid": "task-control 요청 식별자가 올바르지 않습니다.",
  "task-control-action-corrupt": "task-control 요청의 작업 종류를 확인하지 못했습니다.",
  "task-control-receipt-status-corrupt": "task-control 처리 상태를 확인하지 못했습니다.",
  "task-control-receipt-state": "task-control 처리 상태가 올바르지 않습니다.",
  "task-control-receipt-shape-invalid": "task-control 처리 내역 형식이 올바르지 않습니다.",
  "task-control-receipt-storage": "task-control 처리 내역을 저장하지 못했습니다.",
  "task-control-storage": "task-control 상태를 읽지 못했습니다.",
  "task-control-lease-expired": "확인 시간이 너무 길어 task-control 요청이 만료되었습니다.",
  "task-control-interrupted": "앱이 종료되어 처리 중이던 task-control 요청이 중단되었습니다.",
};

/** Convert fixed native workspace-task codes to actionable UI text without
 * echoing paths, commands, argv, source text, or environment values. */
export function friendlyErrorMessage(cause: unknown): string {
  const value = cause instanceof Error ? cause.message : String(cause);
  const normalized = value.trim();
  const code = normalized.startsWith("workspace-task-")
    ? normalized
    : `workspace-task-${normalized}`;
  return FRIENDLY_BACKEND_ERRORS[normalized]
    ?? FRIENDLY_BACKEND_ERRORS[code]
    ?? "요청을 완료하지 못했습니다.";
}

export function startService(id: string): Promise<ServiceInstance> {
  if (!isTauri()) {
    mockServiceState.set(id, "running");
    return Promise.resolve(mockServiceInstance(id)!);
  }
  return invoke<ServiceInstance>("start_service", { id });
}

export function stopService(id: string): Promise<ServiceInstance | null> {
  if (!isTauri()) {
    mockServiceState.set(id, "stopped");
    return Promise.resolve(mockServiceInstance(id));
  }
  return invoke<ServiceInstance | null>("stop_service", { id });
}

export function restartService(id: string): Promise<ServiceInstance> {
  if (!isTauri()) {
    mockServiceState.set(id, "running");
    return Promise.resolve(mockServiceInstance(id)!);
  }
  return invoke<ServiceInstance>("restart_service", { id });
}

const mockServiceState = new Map<string, "stopped" | "running">();

function mockServiceInstance(id: string): ServiceInstance | null {
  const service = mockServices.find((item) => item.id === id);
  if (!service) return null;
  return {
    jobId: id,
    generation: 0,
    state: mockServiceState.get(id) ?? "stopped",
    consecutiveFailures: 0,
    nextRetryAt: null,
  };
}

export function previewCron(cronExpr: string): Promise<CronPreviewItem[]> {
  if (!isTauri()) return Promise.resolve(mockPreview(cronExpr));
  return invoke<CronPreviewItem[]>("preview_cron", { input: { cronExpr } });
}

export function listRuns(
  jobId: string | null,
  options: RunHistoryOptions = {},
): Promise<Run[]> {
  if (!isTauri()) {
    const source = jobId ? (mockRuns[jobId] ?? []) : Object.values(mockRuns).flat();
    const definitions = [...mockJobs, ...mockServices];
    const filtered = source.filter((run) => {
      const definition = definitions.find((item) => item.id === run.jobId);
      if (options.kind && definition?.kind !== options.kind) return false;
      if (options.status && run.status !== options.status) return false;
      const timestamp = run.startedAt ?? run.createdAt;
      if (options.startAt !== null && options.startAt !== undefined && timestamp < options.startAt) return false;
      if (options.endAt !== null && options.endAt !== undefined && timestamp >= options.endAt) return false;
      if (options.minDurationMs !== null && options.minDurationMs !== undefined) {
        if (run.startedAt === null || (run.endedAt ?? Date.now()) - run.startedAt < options.minDurationMs) return false;
      }
      if (options.maxDurationMs !== null && options.maxDurationMs !== undefined) {
        if (run.startedAt === null || (run.endedAt ?? Date.now()) - run.startedAt > options.maxDurationMs) return false;
      }
      return true;
    });
    const limit = Math.max(1, Math.min(options.limit ?? 50, 500));
    return Promise.resolve(filtered.slice(0, limit));
  }
  return invoke<Run[]>("list_runs", {
    jobId: jobId ?? null,
    limit: options.limit ?? 50,
    startAt: options.startAt ?? null,
    endAt: options.endAt ?? null,
    status: options.status ?? null,
    kind: options.kind ?? null,
    minDurationMs: options.minDurationMs ?? null,
    maxDurationMs: options.maxDurationMs ?? null,
  });
}

export function runJobNow(jobId: string): Promise<Run> {
  if (!isTauri()) {
    const now = Date.now();
    const run: Run = {
      id: `mock-run-${++mockSequence}`,
      jobId,
      scheduledAt: null,
      occurrenceWallKey: null,
      queueSequence: (mockRuns[jobId]?.length ?? 0) + 1,
      startedAt: now,
      endedAt: null,
      exitCode: null,
      status: "running",
      logsAvailable: false,
      failureCode: null,
      createdAt: now,
    };
    mockRuns = { ...mockRuns, [jobId]: [run, ...(mockRuns[jobId] ?? [])] };
    return Promise.resolve(run);
  }
  return invoke<Run>("run_job_now", { id: jobId });
}

export function stopActiveRun(jobId: string): Promise<Run | null> {
  if (!isTauri()) {
    const current = mockRuns[jobId] ?? [];
    const active = current.find((run) => ["starting", "running", "stopping"].includes(run.status));
    if (!active) return Promise.resolve(null);
    const stopped: Run = { ...active, status: "cancelled", endedAt: Date.now() };
    mockRuns = { ...mockRuns, [jobId]: current.map((run) => (run.id === active.id ? stopped : run)) };
    return Promise.resolve(stopped);
  }
  return invoke<Run | null>("stop_active_run", { id: jobId });
}

export function getActiveRun(jobId: string): Promise<Run | null> {
  if (!isTauri()) {
    return Promise.resolve(
      (mockRuns[jobId] ?? []).find((run) => ["starting", "running", "stopping"].includes(run.status)) ?? null,
    );
  }
  return invoke<Run | null>("get_active_run", { id: jobId });
}

export function listActiveRuns(): Promise<Run[]> {
  if (!isTauri()) {
    return Promise.resolve(
      Object.values(mockRuns)
        .flat()
        .filter((run) => ["starting", "running", "stopping"].includes(run.status)),
    );
  }
  return invoke<Run[]>("list_active_runs");
}

export function tailLog(
  runId: string,
  stream: LogStream,
  cursor: string | null,
  maxBytes = 256 * 1024,
): Promise<TailResponse> {
  if (!isTauri()) {
    return Promise.resolve({
      data: [],
      retainedStartOffset: cursor ?? "0",
      nextCursor: cursor ?? "0",
      truncated: false,
    });
  }
  return invoke<TailResponse>("tail_log", {
    input: { runId, stream, cursor, maxBytes },
  });
}

export function searchRunLogs(runId: string, options: LogSearchOptions): Promise<LogSearchResponse> {
  const input = {
    runId,
    query: options.query,
    mode: options.mode,
    source: options.source,
    level: options.level,
    startAt: options.startAt,
    endAt: options.endAt,
  };
  if (!isTauri()) {
    return Promise.resolve({
      matches: [],
      scannedLines: 0,
      scannedBytes: 0,
      truncated: false,
      sources: options.source
        ? [{ kind: "log-source/v1" as const, sourceId: `run-manager:${runId}:${options.source}`, runId, stream: options.source }]
        : [],
    });
  }
  return invoke<LogSearchResponse>("search_run_logs", { input });
}

/** Start the explicit Run Manager -> Log Lens handoff. Browser fixtures never
 * publish a pending envelope or launch another application. */
export async function openRunLogInLogLens(runId: string, stream: LogStream): Promise<void> {
  if (!isTauri()) throw new Error("Log Lens handoff는 데스크톱 앱에서만 사용할 수 있습니다");
  await invoke("open_run_log_in_log_lens", { runId, stream });
}

/**
 * Browser preview data is only a development fallback; the Tauri command
 * above is authoritative and uses core::cron. Keeping a small fallback makes
 * the editor usable in Vite/RTL without pretending JavaScript is the scheduler.
 */
function mockPreview(cronExpr: string): CronPreviewItem[] {
  const value = cronExpr.trim();
  if (!value || value.startsWith("@") || (value.split(/\s+/).length !== 5 && value.split(/\s+/).length !== 6)) {
    throw new Error("cron_expr: invalid cron expression");
  }
  const now = new Date();
  return Array.from({ length: 5 }, (_, index) => {
    const next = new Date(now.getTime() + (index + 1) * 60_000);
    const pad = (number: number) => String(number).padStart(2, "0");
    const wallTime = `${next.getFullYear()}-${pad(next.getMonth() + 1)}-${pad(next.getDate())} ${pad(next.getHours())}:${pad(next.getMinutes())}:00`;
    return {
      timestampMillis: next.getTime(),
      datetime: next.toISOString(),
      wallTime,
      wallKey: wallTime.replace(" ", "T"),
    };
  });
}
