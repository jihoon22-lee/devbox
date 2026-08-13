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
