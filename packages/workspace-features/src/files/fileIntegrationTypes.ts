import type { DocId, SavedFile } from "./types";

export interface SaveOutcome {
  saved: SavedFile;
  matchedSnapshot: boolean;
}

export interface NavEntry {
  docId: DocId;
  path: string;
  cursor: number;
}

export interface FileOpenRequest {
  id: string;
  contextKey: string;
  path: string;
  line: number | null;
  column?: number | null;
  receivedReference?: string;
}
