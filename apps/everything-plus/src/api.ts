import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { ContentResult, FileEntry, IndexStatus, RootInfo } from "./types";

const MOCK_FILES: FileEntry[] = [
  { id: 1, path: "C:\\projects\\devbox\\PLAN.md", name: "PLAN.md", ext: "md", size: 3555, modified_ts: 0 },
  { id: 2, path: "C:\\projects\\devbox\\apps\\port-manager\\src\\App.tsx", name: "App.tsx", ext: "tsx", size: 5120, modified_ts: 0 },
  { id: 3, path: "C:\\projects\\devbox\\src-tauri\\tauri.conf.json", name: "tauri.conf.json", ext: "json", size: 1100, modified_ts: 0 },
];

const MOCK_CONTENT: ContentResult[] = [
  { path: "C:\\notes\\meeting.md", name: "meeting.md", snippet: "quarterly [review] with the team" },
];

const MOCK_STATUS: IndexStatus = { indexing: false, total_files: 42317, indexed_files: 42317, roots: 2, last_indexed_at: null };

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
