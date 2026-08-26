import { invoke } from "@tauri-apps/api/core";
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
  RuntimeStatus,
  StartupShortcutStatus,
  TailResponse,
} from "./types";

let mockJobs: Job[] = [];
let mockServices: Job[] = [];
let mockRuns: Record<string, Run[]> = {};
let mockSequence = 0;

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
}

export interface ImportPlan {
  schemaVersion: number;
  items: ImportItem[];
}

export function importDefinitions(json: string): Promise<ImportPlan> {
  if (!isTauri()) {
    return Promise.resolve({ schemaVersion: 1, items: [] });
  }
  return invoke<ImportPlan>("import_definitions", { json });
}

export function applyImport(json: string, selected: string[]): Promise<number> {
  if (!isTauri()) return Promise.resolve(0);
  return invoke<number>("apply_import", { json, selected });
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
  jobId: string,
  options: { limit?: number; startAt?: number | null; endAt?: number | null } = {},
): Promise<Run[]> {
  if (!isTauri()) {
    return Promise.resolve([...(mockRuns[jobId] ?? [])].slice(0, options.limit ?? 50));
  }
  return invoke<Run[]>("list_runs", {
    jobId,
    limit: options.limit ?? 50,
    startAt: options.startAt ?? null,
    endAt: options.endAt ?? null,
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
