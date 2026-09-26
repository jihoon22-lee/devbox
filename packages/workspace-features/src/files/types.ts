export type EncodingKind = "utf8" | "utf16Le" | "utf16Be" | "cp949";

export interface Encoding {
  encodingKind: EncodingKind;
  bom: boolean;
}

export type LineEnding = "lf" | "crlf" | "cr";

export interface OpenedFile {
  nativeRevision?: string | null;
  path: string;
  text: string;
  encoding: Encoding;
  lineEnding: LineEnding;
  readOnly: boolean;
  size: number;
  /** Decimal epoch nanoseconds. Keep this as a string: JS numbers lose i64 precision. */
  mtimeNanos: string;
  /** SHA-256 of the exact bytes read from disk. */
  contentHash: string;
  lossy: boolean;
  /** A non-fatal warning emitted while syncing the file to disk. */
  durabilityWarning?: string | null;
}

export interface SavedFile {
  nativeRevision?: string | null;
  path: string;
  /** Decimal epoch nanoseconds. Keep this as a string: JS numbers lose i64 precision. */
  mtimeNanos: string;
  size: number;
  /** SHA-256 of the exact bytes committed; use this for the next save snapshot. */
  contentHash: string;
  /** Present only when the content committed but a durability refresh failed. */
  durabilityWarning: string | null;
}

export interface WorkspaceFile {
  path: string;
  relativePath: string;
  size: number;
}

export interface WorkspaceFiles {
  files: WorkspaceFile[];
  truncated: boolean;
  /** True when a bounded walk could not read every entry. */
  incomplete: boolean;
}

export interface WorkspaceCapabilities {
  /** Canonical path used by every subsequent workspace operation. */
  path: string;
  sourceKind: "native" | "wsl";
  watchMode: "native" | "polling";
  editSupported: boolean;
  lspSupported: boolean;
  lspReason: "host_lsp_wsl_unsupported" | "project_untrusted" | null;
}

export interface FileChangedEvent {
  contextKey?: string;
  path: string;
  mtimeNanos: string;
  contentHash: string;
  size: number;
}

/**
 * Inbound cross-app open request (`crates/applink::OpenTarget`/`OpenRequest`).
 * `Option` fields serialize as `null`, never omitted, so every optional field
 * here is typed `| null` rather than `?:`
 * (`docs/superpowers/specs/2026-08-17-app-interop-design.md` §1.2).
 */
export type OpenTarget = import("../generated/OpenTarget").OpenTarget;
export type OpenRequest = import("../generated/OpenRequest").OpenRequest;

export interface MarkdownPreviewResponse {
  kind: "markdown";
  html: string;
  mermaid: string[];
  source?: null;
}

export interface MermaidPreviewResponse {
  kind: "mermaid";
  html?: null;
  mermaid?: [];
  source: string;
}

export type PreviewResponse = MarkdownPreviewResponse | MermaidPreviewResponse;

/** The two editor views are fixed; a third split is deliberately impossible. */
export type ViewId = 0 | 1;
export type DocId = string;

/**
 * A document is the in-memory editor model. `text` is intentionally not part of
 * the session payload; it is reconstructed from disk when a session is restored.
 */
export interface Doc {
  nativeRevision?: string | null;
  id: DocId;
  path: string;
  text: string;
  encoding: Encoding;
  lineEnding: LineEnding;
  readOnly: boolean;
  size: number;
  /** Decimal epoch nanoseconds kept as a string at the JS boundary. */
  mtimeNanos: string;
  /** Content hash of the disk snapshot represented by mtime/size. */
  contentHash: string;
  lossy: boolean;
  durabilityWarning?: string | null;
  dirty: boolean;
  /** Incremented for each local buffer change; used to match an in-flight save. */
  revision: number;
  /** Session-compatible cursor and bookmark metadata. */
  cursor: number;
  bookmarks: number[];
}

/**
 * Frontend state mirrors the native session shape. `text`, file metadata and
 * `split` are runtime state; session serialization omits the buffer and split flag.
 */
export interface EditorState {
  docs: Doc[];
  views: [DocId[], DocId[]];
  activeView: ViewId;
  activeDocByView: [DocId | null, DocId | null];
  workspaceFolder: string | null;
  recentFiles: string[];
  split: boolean;
}

/** The persisted portion is compatible with core::session::Session. */
export interface SessionDoc {
  id: DocId;
  path: string;
  cursor: number;
  bookmarks: number[];
}

export interface SessionState {
  version: 1;
  workspace_folder: string | null;
  docs: SessionDoc[];
  views: [DocId[], DocId[]];
  active_view: ViewId;
  active_doc_by_view: [DocId | null, DocId | null];
  recent_files: string[];
}

export interface LoadedSession {
  session: SessionState;
  persistAllowed: boolean;
  nativeRevision?: string;
}

export type LspClientStatus = "starting" | "ready" | "degraded" | "stopped" | "crashed";
export type LspPositionEncoding = "utf-16" | "utf-8";

export interface LspPosition {
  line: number;
  character: number;
}

export interface EditedLspDocument {
  uri: string;
  version: number;
  text: string;
}

export interface AppliedDocumentEdits {
  documents: EditedLspDocument[];
}

export interface LspRenamePreviewFile {
  /** Workspace-relative display path returned by the native boundary. */
  path: string;
  ranges: Array<{
    range: LspDiagnosticRange;
    newText: string;
  }>;
  before: string;
  after: string;
}

export interface LspRenamePreview {
  planId: string;
  files: LspRenamePreviewFile[];
}

export type LspRenameFileStatus = "applied" | "rolledBack" | "failed" | "notApplied" | "conflict" | "rollbackFailed";

export interface LspRenameFileResult {
  nativeRevision?: string | null;
  path: string;
  status: LspRenameFileStatus;
  mtimeNanos: string | null;
  size: number | null;
  contentHash: string | null;
  error: string | null;
}

export interface LspRenameApplyResult {
  planId: string;
  success: boolean;
  rolledBack: boolean;
  files: LspRenameFileResult[];
  documents: RenamedLspDocument[];
  error: string | null;
}

export interface RenamedLspDocument {
  nativeRevision?: string | null;
  /** Workspace-relative path; native absolute URIs stay out of this result. */
  path: string;
  version: number;
  text: string;
}

export interface LspCapabilities {
  positionEncoding: LspPositionEncoding;
  legacyPositionEncoding: boolean;
  syncKind: "none" | "full" | "incremental" | null;
  openClose: boolean;
  save: boolean;
  completion: boolean;
  hover: boolean;
  definition: boolean;
  references: boolean;
  rename: boolean;
  formatting: boolean;
  diagnostics: boolean;
}

export interface LanguageServerStatus {
  languageId: string;
  status: LspClientStatus;
  processState: string;
  serverInfo: { name: string; version: string | null } | null;
  capabilities: LspCapabilities;
  documentCount: number;
  restartAttempt?: number;
  restartFailures?: number;
  restartDelayMs?: number | null;
  autoRestartDisabled?: boolean;
}

export type LspLogLevel = "info" | "warning" | "error";

export interface LspLogEntry {
  sequence: string;
  level: LspLogLevel;
  code: string;
  message: string;
}

export interface LanguageServerLog {
  languageId: string;
  entries: LspLogEntry[];
  droppedEntries: number;
  droppedStderrBytes: number;
  stderrTruncated: boolean;
}

export interface LspRequestMetadata {
  uri: string;
  version: number;
}

export interface LspDiagnosticRange {
  start: LspPosition;
  end: LspPosition;
}

export interface LspDiagnostic {
  range: LspDiagnosticRange;
  severity?: number | null;
  code?: string | null;
  source?: string | null;
  message: string;
}

export type LspDiagnosticOrigin = "push" | "pull";

export interface LspDiagnosticResult {
  uri: string;
  version: number | null;
  diagnostics: LspDiagnostic[];
  origin: LspDiagnosticOrigin;
  resultId?: string | null;
  unchanged?: boolean;
  stale?: boolean;
}

export interface LspFeatureResponse<T> {
  metadata: LspRequestMetadata;
  value: T;
  stale: boolean;
}

export interface LspCompletionItem {
  label: string;
  kind?: number | null;
  detail?: string | null;
  documentation?: string | { kind: "markdown" | "plaintext"; value: string } | null;
  sortText?: string | null;
  filterText?: string | null;
  insertText?: string | null;
  /** LSP InsertTextFormat: 2 is a snippet, which Code Pad treats as plain label text. */
  insertTextFormat?: number | null;
  textEdit?: unknown;
  /** Deliberately ignored until a multi-range completion transaction exists. */
  additionalTextEdits?: unknown;
}

export interface LspCompletionResult {
  isIncomplete: boolean;
  items: LspCompletionItem[];
}

export interface LspHoverResult {
  text: string;
  markdown: boolean;
  range?: LspDiagnosticRange | null;
}

export interface LspLocationTarget {
  uri: string;
  range: LspDiagnosticRange;
  selectionRange?: LspDiagnosticRange | null;
}

export interface LspFilteredLocations {
  locations: LspLocationTarget[];
  rejected: number;
}

export interface LspDiagnosticsEvent {
  nativeContext?: unknown;
  languageId: string;
  response: LspFeatureResponse<LspDiagnosticResult>;
}

export interface LspStatusEvent {
  nativeContext?: unknown;
  languageId: string;
  status: LanguageServerStatus;
  reason: string | null;
  restarting: boolean;
}

/** Results returned by the native document-sync boundary. */
export interface LspDidOpen {
  uri: string;
  languageId: string;
  version: number;
  text: string;
}

export interface LspDidChange {
  uri: string;
  version: number;
  contentChanges: Array<{ range?: unknown; text: string }>;
}

export interface LspDidSave {
  uri: string;
  version: number;
}

export interface LspDidClose {
  uri: string;
}

export type LspServerRef = import("../generated/ServerRef").ServerRef;

export type LspCustomServer = import("../generated/CustomServer").CustomServer;

/** Persisted schema intentionally uses snake_case and is passed through unchanged. */
export type LspConfig = import("../generated/LspConfig").LspConfig;

export interface LoadedLspConfig {
  nativeRevision?: string | null;
  recoveryAllowed?: boolean;
  config: LspConfig;
  persist_allowed: boolean;
  error: string | null;
}

export type ManagedInstallState = "not_installed" | "installed" | "needs_reinstall";

export type ManagedInstallSource = "network" | "archive_cache" | "local_archive" | "unknown";

export type ManagedArtifact = import("../generated/Artifact").Artifact;

export type ManagedServerManifest = import("../generated/ServerManifest").ServerManifest;

/** Safe managed-install metadata returned by lsp_installed. Paths stay in the
 * process-owned index and are never exposed to the UI. */
export type InstalledServerMetadata = import("../generated/InstalledServerMetadata").InstalledServerMetadata;

export type ManagedInstallStatus = import("../generated/ManagedInstallStatus").ManagedInstallStatus;

export function displayNameForPath(path: string): string {
  const normalized = path.split("\\").join("/");
  return normalized.split("/").pop() || path;
}

/** Stable DOM IDs shared by tab buttons and their tab panels. */
export function tabIdForDoc(docId: DocId): string {
  return `code-pad-tab-${encodeURIComponent(docId)}`;
}

export function panelIdForDoc(docId: DocId): string {
  return `code-pad-editor-${encodeURIComponent(docId)}`;
}
