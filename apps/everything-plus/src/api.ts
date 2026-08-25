import { invoke } from "@tauri-apps/api/core";
import catalogJson from "../../catalog.json";
import { isTauri } from "./lib/isTauri";
import type { ContentResult, FileEntry, IndexStatus, RootInfo, RootStatus } from "./types";

export type OpenTarget =
  | { kind: "path"; path: string; line: number | null; column: number | null }
  | { kind: "profile"; id: string }
  | { kind: "workspace"; path: string }
  | { kind: "query"; text: string };

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
  { path: "C:\\notes\\meeting.md", name: "meeting.md", snippet: "quarterly [review] with the team" },
];

const MOCK_STATUS: IndexStatus = { indexing: false, total_files: 42317, indexed_files: 42317, roots: 2, last_indexed_at: null };

export async function takePendingOpen(): Promise<OpenRequest | null> {
  if (!isTauri()) return null;
  return invoke<OpenRequest | null>("take_pending_open");
}

export async function onOpenRequest(cb: (request: OpenRequest) => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen<OpenRequest>("devbox://open", (event) => cb(event.payload));
}

export async function searchFiles(query: string, limit?: number): Promise<FileEntry[]> {
  if (!isTauri()) {
    return MOCK_FILES.filter((f) => f.name.toLowerCase().includes(query.toLowerCase()));
  }
  return invoke<FileEntry[]>("search_files", { query, limit: limit ?? 200 });
}

export async function searchContent(query: string, limit?: number): Promise<ContentResult[]> {
  if (!isTauri()) {
    return MOCK_CONTENT.filter((f) => f.snippet.toLowerCase().includes(query.toLowerCase()));
  }
  return invoke<ContentResult[]>("search_content", { query, limit: limit ?? 200 });
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
  if (!isTauri()) return [{ path: "C:\\projects\\devbox", content: true }];
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

export async function watcherStatuses(): Promise<RootStatus[]> {
  if (!isTauri()) {
    return [{ root: "C:\\projects\\devbox", lastSyncedAt: Date.now(), pending: 0, error: null }];
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
