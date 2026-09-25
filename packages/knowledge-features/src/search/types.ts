export interface SourceResult {
  reference?: string | null;
  source?: string;
  sourceRoot?: string;
  availability?: string;
  indexStale?: boolean;
}

export type FileEntry = import("../generated/FileEntry").FileEntry & SourceResult;

export type ContentResult = import("../generated/ContentResult").ContentResult & SourceResult;

export type SearchFilter = import("../generated/SearchFilter").SearchFilter;

export type OpenQueryFilter = SearchFilter;

export type SavedQuery = import("../generated/SavedQuery").SavedQuery;

export type SaveSavedQueryRequest = import("../generated/SaveSavedQueryRequest").SaveSavedQueryRequest;

export type RootInfo = import("../generated/RootInfo").RootInfo;

export type IndexStatus = import("../generated/IndexStatus").IndexStatus;

export type RootStatus = import("../generated/RootStatus").RootStatus;
