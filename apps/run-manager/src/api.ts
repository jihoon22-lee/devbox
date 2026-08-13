import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type {
  CronPreviewItem,
  Job,
  JobInput,
  LogStream,
  Run,
  RuntimeStatus,
  StartupShortcutStatus,
  TailResponse,
} from "./types";

let mockJobs: Job[] = [];
let mockSequence = 0;

export function loadRuntimeStatus(): Promise<RuntimeStatus> {
  if (!isTauri()) {
    return Promise.resolve({
      backgroundLaunch: false,
      schedulerRunning: true,
      shutdownRequested: false,
      databasePath: "%LOCALAPPDATA%\\com.workbench.runmanager\\data.db",
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
      envConfigured: false,
      cronExpr: input.cronExpr,
      enabled: input.enabled,
      overlapPolicy: input.overlapPolicy,
      catchUp: input.catchUp,
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
      ...input,
      targetDistro: input.targetKind === "wsl" ? input.targetDistro : null,
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

export function deleteJob(id: string): Promise<boolean> {
  if (!isTauri()) {
    const before = mockJobs.length;
    mockJobs = mockJobs.filter((job) => job.id !== id);
    return Promise.resolve(before !== mockJobs.length);
  }
  return invoke<boolean>("delete_job", { id });
}

export function previewCron(cronExpr: string): Promise<CronPreviewItem[]> {
  if (!isTauri()) return Promise.resolve(mockPreview(cronExpr));
  return invoke<CronPreviewItem[]>("preview_cron", { input: { cronExpr } });
}

export function listRuns(
  jobId: string,
  options: { limit?: number; startAt?: number | null; endAt?: number | null } = {},
): Promise<Run[]> {
  if (!isTauri()) return Promise.resolve([]);
  return invoke<Run[]>("list_runs", {
    jobId,
    limit: options.limit ?? 50,
    startAt: options.startAt ?? null,
    endAt: options.endAt ?? null,
  });
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
