export interface RuntimeStatus {
  backgroundLaunch: boolean;
  schedulerRunning: boolean;
  shutdownRequested: boolean;
  databasePath: string;
}

export interface StartupShortcutStatus {
  supported: boolean;
  enabled: boolean;
  shortcutPath: string;
}

export type TargetKind = "windows" | "wsl";
export type WorkspaceTaskKind = "process" | "shell";
export type WorkspaceTaskDependsOrder = "parallel" | "sequence";
export type OverlapPolicy = "skip" | "queue" | "kill-previous";
export type RestartPolicy = "never" | "on-failure" | "always";
export type RunStatus =
  | "queued"
  | "starting"
  | "running"
  | "stopping"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "skipped";
export type RunDefinitionKind = "job" | "service";
export type LogStream = "stdout" | "stderr";
export type LogSearchMode = "literal" | "regex";
export type LogLevel = "trace" | "debug" | "info" | "warn" | "error";
export type EnvironmentAction = "keep" | "replace" | "clear";

export type EnvironmentUpdate =
  | { action: "keep" }
  | { action: "replace"; values: Record<string, string> }
  | { action: "clear" };

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
  restartPolicy: RestartPolicy | null;
  autoStart: boolean | null;
  healthTcpAddress: string | null;
  healthTcpPort: number | null;
  healthStartGraceMs: number | null;
  createdAt: number;
  updatedAt: number;
}

/** Read-only projection of one bounded VS Code workspace task preview. */
export interface WorkspaceTaskItem {
  id: string;
  sourceIndex: number;
  label: string;
  status: "ready" | "blocked" | "conflict";
  taskKind?: WorkspaceTaskKind | null;
  command?: string | null;
  args: string[];
  cwd?: string | null;
  environmentKeys: string[];
  appliedOverride?: string | null;
  dependsOn: string[];
  dependsOrder: WorkspaceTaskDependsOrder;
  hasProblemMatcher: boolean;
  problemMatcher: WorkspaceProblemMatcher | null;
  blockedReason?: string | null;
}

/** The bounded problem-matcher projection supported by Run Manager. */
export interface WorkspaceProblemMatcher {
  regexp: string;
  file: number;
  line: number;
  column: number | null;
  message: number;
  severity: number | null;
}

export interface WorkspaceTaskPlan {
  schemaVersion: number;
  sourceRoot: string;
  sourcePath: string;
  projectIdentity: string;
  revision: string;
  targetKind: TargetKind;
  targetDistro?: string | null;
  selectedPlatform: string;
  items: WorkspaceTaskItem[];
}

/** Persisted workspace-task state. It deliberately contains environment key
 * names only; values remain in the protected environment store. */
export interface WorkspaceTaskState {
  jobId: string;
  sourceId: string;
  label: string;
  taskKind: WorkspaceTaskKind;
  sourceRoot: string;
  revision: string;
  targetKind: TargetKind;
  targetDistro?: string | null;
  environmentKeys: string[];
  appliedOverride?: string | null;
  dependsOn: string[];
  dependsOrder: WorkspaceTaskDependsOrder;
  hasProblemMatcher: boolean;
  trusted: boolean;
  shellTrusted: boolean;
  available: boolean;
}

export interface WorkspaceTaskApplyResult {
  sourceId: string;
  created: number;
  updated: number;
  madeUnavailable: number;
  skippedConflicts: number;
}

export type WorkspaceTaskOperationStatus =
  | "queued"
  | "running"
  | "stopping"
  | "succeeded"
  | "failed"
  | "cancelled";
export type WorkspaceTaskOperationRunStatus =
  | "pending"
  | "launching"
  | "running"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "skipped";

export interface WorkspaceTaskOperationRun {
  jobId: string;
  runId?: string | null;
  layerIndex: number;
  sequence: number;
  status: WorkspaceTaskOperationRunStatus;
  failureCode?: string | null;
}

export interface WorkspaceTaskOperation {
  id: string;
  rootJobId: string;
  sourceId: string;
  revision: string;
  status: WorkspaceTaskOperationStatus;
  failFast: boolean;
  failureCode?: string | null;
  createdAt: number;
  startedAt?: number | null;
  endedAt?: number | null;
  runs: WorkspaceTaskOperationRun[];
}

export interface WorkspaceTaskDiagnostic {
  index: number;
  file: string;
  line: number;
  column?: number | null;
  message: string;
  severity?: string | null;
  stream: string;
}

export interface WorkspaceTaskDiagnostics {
  runId: string;
  items: WorkspaceTaskDiagnostic[];
  truncated: boolean;
}

export type WorkspaceTaskControlAction = "start" | "stop";
export type WorkspaceTaskControlReceiptStatus = "accepted" | "rejected" | "started" | "stopped" | "failed";

export interface WorkspaceTaskControlPreview {
  requestId: string;
  taskId: string;
  action: WorkspaceTaskControlAction;
  expectedRevision: string;
  label: string;
  taskKind: string;
}

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
  environment: EnvironmentUpdate;
}

export interface ServiceInput {
  name: string;
  command: string;
  cwd: string | null;
  targetKind: TargetKind;
  targetDistro: string | null;
  restartPolicy: RestartPolicy;
  autoStart: boolean;
  healthTcpAddress: string | null;
  healthTcpPort: number | null;
  environment: EnvironmentUpdate;
}

export interface Run {
  id: string;
  jobId: string;
  scheduledAt: number | null;
  occurrenceWallKey: string | null;
  queueSequence: number;
  startedAt: number | null;
  endedAt: number | null;
  exitCode: number | null;
  status: RunStatus;
  logsAvailable: boolean;
  failureCode: string | null;
  createdAt: number;
}

export interface RunHistoryOptions {
  limit?: number;
  startAt?: number | null;
  endAt?: number | null;
  status?: RunStatus | null;
  kind?: RunDefinitionKind | null;
  minDurationMs?: number | null;
  maxDurationMs?: number | null;
}

export interface TailResponse {
  data: number[];
  retainedStartOffset: string;
  nextCursor: string;
  truncated: boolean;
}

export interface LogSearchOptions {
  query: string;
  mode: LogSearchMode;
  source: LogStream | null;
  level: LogLevel | null;
  startAt: number | null;
  endAt: number | null;
}

export interface LogSourceRef {
  kind: "log-source/v1";
  sourceId: string;
  runId: string;
  stream: LogStream;
}

export interface LogSearchMatch {
  sourceId: string;
  stream: LogStream;
  lineNumber: number;
  level: LogLevel | null;
  timestampMillis: number | null;
}

export interface LogSearchResponse {
  matches: LogSearchMatch[];
  scannedLines: number;
  scannedBytes: number;
  truncated: boolean;
  sources: LogSourceRef[];
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

export type ServiceField =
  | "name"
  | "command"
  | "cwd"
  | "targetKind"
  | "targetDistro"
  | "restartPolicy"
  | "autoStart"
  | "healthTcpAddress"
  | "healthTcpPort"
  | "env";

export type ServiceFieldErrors = Partial<Record<ServiceField, string>>;

export type ServiceInstanceState =
  | "stopped"
  | "starting"
  | "running"
  | "stopping"
  | "retry_waiting";

export interface ServiceInstance {
  jobId: string;
  generation: number;
  state: ServiceInstanceState;
  consecutiveFailures: number;
  nextRetryAt: number | null;
}
