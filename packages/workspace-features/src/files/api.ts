import { componentInvoke, isProductHosted } from "../transport";
import { typedCall } from "../typed";
import type { WorkspaceFilesCall } from "../generated/WorkspaceFilesCall";
import type { WorkspaceLspCall } from "../generated/WorkspaceLspCall";
import type { FilesResults } from "../generated/files-results";
import type { LspResults } from "../generated/lsp-results";
const owner = (method: string) =>
  method.startsWith("lsp_") || method.includes("_lsp_") || method.includes("language_server")
    ? ("workspace.lsp" as const)
    : ("workspace.files" as const);
const invoke = typedCall<WorkspaceFilesCall | WorkspaceLspCall, FilesResults & LspResults>(owner);
const rawInvoke = componentInvoke(owner);
function legacyInvoke<T>(method: string, args?: Record<string, unknown>): Promise<T> {
  if (isProductHosted()) return Promise.reject(new Error("Workspace에서 지원하지 않는 작업입니다."));
  return rawInvoke<T>(method, args);
}
function requiredRevision(value: string | null | undefined): string {
  if (typeof value !== "string") throw new Error("파일 상태를 다시 확인해 주세요.");
  return value;
}
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  Encoding,
  LineEnding,
  OpenedFile,
  PreviewResponse,
  SavedFile,
  LoadedSession,
  SessionState,
  WorkspaceCapabilities,
  WorkspaceFiles,
  LanguageServerStatus,
  LoadedLspConfig,
  LspConfig,
  AppliedDocumentEdits,
  LspRenameApplyResult,
  LspRenamePreview,
  LspPosition,
  LspDidChange,
  LspDidClose,
  LspDidOpen,
  LspDidSave,
  LspFeatureResponse,
  LspDiagnosticResult,
  LspCompletionResult,
  LspHoverResult,
  LspFilteredLocations,
  ManagedInstallStatus,
  ManagedServerManifest,
  LanguageServerLog,
  OpenRequest,
} from "./types";

export interface FileActionSnapshot {
  nativeRevision?: string | null;
  path: string;
  mtimeNanos: string;
  size: number;
  contentHash: string;
}

export interface RenamedFile {
  nativeRevision?: string | null;
  path: string;
  mtimeNanos: string;
  size: number;
  contentHash: string;
}

export function openFile(
  path: string,
  encoding: Encoding | null = null,
  receivedReference?: string,
): Promise<OpenedFile> {
  return invoke("open_file", {
    request: { path, encoding },
    ...(receivedReference ? { receivedReference } : {}),
  });
}

export function pickFiles(): Promise<string[]> {
  return invoke("pick_files");
}

function actionRequest(file: FileActionSnapshot) {
  return {
    path: file.path,
    expectedMtimeNanos: file.mtimeNanos,
    expectedSize: file.size,
    expectedContentHash: file.contentHash,
    ...(file.nativeRevision !== undefined ? { nativeRevision: file.nativeRevision } : {}),
  };
}

/**
 * Takes (and clears) the inbound open request left by a cold-start argv parse
 * or a single-instance relaunch, if any. `null` when nothing is pending.
 * Clearing on take means a page reload does not re-trigger the same open
 * (`docs/superpowers/specs/2026-08-17-app-interop-design.md` §3).
 */
export function takePendingOpen(): Promise<OpenRequest | null> {
  return invoke("take_pending_open");
}

export async function saveFile(
  path: string,
  text: string,
  encoding: Encoding,
  lineEnding: LineEnding,
  expectedMtimeNanos: string,
  expectedSize: number,
  expectedContentHash: string,
  sourceLossy: boolean,
  nativeRevision?: string | null,
): Promise<SavedFile> {
  if (!isProductHosted())
    return legacyInvoke("save_file", {
      request: {
        path,
        text,
        encoding,
        lineEnding,
        expectedMtimeNanos,
        expectedSize,
        expectedContentHash,
        sourceLossy,
        ...(nativeRevision !== undefined ? { nativeRevision } : {}),
      },
    });
  return invoke("save_file", {
    request: {
      path,
      text,
      encoding,
      lineEnding,
      expectedMtimeNanos,
      expectedSize,
      expectedContentHash,
      sourceLossy,
      nativeRevision: requiredRevision(nativeRevision),
    },
  });
}

export function validateEncoding(text: string, encoding: Encoding): Promise<void> {
  return Promise.resolve(invoke("validate_encoding", { request: { text, encoding } })).then(() => undefined);
}

export async function renameFileAction(file: FileActionSnapshot, newName: string): Promise<RenamedFile> {
  if (!isProductHosted()) return legacyInvoke("rename_file_action", { request: { ...actionRequest(file), newName } });
  return invoke("rename_file_action", {
    request: { ...actionRequest(file), nativeRevision: requiredRevision(file.nativeRevision), newName },
  });
}

export async function deleteFileAction(file: FileActionSnapshot): Promise<void> {
  if (!isProductHosted()) return legacyInvoke("delete_file_action", { request: actionRequest(file) });
  return Promise.resolve(
    invoke("delete_file_action", {
      request: { ...actionRequest(file), nativeRevision: requiredRevision(file.nativeRevision) },
    }),
  ).then(() => undefined);
}

export function revealFileAction(path: string): Promise<void> {
  return Promise.resolve(invoke("reveal_file_action", { path })).then(() => undefined);
}

export function readClipboardText(): Promise<string> {
  return isProductHosted() ? invoke("read_clipboard_text") : readText();
}

export function listWorkspaceFiles(path: string): Promise<WorkspaceFiles> {
  return invoke("list_workspace_files", { path });
}

export function canonicalizeWorkspace(path: string): Promise<string> {
  return invoke("canonicalize_workspace", { path });
}

export function workspaceCapabilities(path: string): Promise<WorkspaceCapabilities> {
  return invoke("workspace_capabilities", { path });
}

export function watchFile(path: string): Promise<void> {
  return Promise.resolve(invoke("watch_file", { path })).then(() => undefined);
}

export function unwatchFile(path: string, contextKey?: string): Promise<void> {
  return Promise.resolve(
    invoke("unwatch_file", {
      path,
      ...(isProductHosted() && contextKey && contextKey !== "standalone"
        ? { documentContext: JSON.parse(contextKey) }
        : {}),
    }),
  ).then(() => undefined);
}

export function loadSession(): Promise<LoadedSession> {
  return invoke("load_session");
}

export async function saveSession(session: SessionState, nativeRevision?: string): Promise<string | undefined> {
  if (!isProductHosted()) {
    await legacyInvoke<void>("save_session", { session });
    return undefined;
  }
  const result = await invoke("save_session", { session, nativeRevision: requiredRevision(nativeRevision) });
  return result.nativeRevision;
}

export async function renderPreview(path: string, content: string, workspaceRoot: string): Promise<PreviewResponse> {
  const preview = await invoke("render_preview", { path, content, workspaceRoot });
  if (preview.kind === "markdown" && typeof preview.html === "string")
    return { kind: "markdown", html: preview.html, mermaid: preview.mermaid };
  if (preview.kind === "mermaid" && typeof preview.source === "string")
    return { kind: "mermaid", source: preview.source };
  throw new Error("미리보기를 확인하지 못했습니다.");
}

export function loadLspConfig(): Promise<LoadedLspConfig> {
  return invoke("load_lsp_config");
}

export async function saveLspConfig(
  config: LspConfig,
  recoverInvalid = false,
  nativeRevision?: string | null,
): Promise<void> {
  if (!isProductHosted()) return legacyInvoke<void>("save_lsp_config", { config, recoverInvalid });
  return Promise.resolve(
    invoke("save_lsp_config", { config, recoverInvalid, nativeRevision: requiredRevision(nativeRevision) }),
  ).then(() => undefined);
}

export function startLanguageServer(languageId: string): Promise<void> {
  return startServer("start_language_server", languageId);
}

export function stopLanguageServer(languageId: string): Promise<void> {
  const operationId = isProductHosted() ? pendingStarts.get(languageId)?.operationId : undefined;
  return Promise.resolve(invoke("stop_language_server", { languageId, ...(operationId ? { operationId } : {}) })).then(
    () => undefined,
  );
}

export function restartLanguageServer(languageId: string): Promise<void> {
  return startServer("restart_language_server", languageId);
}

const pendingStarts = new Map<string, { operationId: string; promise: Promise<void> }>();
function startServer(method: "start_language_server" | "restart_language_server", languageId: string): Promise<void> {
  if (!isProductHosted()) return legacyInvoke<void>(method, { languageId });
  const pending = pendingStarts.get(languageId);
  if (pending) return pending.promise;
  const operationId = crypto.randomUUID();
  // Publish cancellation identity before the first invoke/description await.
  const promise = Promise.resolve()
    .then(() => Promise.resolve(invoke(method, { languageId, operationId })).then(() => undefined))
    .finally(() => {
      if (pendingStarts.get(languageId)?.operationId === operationId) pendingStarts.delete(languageId);
    });
  pendingStarts.set(languageId, { operationId, promise });
  return promise;
}

export async function languageServerStatuses(): Promise<LanguageServerStatus[]> {
  const statuses = await invoke("language_server_statuses");
  return statuses.map((status) => ({
    ...status,
    capabilities: {
      ...status.capabilities,
      positionEncoding: status.capabilities.positionEncoding === "utf8" ? "utf-8" : "utf-16",
    },
  }));
}

export function languageServerLogs(): Promise<LanguageServerLog[]> {
  return invoke("language_server_logs");
}

export async function openLspDocument(
  languageId: string,
  path: string,
  text: string,
  nativeRevision?: string | null,
): Promise<LspDidOpen> {
  if (!isProductHosted()) return legacyInvoke<LspDidOpen>("open_lsp_document", { languageId, path, text });
  return invoke("open_lsp_document", {
    languageId,
    path,
    text,
    nativeRevision: requiredRevision(nativeRevision),
  });
}

export async function changeLspDocument(
  languageId: string,
  uri: string,
  text: string,
  dirty: boolean,
  nativeRevision?: string | null,
): Promise<LspDidChange> {
  if (!isProductHosted()) return legacyInvoke<LspDidChange>("change_lsp_document", { languageId, uri, text, dirty });
  return invoke("change_lsp_document", {
    languageId,
    uri,
    text,
    dirty,
    nativeRevision: requiredRevision(nativeRevision),
  });
}

export async function reloadLspDocument(
  languageId: string,
  uri: string,
  text: string,
  nativeRevision?: string | null,
): Promise<LspDidChange> {
  if (!isProductHosted()) return legacyInvoke<LspDidChange>("reload_lsp_document", { languageId, uri, text });
  return invoke("reload_lsp_document", {
    languageId,
    uri,
    text,
    nativeRevision: requiredRevision(nativeRevision),
  });
}

export async function saveLspDocument(
  languageId: string,
  uri: string,
  nativeRevision?: string | null,
  text?: string,
): Promise<LspDidSave> {
  if (!isProductHosted()) return legacyInvoke<LspDidSave>("save_lsp_document", { languageId, uri });
  return invoke("save_lsp_document", {
    languageId,
    uri,
    nativeRevision: requiredRevision(nativeRevision),
    ...(text !== undefined ? { text } : {}),
  });
}

export function closeLspDocument(languageId: string, uri: string): Promise<LspDidClose> {
  return invoke("close_lsp_document", { languageId, uri });
}

export function lspCatalog(): Promise<ManagedServerManifest[]> {
  return invoke("lsp_catalog");
}

export function lspInstalled(): Promise<ManagedInstallStatus[]> {
  return invoke("lsp_installed");
}

export function installLsp(manifestId: string, version: string, platform: string): Promise<void> {
  return Promise.resolve(invoke("lsp_install", { manifestId, version, platform })).then(() => undefined);
}

export async function pickLspArchives(): Promise<string[]> {
  if (isProductHosted()) return invoke("pick_lsp_archives");
  const selected = await open({
    directory: false,
    multiple: true,
    title: "관리형 LSP archive 선택",
    filters: [{ name: "LSP archive", extensions: ["zip", "tgz", "tar.gz"] }],
  });
  if (Array.isArray(selected)) return selected;
  return typeof selected === "string" ? [selected] : [];
}

/** Release opaque product picker choices. Standalone paths have no native lease. */
export async function discardLspArchives(archivePaths: string[]): Promise<void> {
  if (isProductHosted() && archivePaths.length > 0) {
    await Promise.resolve(invoke("discard_lsp_archives", { archivePaths })).then(() => undefined);
  }
}

export function importLspArchives(
  manifestId: string,
  version: string,
  platform: string,
  archivePaths: string[],
): Promise<void> {
  return Promise.resolve(
    invoke("lsp_import_archive", {
      manifestId,
      version,
      platform,
      archivePaths,
    }),
  ).then(() => undefined);
}

export function uninstallLsp(manifestId: string, version: string, platform: string): Promise<void> {
  return Promise.resolve(invoke("lsp_uninstall", { manifestId, version, platform })).then(() => undefined);
}

export function recoverInstalledLsp(): Promise<void> {
  return Promise.resolve(invoke("lsp_recover_installed")).then(() => undefined);
}

export function requestLspRename(
  languageId: string,
  uri: string,
  position: LspPosition,
  newName: string,
): Promise<LspRenamePreview> {
  return invoke("request_lsp_rename", {
    languageId,
    uri,
    position,
    newName,
  });
}

export function applyLspRename(planId: string): Promise<LspRenameApplyResult> {
  return invoke("apply_lsp_rename", { planId });
}

export function cancelLspRename(planId: string): Promise<boolean> {
  return invoke("cancel_lsp_rename", { planId });
}

export function discardLspRename(planId: string): Promise<boolean> {
  return invoke("discard_lsp_rename", { planId });
}

export function requestLspFormatting(
  languageId: string,
  uri: string,
  tabSize: number,
  insertSpaces: boolean,
): Promise<AppliedDocumentEdits> {
  return invoke("request_lsp_formatting", {
    languageId,
    uri,
    tabSize,
    insertSpaces,
  });
}

export function pullLspDiagnostics(languageId: string, uri: string): Promise<LspFeatureResponse<LspDiagnosticResult>> {
  return invoke("pull_lsp_diagnostics", {
    languageId,
    uri,
  });
}

export function requestLspCompletion(
  languageId: string,
  uri: string,
  position: LspPosition,
): Promise<LspFeatureResponse<LspCompletionResult>> {
  return invoke("request_lsp_completion", {
    languageId,
    uri,
    position,
  });
}

export function requestLspHover(
  languageId: string,
  uri: string,
  position: LspPosition,
): Promise<LspFeatureResponse<LspHoverResult | null>> {
  return invoke("request_lsp_hover", {
    languageId,
    uri,
    position,
  });
}

export function requestLspDefinition(
  languageId: string,
  uri: string,
  position: LspPosition,
): Promise<LspFeatureResponse<LspFilteredLocations>> {
  return invoke("request_lsp_definition", {
    languageId,
    uri,
    position,
  });
}

export function requestLspReferences(
  languageId: string,
  uri: string,
  position: LspPosition,
  includeDeclaration = true,
): Promise<LspFeatureResponse<LspFilteredLocations>> {
  return invoke("request_lsp_references", {
    languageId,
    uri,
    position,
    includeDeclaration,
  });
}

// ── recovery (§12.1) ──────────────────────────────────────────────

export interface RecoveryEntry {
  path: string;
  content: string;
  baseHash: string | null;
  snapshotAtMs: number;
}
interface RecoveryWire {
  path: string;
  content: string;
  base_hash: string | null;
  snapshot_at_ms: number;
}

export interface LoadedRecovery {
  entries: RecoveryEntry[];
  nativeRevision?: string;
}
export async function saveRecovery(entries: RecoveryEntry[], nativeRevision?: string): Promise<string | undefined> {
  const args = {
    entries: entries.map((entry) => ({
      path: entry.path,
      content: entry.content,
      base_hash: entry.baseHash,
      snapshot_at_ms: entry.snapshotAtMs,
    })),
  };
  if (isProductHosted())
    return (await invoke("save_recovery", { ...args, nativeRevision: requiredRevision(nativeRevision) }))
      .nativeRevision;
  await legacyInvoke<void>("save_recovery", args);
}
export async function loadRecoveryState(): Promise<LoadedRecovery> {
  const hosted = isProductHosted();
  const result = hosted ? await invoke("load_recovery") : await legacyInvoke<RecoveryWire[]>("load_recovery");
  const state = hosted
    ? (result as { entries: RecoveryWire[]; nativeRevision: string })
    : { entries: result as RecoveryWire[], nativeRevision: undefined };
  return {
    entries: state.entries.map((entry) => ({
      path: entry.path,
      content: entry.content,
      baseHash: entry.base_hash,
      snapshotAtMs: entry.snapshot_at_ms,
    })),
    nativeRevision: state.nativeRevision,
  };
}
export async function loadRecovery(): Promise<RecoveryEntry[]> {
  return (await loadRecoveryState()).entries;
}
export async function discardRecovery(path: string | null, nativeRevision?: string): Promise<string | undefined> {
  if (isProductHosted())
    return (await invoke("discard_recovery", { path, nativeRevision: requiredRevision(nativeRevision) }))
      .nativeRevision;
  await legacyInvoke<void>("discard_recovery", { path });
}

export function applyRecovery(path: string, content: string): Promise<void> {
  return legacyInvoke<void>("apply_recovery", { path, content });
}
export interface RecoveryPreview {
  previewId: string;
  path: string;
  before: string;
  after: string;
}
export function prepareRecovery(path: string): Promise<RecoveryPreview> {
  return invoke("prepare_recovery", { path });
}
export function applyRecoveryPreview(previewId: string): Promise<SavedFile> {
  return invoke("apply_recovery_preview", { previewId });
}
export function cancelRecoveryPreview(previewId: string): Promise<void> {
  return Promise.resolve(invoke("cancel_recovery_preview", { previewId })).then(() => undefined);
}

/** Hosted metadata mirror; standalone editing has no product document owner. */
export function syncEditorDocument(path: string, nativeRevision: string, text: string): Promise<boolean> {
  return isProductHosted() ? invoke("sync_editor_document", { path, nativeRevision, text }) : Promise.resolve(false);
}

export interface LspExecutionPreview {
  previewId: string;
  approved: boolean;
  workspaceRoot: string;
  configRevision: string;
  commands: Array<{ languageId: string; executable: string; args: string[]; runtime: string | null }>;
  environment?: Record<string, string>;
  environmentKeys?: string[];
  definitionsDigest: string;
}
export function previewLspExecution(): Promise<LspExecutionPreview> {
  return invoke("lsp_execution_preview");
}
export function approveLspExecution(previewId: string): Promise<void> {
  return Promise.resolve(invoke("lsp_execution_approve", { previewId })).then(() => undefined);
}
export function cancelLspExecutionReview(previewId: string): Promise<void> {
  return Promise.resolve(invoke("lsp_execution_cancel", { previewId })).then(() => undefined);
}
export function revokeLspExecution(): Promise<void> {
  return Promise.resolve(invoke("lsp_execution_revoke")).then(() => undefined);
}

export interface LspRecoveryListing {
  records: Array<{ journalId: string; files: number; available: boolean }>;
  truncated: boolean;
}
export interface LspRecoveryPreview {
  previewId: string;
  journalId: string;
  files: Array<{
    path: string;
    restore: boolean;
    current: string;
    original: string;
    currentSize: number;
    originalSize: number;
  }>;
}
export interface LspRecoveryResult {
  complete: boolean;
  restored: string[];
  cleanupPending: boolean;
  error: string | null;
}
export function listLspRecovery(): Promise<LspRecoveryListing> {
  return invoke("lsp_recovery_list");
}
export function previewLspRecovery(journalId: string): Promise<LspRecoveryPreview> {
  return invoke("lsp_recovery_preview", { journalId });
}
export function applyLspRecovery(previewId: string): Promise<LspRecoveryResult> {
  return invoke("lsp_recovery_apply", { previewId });
}
export function cancelLspRecovery(previewId: string): Promise<void> {
  return Promise.resolve(invoke("lsp_recovery_cancel", { previewId })).then(() => undefined);
}

export async function sendEditorSelection(
  path: string,
  nativeRevision: string,
  text: string,
  from: number,
  to: number,
): Promise<void> {
  if (!isProductHosted()) throw new Error("Workspace에서 사용할 수 있습니다.");
  await invoke("send_editor_selection", { path, nativeRevision, text, from, to });
}
