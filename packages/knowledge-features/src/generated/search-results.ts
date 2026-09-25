import type { ContentResult } from "./ContentResult";
import type { FileEntry } from "./FileEntry";
import type { IndexStatus } from "./IndexStatus";
import type { OpenRequest } from "./OpenRequest";
import type { RootInfo } from "./RootInfo";
import type { RootStatus } from "./RootStatus";
import type { SavedQuery } from "./SavedQuery";
import type { SourceReference } from "./SourceReference";
import type { SourceSnapshot } from "./SourceSnapshot";

export type SearchResults = {
  take_pending_open: OpenRequest | null;
  watcher_statuses: Array<RootStatus>;
  search_files: Array<FileEntry>;
  search_content: Array<ContentResult>;
  list_roots: Array<RootInfo>;
  index_status: IndexStatus;
  list_saved_queries: Array<SavedQuery>;
  source_query: SourceSnapshot;
  source_poll: SourceSnapshot;
  source_cancel: null;
  source_reference: SourceReference;
  source_saved_reference: SavedQuery;
};
