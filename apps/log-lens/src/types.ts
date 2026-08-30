export type LogFormat = "plain" | "jsonl" | "logfmt";
export type LogLevel = "trace" | "debug" | "info" | "warn" | "error" | "fatal";
export type SourceKind =
  | "localFile"
  | "directory"
  | "wslFile"
  | "wslJournal"
  | "run"
  | "container";
export type ContainerEngine = "docker" | "podman";
export type ReadStatus = "initial" | "advanced" | "rotated" | "truncated" | "unavailable";

export type SourceSpec =
  | { kind: "localFile"; path: string }
  | { kind: "directory"; path: string; pattern: string }
  | { kind: "wslFile"; distro: string; path: string }
  | { kind: "wslJournal"; distro: string; unit?: string }
  | { kind: "run"; sourceId: string }
  | { kind: "container"; engine: ContainerEngine; containerId: string };

export interface SourceSummary {
  sourceId: string;
  kind: SourceKind;
  displayName: string;
  readOnly: boolean;
  handoff: boolean;
}

export interface FileIdentity {
  device: number | null;
  inode: number | null;
  size: number;
  modifiedMillis: number | null;
}

export interface FileCursor {
  identity: FileIdentity | null;
  offset: string;
  /** Hash of a bounded prefix immediately before offset, used for truncate detection. */
  anchorHash?: string | null;
}

export interface LogRecord {
  sourceId: string;
  sequence: number;
  timestampMillis: number | null;
  level: LogLevel | null;
  message: string;
  fields: Record<string, string>;
  format: LogFormat;
  truncated: boolean;
}

export interface SourceSnapshot {
  operationId: string;
  generation: number;
  source: SourceSummary;
  records: LogRecord[];
  nextCursor: FileCursor | null;
  status: ReadStatus;
  truncated: boolean;
  droppedRecords: number;
  droppedBytes: number;
}

export interface SourcesSnapshot {
  operationId: string;
  generation: number;
  sources: SourceSummary[];
  records: LogRecord[];
  cursors: Array<FileCursor | null>;
  statuses: ReadStatus[];
  truncated: boolean;
  droppedRecords: number;
  droppedBytes: number;
}

export interface FilterSpec {
  text: string;
  regex: boolean;
  sourceId?: string;
  level?: LogLevel;
  startAt?: number;
  endAt?: number;
  field?: string;
  fieldValue?: string;
}

export interface SavedView {
  name: string;
  sources: SourceSpec[];
  filter: FilterSpec;
}

export type HandoffOpenTarget = { kind: "handoff"; handoffKind: string; id: string };

export interface LogSourcePreview {
  id: string;
  kind: "log-source/v1";
  sourceApp: "run-manager" | "port-manager" | "wsl-desktop";
  expiresAtMs: number;
  leaseUntilMs: number;
  source: SourceSummary;
}

export interface ExportedText {
  text: string;
  truncated: boolean;
}

/** Result of publishing an explicit log selection to Developer Toolbox. */
export interface ToolboxDispatch {
  handoffId: string;
  redacted: boolean;
}
