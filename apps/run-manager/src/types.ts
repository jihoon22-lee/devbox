export interface RuntimeStatus {
  backgroundLaunch: boolean;
  schedulerRunning: boolean;
  shutdownRequested: boolean;
  databasePath: string;
}

export type TargetKind = "windows" | "wsl";
export type OverlapPolicy = "skip" | "queue" | "kill-previous";

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
