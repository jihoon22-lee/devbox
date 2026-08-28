export interface FileEntry {
  id: number;
  path: string;
  name: string;
  ext: string;
  size: number;
  modified_ts: number;
  root_id?: number | null;
  content_status?: string | null;
  content_truncated?: boolean;
  truncated?: boolean;
}

export interface ContentResult {
  path: string;
  name: string;
  snippet: string;
  ext?: string;
  size?: number;
  modified_ts?: number;
  root_id?: number | null;
  content_status?: string;
  truncated?: boolean;
  error_code?: string | null;
  extractor_version?: string;
  indexed_at?: number | null;
  encoding?: string | null;
  text_chars?: number;
}

export interface SearchFilter {
  extensions?: string[];
  modifiedAfter?: number;
  modifiedBefore?: number;
  minSize?: number;
  maxSize?: number;
  sourceRootId?: number;
  contentStatus?: string;
}

export type OpenQueryFilter = SearchFilter;

export interface SavedQuery {
  id: number;
  name: string;
  query: string;
  filter: SearchFilter;
  createdAt: number;
  updatedAt: number;
}

export interface SaveSavedQueryRequest {
  id?: number;
  name: string;
  query: string;
  filter: SearchFilter;
}

export interface RootInfo {
  id: number;
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
