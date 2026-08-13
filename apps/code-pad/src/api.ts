import { invoke } from "@tauri-apps/api/core";
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
  LspDidChange,
  LspDidClose,
  LspDidOpen,
  LspDidSave,
  ManagedInstallStatus,
  ManagedServerManifest,
} from "./types";

export function openFile(path: string, encoding: Encoding | null = null): Promise<OpenedFile> {
  return invoke<OpenedFile>("open_file", { request: { path, encoding } });
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

export function languageServerStatuses(): Promise<LanguageServerStatus[]> {
  return invoke<LanguageServerStatus[]>("language_server_statuses");
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
