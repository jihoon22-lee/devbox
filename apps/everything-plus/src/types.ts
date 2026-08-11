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
  total_files: number;
  indexed_files: number;
  roots: number;
  last_indexed_at: number | null;
}
