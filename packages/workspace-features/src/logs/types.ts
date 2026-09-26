export type LogFormat = import("../generated/LogFormat").LogFormat;
export type LogLevel = import("../generated/LogLevel").LogLevel;
export type SourceKind =
  | "localFile"
  | "directory"
  | "wslFile"
  | "wslJournal"
  | "run"
  | "runtimeRun"
  | "webhookCapture"
  | "container";
export type ContainerEngine = import("../generated/ContainerEngine").ContainerEngine;
export type ReadStatus = import("../generated/ReadStatus").ReadStatus;

export type SourceSpec = import("../generated/SourceSpec").SourceSpec;

export type WebhookLogPayload = import("../generated/WebhookLogPayload").WebhookLogPayload;

export type SourceSummary = import("../generated/SourceSummary").SourceSummary;

export type FileIdentity = import("../generated/FileIdentity").FileIdentity;

export type FileCursor = import("../generated/FileCursor").FileCursor;

export type LogRecord = Omit<import("../generated/LogRecord").LogRecord, "fields"> & { fields: Record<string, string> };

export type SourceSnapshot = Omit<import("../generated/SourceSnapshot").SourceSnapshot, "records"> & {
  records: LogRecord[];
};

export type SourcesSnapshot = Omit<import("../generated/SourcesSnapshot").SourcesSnapshot, "records"> & {
  records: LogRecord[];
};

export type FilterSpec = import("../generated/FilterSpec").FilterSpec;

export type SavedView = import("../generated/SavedView").SavedView;

export type SavedViewsDocument = import("../generated/SavedViewsDocument").SavedViewsDocument;

export type HandoffOpenTarget = { kind: "handoff"; handoffKind: string; id: string };

export interface LogSourcePreview {
  id: string;
  kind: "log-source/v1" | "webhook-log/v1";
  sourceApp: "run-manager" | "port-manager" | "wsl-desktop" | "webhook-lab";
  expiresAtMs: number;
  leaseUntilMs: number;
  source: SourceSummary;
}

export type ExportedText = import("../generated/ExportedText").ExportedText;

/** Result of publishing an explicit log selection to Developer Toolbox. */
export interface ToolboxDispatch {
  handoffId: string;
  redacted: boolean;
}
