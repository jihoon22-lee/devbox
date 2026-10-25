import type { RewriteRequest, TextEdit } from "../../rpc/gen/rpc";
import type { Doc } from "../documents/machine";
import { keyId } from "../documents/offlineJournal";
import { type DocumentsClient, useDocuments } from "../documents/store";

/** The daemon waits 30 s; answer well before it gives up. */
const REPLY_WITHIN_MS = 20_000;

/**
 * Applies UTF-16 `edits` (JS string offsets are UTF-16 already) from the end so earlier offsets
 * stay valid. Out-of-range or overlapping edits refuse the whole request.
 */
export function applyEdits(text: string, edits: TextEdit[]): string | null {
  const sorted = [...edits].sort((a, b) => b.fromUtf16 - a.fromUtf16 || b.toUtf16 - a.toUtf16);
  let limit = text.length;
  let out = text;
  for (const edit of sorted) {
    const { fromUtf16: from, toUtf16: to } = edit;
    if (!Number.isInteger(from) || !Number.isInteger(to) || from < 0 || to < from || to > limit) return null;
    out = out.slice(0, from) + edit.text + out.slice(to);
    limit = from;
  }
  return out;
}

/** Resolves true once a save covering `seq` landed; false on conflict, lease loss, close or timeout. */
function savedThrough(id: string, seq: number, ms: number): Promise<boolean> {
  return new Promise((resolve) => {
    let off = () => {};
    const done = (ok: boolean) => {
      clearTimeout(timer);
      off();
      resolve(ok);
    };
    const check = (d: Doc | undefined) => {
      if (!d || d.state === "Conflict" || d.state === "ReadOnly" || d.state === "Recovering") done(false);
      else if (d.savedSeq >= seq && d.state !== "Saving") done(true);
    };
    const timer = setTimeout(() => done(false), ms);
    off = useDocuments.subscribe((s) => check(s.docs[id]));
    check(useDocuments.getState().docs[id]);
  });
}

/** Match the note save's byte encoding before changing the holder's buffer. */
export function fitsNoteRewrite(doc: Pick<Doc, "policy" | "eol" | "bom">, text: string): boolean {
  if (doc.policy.kind !== "note" || text.includes("\0")) return false;
  const bytes = new TextEncoder();
  const disk = doc.eol === "crlf" ? text.replaceAll("\r\n", "\n").replaceAll("\n", "\r\n") : text;
  return (
    bytes.encode(disk).length + (doc.bom ? 3 : 0) <= 2 * 1024 * 1024 &&
    bytes.encode(JSON.stringify(text)).length - 2 <= 12 * 1024 * 1024
  );
}

/**
 * One `notes.rewrite_request` (rename links, append, daily summary): only the window holding the
 * lease answers, and only when its buffer is still the journaled one. The edits go into the buffer
 * and the editor as one undoable change, the note is saved, and the reply says whether it landed.
 */
export async function handleRewrite(client: DocumentsClient, request: RewriteRequest): Promise<void> {
  const store = useDocuments.getState();
  const doc = Object.values(store.docs).find(
    (d) => d.leaseId === request.leaseId && keyId(d.key) === keyId(request.key),
  );
  // Every window receives the topic; a reply from a non-holder would overwrite the holder's answer.
  if (!doc) return;
  const reply = (applied: boolean) =>
    client.call("notes.rewrite_reply", { opId: request.opId, key: request.key, applied }).then(
      () => {},
      () => {},
    );
  const current = (doc.state === "Dirty" || doc.state === "Saving") && doc.editSeq === request.journalSeq;
  const next = current ? applyEdits(doc.text, request.edits) : null;
  if (next === null || !fitsNoteRewrite(doc, next)) return reply(false);
  store.replace(doc.key, next);
  const seq = useDocuments.getState().docs[doc.id]?.editSeq ?? Number.POSITIVE_INFINITY;
  store.save(doc.key);
  return reply(await savedThrough(doc.id, seq, REPLY_WITHIN_MS));
}

/** Subscribes this window to rewrite requests; returns the unsubscribe function. */
export function listenRewrites(client: DocumentsClient): () => void {
  return client.subscribe(
    "notes.rewrite_request",
    (_rev, request) => void handleRewrite(client, request),
    () => {},
  );
}
