import { create } from "zustand";
import type { ConflictPreview, SourceTarget } from "../../../rpc/gen/rpc";
export function mergeKey(target: SourceTarget, path: string) {
  return JSON.stringify([target.projectId, target.worktreeId ?? null, path]);
}
export type MergeDraft = {
  request: number;
  loading: boolean;
  preview?: ConflictPreview;
  resolution?: string | null;
  error?: string;
};
export const useMergeDrafts = create<{
  entries: Record<string, MergeDraft>;
  begin: (key: string) => number;
  receive: (key: string, request: number, preview: ConflictPreview) => void;
  fail: (key: string, request: number, error: string) => void;
  edit: (key: string, resolution: string | null) => void;
}>((set, get) => ({
  entries: {},
  begin(key) {
    const old = get().entries[key];
    const request = (old?.request ?? 0) + 1;
    set({ entries: { ...get().entries, [key]: { ...old, request, loading: true } } });
    return request;
  },
  receive(key, request, preview) {
    const old = get().entries[key];
    if (old?.request !== request) return;
    set({
      entries: {
        ...get().entries,
        [key]: {
          ...old,
          loading: false,
          error: undefined,
          preview,
          resolution: old.resolution !== undefined ? old.resolution : (preview.merged ?? ""),
        },
      },
    });
  },
  fail(key, request, error) {
    const old = get().entries[key];
    if (old?.request !== request) return;
    set({ entries: { ...get().entries, [key]: { ...old, loading: false, error } } });
  },
  edit(key, resolution) {
    const old = get().entries[key] ?? { request: 0, loading: false };
    set({ entries: { ...get().entries, [key]: { ...old, resolution } } });
  },
}));
export function choose(preview: ConflictPreview, side: "ours" | "theirs" | "both") {
  if (side !== "both") return preview[side];
  const ours = preview.ours ?? "",
    theirs = preview.theirs ?? "";
  return ours + (ours && theirs && !ours.endsWith("\n") ? "\n" : "") + theirs;
}
