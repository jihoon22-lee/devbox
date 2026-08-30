import { invoke } from "@tauri-apps/api/core";
import catalogJson from "../../catalog.json";
import { isTauri } from "./lib/isTauri";
import type {
  ContentResult,
  FileEntry,
  IndexStatus,
  RootInfo,
  RootStatus,
  SavedQuery,
  SaveSavedQueryRequest,
  SearchFilter,
} from "./types";

export type OpenTarget =
  | { kind: "path"; path: string; line: number | null; column: number | null }
  | { kind: "profile"; id: string }
  | { kind: "workspace"; path: string }
  | { kind: "query"; text: string; filter?: SearchFilter | null }
  | { kind: "task"; id: string }
  | { kind: "install"; appId: string };

export interface OpenRequest {
  target: OpenTarget;
  from: string | null;
}

export interface EverythingOpenTarget {
  id: string;
  displayName: string;
}

const MOCK_CATALOG_APPS = catalogJson.apps as Array<{
  id: string;
  displayName: string;
  accepts: string[];
}>;

const MOCK_OPEN_TARGETS: EverythingOpenTarget[] = MOCK_CATALOG_APPS
  .filter((app) => app.id !== "everything-plus" && app.accepts.includes("path"))
  .map(({ id, displayName }) => ({ id, displayName }));

const MOCK_FILES: FileEntry[] = [
  { id: 1, path: "C:\\projects\\devbox\\PLAN.md", name: "PLAN.md", ext: "md", size: 3555, modified_ts: 0 },
  { id: 2, path: "C:\\projects\\devbox\\apps\\port-manager\\src\\App.tsx", name: "App.tsx", ext: "tsx", size: 5120, modified_ts: 0 },
  { id: 3, path: "C:\\projects\\devbox\\src-tauri\\tauri.conf.json", name: "tauri.conf.json", ext: "json", size: 1100, modified_ts: 0 },
];

const MOCK_CONTENT: ContentResult[] = [
  {
    path: "C:\\notes\\meeting.md",
    name: "meeting.md",
    snippet: "quarterly [review] with the team",
    ext: "md",
    size: 128,
    modified_ts: 0,
    root_id: 1,
    content_status: "indexed",
    truncated: false,
  },
];

let mockSavedQueries: SavedQuery[] = [];

const MOCK_STATUS: IndexStatus = {
  indexing: false,
  cancel_requested: false,
  total_files: 42317,
  indexed_files: 42317,
  content_indexed_files: 11840,
  content_truncated_files: 2,
  content_failed_files: 6,
  roots: 2,
  last_indexed_at: null,
  last_error: null,
};

export async function takePendingOpen(): Promise<OpenRequest | null> {
  if (!isTauri()) return null;
  return invoke<OpenRequest | null>("take_pending_open");
}

export async function onOpenRequest(cb: (request: OpenRequest) => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen<OpenRequest>("devbox://open", (event) => cb(event.payload));
}

function filterIsEmpty(filter?: SearchFilter): boolean {
  return !filter || (
    !(filter.extensions?.length) &&
    filter.modifiedAfter == null &&
    filter.modifiedBefore == null &&
    filter.minSize == null &&
    filter.maxSize == null &&
    filter.sourceRootId == null &&
    !filter.contentStatus
  );
}

function matchesFilter(file: FileEntry | ContentResult, filter?: SearchFilter): boolean {
  if (filterIsEmpty(filter)) return true;
  if (filter?.extensions?.length && !filter.extensions.includes(file.ext ?? "")) return false;
  if (filter?.modifiedAfter != null && (file.modified_ts ?? 0) < filter.modifiedAfter) return false;
  if (filter?.modifiedBefore != null && (file.modified_ts ?? 0) > filter.modifiedBefore) return false;
  if (filter?.minSize != null && (file.size ?? 0) < filter.minSize) return false;
  if (filter?.maxSize != null && (file.size ?? 0) > filter.maxSize) return false;
  if (filter?.sourceRootId != null && file.root_id !== filter.sourceRootId) return false;
  if (filter?.contentStatus) {
    const status = file.content_status;
    const truncated = file.truncated ?? (
      "content_truncated" in file ? file.content_truncated : false
    );
    if (filter.contentStatus === "not_indexed" && status) return false;
    if (filter.contentStatus === "failed" && (!status || status === "indexed")) return false;
    if ((filter.contentStatus === "truncated" || filter.contentStatus === "partial") &&
      (status !== "indexed" || !truncated)) return false;
    if (filter.contentStatus === "indexed" && (status !== "indexed" || truncated)) return false;
    if (!["not_indexed", "failed", "truncated", "partial", "indexed"].includes(filter.contentStatus) && status !== filter.contentStatus) return false;
  }
  return true;
}

function invokeSearchArgs(query: string, limit: number | undefined, filter?: SearchFilter) {
  const args: { query: string; limit: number; filter?: SearchFilter } = { query, limit: limit ?? 200 };
  if (!filterIsEmpty(filter)) args.filter = filter;
  return args;
}

export async function searchFiles(query: string, limit?: number, filter?: SearchFilter): Promise<FileEntry[]> {
  if (!isTauri()) {
    return MOCK_FILES.filter((f) => f.name.toLowerCase().includes(query.toLowerCase()) && matchesFilter(f, filter));
  }
  return invoke<FileEntry[]>("search_files", invokeSearchArgs(query, limit, filter));
}

export async function searchContent(query: string, limit?: number, filter?: SearchFilter): Promise<ContentResult[]> {
  if (!isTauri()) {
    return MOCK_CONTENT.filter((f) => f.snippet.toLowerCase().includes(query.toLowerCase()) && matchesFilter(f, filter));
  }
  return invoke<ContentResult[]>("search_content", invokeSearchArgs(query, limit, filter));
}

export async function addRoot(path: string, indexContent: boolean): Promise<void> {
  if (!isTauri()) return;
  await invoke("add_root", { path, indexContent });
}

export async function removeRoot(path: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("remove_root", { path });
}

export async function listRoots(): Promise<RootInfo[]> {
  if (!isTauri()) return [{ id: 1, path: "C:\\projects\\devbox", content: true }];
  return invoke<RootInfo[]>("list_roots");
}

export async function indexStatus(): Promise<IndexStatus> {
  if (!isTauri()) return MOCK_STATUS;
  return invoke<IndexStatus>("index_status");
}

export async function indexNow(): Promise<void> {
  if (!isTauri()) return;
  await invoke("index_now");
}

export async function cancelIndex(): Promise<void> {
  if (!isTauri()) return;
  await invoke("cancel_index");
}

export async function watcherStatuses(): Promise<RootStatus[]> {
  if (!isTauri()) {
    return [{
      root: "C:\\projects\\devbox",
      sourceKind: "native",
      watchMode: "native",
      lastSyncedAt: Date.now(),
      pending: 0,
      error: null,
    }];
  }
  return invoke<RootStatus[]>("watcher_statuses");
}

export async function openFile(path: string): Promise<void> {
  if (!isTauri()) {
    window.open("about:blank", "_blank");
    return;
  }
  await invoke("open_file", { path });
}

export async function revealFile(path: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("reveal_file", { path });
}

export async function copyPath(path: string): Promise<void> {
  await navigator.clipboard.writeText(path);
}

export async function openTargets(): Promise<EverythingOpenTarget[]> {
  if (!isTauri()) return MOCK_OPEN_TARGETS;
  return invoke<EverythingOpenTarget[]>("open_targets");
}

export async function openIn(appId: string, path: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("open_in", { appId, path });
}

export async function listSavedQueries(): Promise<SavedQuery[]> {
  if (!isTauri()) return [...mockSavedQueries];
  return invoke<SavedQuery[]>("list_saved_queries");
}

export async function saveSavedQuery(request: SaveSavedQueryRequest): Promise<SavedQuery> {
  if (!isTauri()) {
    const now = Date.now();
    const saved: SavedQuery = {
      id: request.id ?? (mockSavedQueries.reduce((max, item) => Math.max(max, item.id), 0) + 1),
      name: request.name.trim(),
      query: request.query.trim(),
      filter: request.filter,
      createdAt: request.id ? (mockSavedQueries.find((item) => item.id === request.id)?.createdAt ?? now) : now,
      updatedAt: now,
    };
    mockSavedQueries = [saved, ...mockSavedQueries.filter((item) => item.id !== saved.id)];
    return saved;
  }
  return invoke<SavedQuery>("save_saved_query", { request });
}

export async function deleteSavedQuery(id: number): Promise<void> {
  if (!isTauri()) {
    mockSavedQueries = mockSavedQueries.filter((item) => item.id !== id);
    return;
  }
  await invoke("delete_saved_query", { id });
}
