import type { HistoryItem } from "../types";
import {
  historyVisibleMetadata as projectVisibleMetadata,
  MAX_HISTORY_DISPLAY_CHARS,
  MAX_HISTORY_METHOD_CHARS,
  projectHistoryItem,
  type HistoryVisibleMetadata,
} from "./persistence";

export { MAX_HISTORY_DISPLAY_CHARS, MAX_HISTORY_METHOD_CHARS };
export type { HistoryVisibleMetadata };

export const MAX_HISTORY_QUERY_CHARS = 128;

export type HistoryStatusFilter = "all" | "success" | "error";

export interface HistoryFilter {
  query: string;
  method: string;
  status: HistoryStatusFilter;
}

/** Metadata-only projection used by labels, method options, and search. */
export function historyVisibleMetadata(item: HistoryItem): HistoryVisibleMetadata {
  return projectVisibleMetadata(item);
}

/** Display-only label; localStorage edits must not turn a History row into a raw URL echo. */
export function historyDisplayLabel(item: HistoryItem): string {
  const metadata = historyVisibleMetadata(item);
  return metadata.name || metadata.url || "(no url)";
}

export function historyMethod(item: HistoryItem): string {
  return historyVisibleMetadata(item).method;
}

/**
 * History search deliberately indexes only safe display metadata. Request headers, cookies,
 * auth, and body are excluded even when their persisted values are already masked.
 */
export function filterHistory(items: readonly HistoryItem[], filter: HistoryFilter): HistoryItem[] {
  const query = filter.query.trim().slice(0, MAX_HISTORY_QUERY_CHARS).toLocaleLowerCase();
  const method = filter.method.trim().toUpperCase();
  return items.filter((item) => {
    const metadata = historyVisibleMetadata(item);
    const status = metadata.status === undefined
      ? "error"
      : metadata.status >= 200 && metadata.status < 400
        ? "success"
        : "error";
    if (method && metadata.method !== method) return false;
    if (filter.status !== "all" && status !== filter.status) return false;
    if (!query) return true;
    const haystack = [
      metadata.name,
      metadata.method,
      metadata.url,
      metadata.status === undefined ? "" : String(metadata.status),
    ].join(" ").toLocaleLowerCase();
    return haystack.includes(query);
  });
}

// Keep the replay projection available to callers that construct a manually
// edited History item instead of loading it through parseHistoryStore.
export function projectHistoryForReplay(item: HistoryItem, index = 0): HistoryItem {
  return projectHistoryItem(item, index);
}
