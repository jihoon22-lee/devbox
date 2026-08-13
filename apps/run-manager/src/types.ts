export interface RuntimeStatus {
  backgroundLaunch: boolean;
  schedulerRunning: boolean;
  shutdownRequested: boolean;
  databasePath: string;
}

export type TargetKind = "windows" | "wsl";
export type OverlapPolicy = "skip" | "queue" | "kill-previous";
export type RunStatus =
  | "queued"
  | "starting"
  | "running"
  | "stopping"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "skipped";
export type LogStream = "stdout" | "stderr";

export interface Job {
  id: string;
  kind: "job" | "service";
  name: string;
  command: string;
  cwd: string | null;
  targetKind: TargetKind;
  targetDistro: string | null;
  envConfigured: boolean;
  cronExpr: string | null;
  enabled: boolean;
  overlapPolicy: OverlapPolicy;
  catchUp: boolean;
  lastEvaluatedAt: number | null;
  nextQueueSequence: number;
  restartPolicy: string | null;
  autoStart: boolean | null;
  healthTcpAddress: string | null;
  healthTcpPort: number | null;
  healthStartGraceMs: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface JobInput {
  name: string;
  command: string;
  cwd: string | null;
  targetKind: TargetKind;
  targetDistro: string | null;
  cronExpr: string;
  enabled: boolean;
  overlapPolicy: OverlapPolicy;
  catchUp: boolean;
}

export interface Run {
  id: string;
  jobId: string;
  scheduledAt: number | null;
  occurrenceWallKey: string | null;
  queueSequence: number;
  blockedByRunId: string | null;
  startedAt: number | null;
  endedAt: number | null;
  exitCode: number | null;
  status: RunStatus;
  ownerInstanceId: string | null;
  attemptToken: string | null;
  errorMessage: string | null;
  targetPid: number | null;
  targetProcessCreatedAt: number | null;
  targetPgid: number | null;
  targetSid: number | null;
  processMarker: string | null;
  logDir: string | null;
  logsDeletedAt: number | null;
  createdAt: number;
}

export interface TailResponse {
  data: number[];
  retainedStartOffset: string;
  nextCursor: string;
  truncated: boolean;
}

export interface EnvironmentDraft {
  id: string;
  key: string;
  value: string;
  persisted: boolean;
}

export interface CronPreviewItem {
  timestampMillis: number;
  datetime: string;
  wallTime: string;
  wallKey: string;
}

export type JobField =
  | "name"
  | "command"
  | "cwd"
  | "targetKind"
  | "targetDistro"
  | "cronExpr"
  | "enabled"
  | "catchUp"
  | "overlapPolicy"
  | "env";

export type JobFieldErrors = Partial<Record<JobField, string>>;
