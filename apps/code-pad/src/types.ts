export type EncodingKind = "utf8" | "utf16Le" | "utf16Be" | "cp949";

export interface Encoding {
  encodingKind: EncodingKind;
  bom: boolean;
}

export type LineEnding = "lf" | "crlf" | "cr";

export interface OpenedFile {
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
  path: string;
  /** Decimal epoch nanoseconds. Keep this as a string: JS numbers lose i64 precision. */
  mtimeNanos: string;
  size: number;
  /** SHA-256 of the exact bytes committed; use this for the next save snapshot. */
  contentHash: string;
  /** Present only when the content committed but a durability refresh failed. */
  durabilityWarning: string | null;
}

/** The two editor views are fixed; a third split is deliberately impossible. */
export type ViewId = 0 | 1;
export type DocId = string;

/**
 * A document is the in-memory editor model. `text` is intentionally not part of
 * the session payload; it is reconstructed from disk when a session is restored.
 */
export interface Doc {
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
  /** Session-compatible cursor and bookmark metadata. Bookmarks are not edited in Phase 1. */
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
