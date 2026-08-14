// collection 저장·조회 순수 로직.
//
// [설계] 수명 정책 분리:
// - history: 자동·단기·상한(50) — 이 파일과 무관 (App.tsx가 localStorage "apip-history" 처리)
// - collection: 사용자가 명시적으로 저장한 재사용 자산 — "apip-collections"에 영구 보관
// 두 저장소를 완전히 분리한다.

import type { ApiRequest } from "../types";

export const COLLECTION_LS_KEY = "apip-collections";

export interface CollectionEntry {
  id: string;
  name: string;
  folder: string;
  saved_at: number;
  request: ApiRequest;
}

export interface CollectionStore {
  version: number;
  collections: CollectionEntry[];
}

export const COLLECTION_VERSION = 1;

export function emptyStore(): CollectionStore {
  return { version: COLLECTION_VERSION, collections: [] };
}

export function loadStore(): CollectionStore {
  try {
    const parsed = JSON.parse(localStorage.getItem(COLLECTION_LS_KEY) ?? "null") as CollectionStore | null;
    if (parsed && parsed.version === COLLECTION_VERSION && Array.isArray(parsed.collections)) {
      return parsed;
    }
  } catch {
    // 손상된 저장소는 빈 스토어로 시작
  }
  return emptyStore();
}

export function saveStore(store: CollectionStore): void {
  localStorage.setItem(COLLECTION_LS_KEY, JSON.stringify(store));
}

export function addEntry(
  store: CollectionStore,
  input: { name: string; folder: string; request: ApiRequest },
  now: number,
  makeId: () => string,
): CollectionStore {
  const entry: CollectionEntry = {
    id: makeId(),
    name: input.name.trim() || input.request.url || "untitled",
    folder: input.folder.trim(),
    saved_at: now,
    request: input.request,
  };
  return { ...store, collections: [entry, ...store.collections] };
}

export function removeEntry(store: CollectionStore, id: string): CollectionStore {
  return { ...store, collections: store.collections.filter((c) => c.id !== id) };
}

export function foldersOf(store: CollectionStore): string[] {
  const folders = new Set(store.collections.map((c) => c.folder).filter(Boolean));
  return [...folders].sort();
}
