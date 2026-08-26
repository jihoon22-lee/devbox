export interface FileEntry {
  id: number;
  path: string;
  name: string;
  ext: string;
  size: number;
  modified_ts: number;
}

export interface ContentResult {
  path: string;
  name: string;
  snippet: string;
}

export interface RootInfo {
  path: string;
  content: boolean;
}

export interface IndexStatus {
  indexing: boolean;
  cancel_requested: boolean;
  total_files: number;
  indexed_files: number;
  content_indexed_files: number;
  content_truncated_files: number;
  content_failed_files: number;
  roots: number;
  last_indexed_at: number | null;
  last_error: string | null;
}

export interface RootStatus {
  root: string;
  lastSyncedAt: number | null;
  pending: number;
  error: string | null;
}
