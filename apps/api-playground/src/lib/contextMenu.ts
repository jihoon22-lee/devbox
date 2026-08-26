import type { ContextMenuEntry } from "@devbox/context-menu";
import type { HistoryItem, PersistedHistoryRequest } from "../types";
import { normalizeCookies } from "./cookies";
import type { HistoryStore } from "./persistence";

export function buildRequestItemContextMenu(disabled: boolean): readonly ContextMenuEntry[] {
  return [
    { type: "item", id: "duplicate", label: "복제", disabled },
    { type: "item", id: "rename", label: "이름 변경", disabled },
    { type: "item", id: "delete", label: "삭제", disabled, danger: true },
    { type: "item", id: "copy-curl", label: "curl 복사", disabled },
  ];
}

export function duplicateHistoryItem(
  store: HistoryStore,
  id: string,
  now: number,
  makeId: () => string,
): HistoryStore {
  const source = store.history.find((item) => item.id === id);
  if (!source) return store;
  const duplicate: HistoryItem = {
    ...source,
    id: makeId(),
    name: copyName((source.name ?? source.request.url) || "untitled"),
    saved_at: now,
    request: clonePersistedRequest(source.request),
  };
  return { ...store, history: [duplicate, ...store.history].slice(0, 50) };
}

export function renameHistoryItem(store: HistoryStore, id: string, name: string): HistoryStore {
  const normalized = normalizeName(name);
  if (!normalized) return store;
  return {
    ...store,
    history: store.history.map((item) => item.id === id ? { ...item, name: normalized } : item),
  };
}

export function removeHistoryItem(store: HistoryStore, id: string): HistoryStore {
  return { ...store, history: store.history.filter((item) => item.id !== id) };
}

function copyName(name: string): string {
  const suffix = " 복사본";
  return `${name.slice(0, 120 - suffix.length)}${suffix}`;
}

function normalizeName(name: string): string {
  return name.replace(/[\r\n]+/g, " ").trim().slice(0, 120);
}

function clonePersistedRequest(request: PersistedHistoryRequest): PersistedHistoryRequest {
  return {
    ...request,
    headers: request.headers.map((header) => ({ ...header })),
    cookies: normalizeCookies(request.cookies),
    params: request.params.map((param) => ({ ...param })),
    auth: request.auth ? { ...request.auth } : null,
  };
}
