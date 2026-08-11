export interface FileEntry {
  id: number;
  path: string;
  name: string;
  ext: string;
  size: number;
  modified_ts: number;
}

export interface IndexStatus {
  indexing: boolean;
  total_files: number;
  roots: number;
  last_indexed_at: number | null;
}
