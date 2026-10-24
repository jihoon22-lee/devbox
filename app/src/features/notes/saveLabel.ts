import type { ErrorCode } from "../../rpc/client";
import type { Doc } from "../documents/machine";

/** Errors after which the store keeps the edit on this device and retries once connected again. */
const OFFLINE: ReadonlySet<ErrorCode> = new Set<ErrorCode>([
  "connection_lost",
  "internal",
  "cancelled",
  "daemon_restarting",
]);
const pad = (n: number) => String(n).padStart(2, "0");

/** The saved time is the file's own modification time (our last save, or the newer disk we loaded). */
function savedAt(doc: Doc): string {
  const ns = doc.diskRev?.mtimeNs;
  if (!ns || !/^\d+$/.test(ns)) return "저장됨";
  const at = new Date(Number(BigInt(ns) / 1_000_000n));
  return `저장됨 ${pad(at.getHours())}:${pad(at.getMinutes())}`;
}

/**
 * The note header's save state. Four words only: saved, saving, kept on this device while
 * disconnected, and conflict. Recovering, read-only and loading show their own panels instead.
 */
export function saveLabel(doc: Doc, online: boolean): string | null {
  const unsaved = doc.editSeq > doc.savedSeq;
  switch (doc.state) {
    case "Clean":
      return savedAt(doc);
    case "Conflict":
      return "충돌 · 비교 필요";
    case "Dirty":
    case "Saving":
    case "Orphaned":
      if (!unsaved) return doc.state === "Orphaned" ? null : "저장 중";
      if (!online || doc.retry || (doc.error && OFFLINE.has(doc.error))) return "연결 끊김 · 이 기기에 임시 저장";
      return "저장 중";
    default:
      return null;
  }
}
