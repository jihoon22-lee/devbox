import { invoke } from "@tauri-apps/api/core";
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
  path: string;
  mtimeNanos: string;
  size: number;
  contentHash: string;
}

export interface RenamedFile {
  path: string;
  mtimeNanos: string;
  size: number;
  contentHash: string;
}

export function openFile(path: string, encoding: Encoding | null = null): Promise<OpenedFile> {
  return invoke<OpenedFile>("open_file", { request: { path, encoding } });
}

/**
 * Takes (and clears) the inbound open request left by a cold-start argv parse
 * or a single-instance relaunch, if any. `null` when nothing is pending.
 * Clearing on take means a page reload does not re-trigger the same open
 * (`docs/superpowers/specs/2026-08-17-app-interop-design.md` §3).
 */
export function takePendingOpen(): Promise<OpenRequest | null> {
  return invoke<OpenRequest | null>("take_pending_open");
}

export function saveFile(
  path: string,
  text: string,
  encoding: Encoding,
  lineEnding: LineEnding,
  expectedMtimeNanos: string,
  expectedSize: number,
  expectedContentHash: string,
  sourceLossy: boolean,
): Promise<SavedFile> {
  return invoke<SavedFile>("save_file", {
    request: {
      path,
      text,
      encoding,
      lineEnding,
      expectedMtimeNanos,
      expectedSize,
      expectedContentHash,
      sourceLossy,
    },
  });
}

export function validateEncoding(text: string, encoding: Encoding): Promise<void> {
  return invoke<void>("validate_encoding", { request: { text, encoding } });
}

export function renameFileAction(
  file: FileActionSnapshot,
  newName: string,
): Promise<RenamedFile> {
  return invoke<RenamedFile>("rename_file_action", {
    request: { ...file, newName },
  });
}

export function deleteFileAction(file: FileActionSnapshot): Promise<void> {
  return invoke<void>("delete_file_action", { request: file });
}

export function revealFileAction(path: string): Promise<void> {
  return invoke<void>("reveal_file_action", { path });
}

export function readClipboardText(): Promise<string> {
  return readText();
}

export function listWorkspaceFiles(path: string): Promise<WorkspaceFiles> {
  return invoke<WorkspaceFiles>("list_workspace_files", { path });
}

export function canonicalizeWorkspace(path: string): Promise<string> {
  return invoke<string>("canonicalize_workspace", { path });
}

export function watchFile(path: string): Promise<void> {
  return invoke<void>("watch_file", { path });
}

export function unwatchFile(path: string): Promise<void> {
  return invoke<void>("unwatch_file", { path });
}

export function loadSession(): Promise<LoadedSession> {
  return invoke<LoadedSession>("load_session");
}

export function saveSession(session: SessionState): Promise<void> {
  return invoke<void>("save_session", { session });
}

export function renderPreview(
  path: string,
  content: string,
  workspaceRoot: string,
): Promise<PreviewResponse> {
  return invoke<PreviewResponse>("render_preview", {
    path,
    content,
    workspaceRoot,
  });
}

export function loadLspConfig(): Promise<LoadedLspConfig> {
  return invoke<LoadedLspConfig>("load_lsp_config");
}

export function saveLspConfig(config: LspConfig, recoverInvalid = false): Promise<void> {
  return invoke<void>("save_lsp_config", { config, recoverInvalid });
}

export function startLanguageServer(languageId: string): Promise<void> {
  return invoke<void>("start_language_server", { languageId });
}

export function stopLanguageServer(languageId: string): Promise<void> {
  return invoke<void>("stop_language_server", { languageId });
}

export function restartLanguageServer(languageId: string): Promise<void> {
  return invoke<void>("restart_language_server", { languageId });
}

export function languageServerStatuses(): Promise<LanguageServerStatus[]> {
  return invoke<LanguageServerStatus[]>("language_server_statuses");
}

export function languageServerLogs(): Promise<LanguageServerLog[]> {
  return invoke<LanguageServerLog[]>("language_server_logs");
}

export function openLspDocument(
  languageId: string,
  path: string,
  text: string,
): Promise<LspDidOpen> {
  return invoke<LspDidOpen>("open_lsp_document", { languageId, path, text });
}

export function changeLspDocument(
  languageId: string,
  uri: string,
  text: string,
  dirty: boolean,
): Promise<LspDidChange> {
  return invoke<LspDidChange>("change_lsp_document", { languageId, uri, text, dirty });
}

export function reloadLspDocument(
  languageId: string,
  uri: string,
  text: string,
): Promise<LspDidChange> {
  return invoke<LspDidChange>("reload_lsp_document", { languageId, uri, text });
}

export function saveLspDocument(languageId: string, uri: string): Promise<LspDidSave> {
  return invoke<LspDidSave>("save_lsp_document", { languageId, uri });
}

export function closeLspDocument(languageId: string, uri: string): Promise<LspDidClose> {
  return invoke<LspDidClose>("close_lsp_document", { languageId, uri });
}

export function lspCatalog(): Promise<ManagedServerManifest[]> {
  return invoke<ManagedServerManifest[]>("lsp_catalog");
}

export function lspInstalled(): Promise<ManagedInstallStatus[]> {
  return invoke<ManagedInstallStatus[]>("lsp_installed");
}

export function installLsp(
  manifestId: string,
  version: string,
  platform: string,
): Promise<void> {
  return invoke<void>("lsp_install", { manifestId, version, platform });
}

export async function pickLspArchives(): Promise<string[]> {
  const selected = await open({
    directory: false,
    multiple: true,
    title: "관리형 LSP archive 선택",
    filters: [{ name: "LSP archive", extensions: ["zip", "tgz", "tar.gz"] }],
  });
  if (Array.isArray(selected)) return selected;
  return typeof selected === "string" ? [selected] : [];
}

export function importLspArchives(
  manifestId: string,
  version: string,
  platform: string,
  archivePaths: string[],
): Promise<void> {
  return invoke<void>("lsp_import_archive", {
    manifestId,
    version,
    platform,
    archivePaths,
  });
}

export function uninstallLsp(
  manifestId: string,
  version: string,
  platform: string,
): Promise<void> {
  return invoke<void>("lsp_uninstall", { manifestId, version, platform });
}

export function recoverInstalledLsp(): Promise<void> {
  return invoke<void>("lsp_recover_installed");
}

export function requestLspRename(
  languageId: string,
  uri: string,
  position: LspPosition,
  newName: string,
): Promise<LspRenamePreview> {
  return invoke<LspRenamePreview>("request_lsp_rename", {
    languageId,
    uri,
    position,
    newName,
  });
}

export function applyLspRename(planId: string): Promise<LspRenameApplyResult> {
  return invoke<LspRenameApplyResult>("apply_lsp_rename", { planId });
}

export function cancelLspRename(planId: string): Promise<boolean> {
  return invoke<boolean>("cancel_lsp_rename", { planId });
}

export function discardLspRename(planId: string): Promise<boolean> {
  return invoke<boolean>("discard_lsp_rename", { planId });
}

export function requestLspFormatting(
  languageId: string,
  uri: string,
  tabSize: number,
  insertSpaces: boolean,
): Promise<AppliedDocumentEdits> {
  return invoke<AppliedDocumentEdits>("request_lsp_formatting", {
    languageId,
    uri,
    tabSize,
    insertSpaces,
  });
}

export function pullLspDiagnostics(
  languageId: string,
  uri: string,
): Promise<LspFeatureResponse<LspDiagnosticResult>> {
  return invoke<LspFeatureResponse<LspDiagnosticResult>>("pull_lsp_diagnostics", {
    languageId,
    uri,
  });
}

export function requestLspCompletion(
  languageId: string,
  uri: string,
  position: LspPosition,
): Promise<LspFeatureResponse<LspCompletionResult>> {
  return invoke<LspFeatureResponse<LspCompletionResult>>("request_lsp_completion", {
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
  return invoke<LspFeatureResponse<LspHoverResult | null>>("request_lsp_hover", {
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
  return invoke<LspFeatureResponse<LspFilteredLocations>>("request_lsp_definition", {
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
  return invoke<LspFeatureResponse<LspFilteredLocations>>("request_lsp_references", {
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

export function saveRecovery(entries: RecoveryEntry[]): Promise<void> {
  return invoke<void>("save_recovery", { entries });
}

export function loadRecovery(): Promise<RecoveryEntry[]> {
  return invoke<RecoveryEntry[]>("load_recovery");
}

export function discardRecovery(path: string | null): Promise<void> {
  return invoke<void>("discard_recovery", { path });
}

export function applyRecovery(path: string, content: string): Promise<void> {
  return invoke<void>("apply_recovery", { path, content });
}
