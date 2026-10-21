import type { ErrorCode } from "../../rpc/client";
import type { Claimed, DiskRev, DocKey, DocPolicy, Eol, Expect, LeaseLoss, ReadOnly } from "../../rpc/gen/rpc";
import { entrySeq, type OfflineEntry } from "./offlineJournal";

export type DocState =
  | "Closed"
  | "Loading"
  | "Clean"
  | "Dirty"
  | "Saving"
  | "Recovering"
  | "Conflict"
  | "Orphaned"
  | "ReadOnly";

export interface Doc {
  id: string;
  key: DocKey;
  policy: DocPolicy;
  state: DocState;
  leaseId?: string;
  diskRev: DiskRev | null;
  editSeq: number;
  savedSeq: number;
  journalSeq: number;
  inflight?: { seq: number; baseRev: DiskRev | null; expect: Expect };
  pendingExternal: (DiskRev | null)[];
  conflictRev?: DiskRev | null;
  disk?: { rev: DiskRev | null; text: string | null };
  /** A draft to offer; `other` is a second, different draft (this device vs the daemon) offered too. */
  recovery?: Draft & { other?: Draft };
  text: string;
  eol: Eol;
  bom: boolean;
  readOnly?: ReadOnly;
  retry: boolean;
  error?: ErrorCode;
  closing: boolean;
  closeRequested: boolean;
}

/** A journal draft; `text` is missing until a deferred daemon journal has been read. */
export interface Draft {
  seq: number;
  baseRev: DiskRev | null;
  text?: string;
}

export type DocEvent =
  | { type: "open"; key: DocKey; policy: DocPolicy }
  | { type: "claimed"; claimed: Claimed; offline?: OfflineEntry }
  | { type: "claimFailed"; code: ErrorCode }
  | { type: "edit"; text: string }
  | { type: "journalTick" }
  | { type: "journalOk"; seq: number }
  | { type: "journalFailed"; seq: number; code: ErrorCode; reason?: LeaseLoss }
  | { type: "journalLoaded"; seq: number; baseRev: DiskRev | null; text: string }
  | { type: "saveTick" }
  | { type: "save" }
  | { type: "saveOk"; rev: DiskRev }
  | { type: "saveFailed"; code: ErrorCode; rev?: DiskRev | null; reason?: LeaseLoss }
  | { type: "reconnected" }
  | { type: "resync" }
  | { type: "external"; rev: DiskRev | null }
  | { type: "diskLoaded"; rev: DiskRev | null; text: string | null }
  | { type: "restore"; which?: "other" }
  | { type: "discard" }
  | { type: "resolve"; choice: "mine" | "disk" | "merged"; text?: string }
  | { type: "leaseLost"; reason: LeaseLoss }
  | { type: "take" }
  | { type: "moved"; to: DocKey }
  /** `discardAll` is the explicit "drop both drafts" choice of a Recovering document with unsaved edits. */
  | { type: "close"; choice?: CloseChoice };

export type Effect =
  | { type: "claim"; key: DocKey; policy: DocPolicy; take: boolean }
  | { type: "release"; leaseId: string }
  | { type: "save"; leaseId: string; seq: number; baseRev: DiskRev | null; expect: Expect; text: string }
  | { type: "journal"; leaseId: string; seq: number; baseRev: DiskRev | null; text: string }
  | { type: "journalRead"; leaseId: string }
  /** Drops both the daemon journal and this device's offline entry; there is no one-sided drop. */
  | { type: "journalDrop"; key: DocKey; uptoSeq: number; leaseId?: string }
  | { type: "offlineJournal"; key: DocKey; seq: number; baseRev: DiskRev | null; text: string }
  | { type: "scheduleJournal"; ms: 300 }
  | { type: "scheduleSave"; ms: 1000 }
  | { type: "readDisk"; key: DocKey; policy: DocPolicy }
  | { type: "applyText"; text: string; undoable: true };

export type CloseChoice = "save" | "discard" | "keep" | "discardAll";

export interface Step {
  doc: Doc;
  effects: Effect[];
}

export function closedDoc(id: string, key: DocKey, policy: DocPolicy): Doc {
  return {
    id,
    key,
    policy,
    state: "Closed",
    diskRev: null,
    editSeq: 0,
    savedSeq: 0,
    journalSeq: 0,
    pendingExternal: [],
    text: "",
    eol: "lf",
    bom: false,
    retry: false,
    closing: false,
    closeRequested: false,
  };
}

/** Errors after which the same save is retried once the connection (and lease) is back. */
const TRANSIENT: ReadonlySet<ErrorCode> = new Set<ErrorCode>([
  "connection_lost",
  "internal",
  "cancelled",
  "daemon_restarting",
]);
/** States holding edits the disk does not have yet. */
const EDITING: ReadonlySet<DocState> = new Set<DocState>(["Dirty", "Saving", "Conflict", "Orphaned"]);

/** Content identity, the same rule as the daemon's CAS (`DiskRev::same_content`): a touch is not a change. */
export const sameRev = (a: DiskRev | null | undefined, b: DiskRev | null | undefined) =>
  a === b || (!!a && !!b && a.hash === b.hash && a.size === b.size);

const out = (doc: Doc, effects: Effect[] = []): Step => ({ doc, effects });
const claim = (d: Doc, take = false): Effect => ({ type: "claim", key: d.key, policy: d.policy, take });
const readDisk = (d: Doc): Effect => ({ type: "readDisk", key: d.key, policy: d.policy });
/** The daemon refuses a drop while another window holds the lease, so it carries ours when we have one. */
const drop = (d: Doc, uptoSeq: number): Effect =>
  d.leaseId
    ? { type: "journalDrop", key: d.key, uptoSeq, leaseId: d.leaseId }
    : { type: "journalDrop", key: d.key, uptoSeq };
const apply = (text: string): Effect => ({ type: "applyText", text, undoable: true });
const unsaved = (d: Doc) => d.editSeq > d.savedSeq;
const autosave = (d: Doc): Effect[] => (d.policy.kind === "note" ? [{ type: "scheduleSave", ms: 1000 }] : []);
const release = (d: Doc): Effect[] => (d.leaseId ? [{ type: "release", leaseId: d.leaseId }] : []);
const closed = (d: Doc): Doc => ({ ...d, state: "Closed", leaseId: undefined, closing: false, closeRequested: false });

function startSave(d: Doc): Step {
  if (!d.leaseId) return out({ ...d, retry: true });
  const expect = d.diskRev ? "present" : "absent";
  const inflight = { seq: d.editSeq, baseRev: d.diskRev, expect } as const;
  return out({ ...d, state: "Saving", inflight, retry: false, error: undefined }, [
    { type: "save", leaseId: d.leaseId, seq: d.editSeq, baseRev: d.diskRev, expect, text: d.text },
  ]);
}

/** Keeps the newest edit on this device; `journalSeq` covers every seq handed to either journal. */
function offline(d: Doc, baseRev: DiskRev | null = d.diskRev): Step {
  return out({ ...d, journalSeq: Math.max(d.journalSeq, d.editSeq) }, [
    { type: "offlineJournal", key: d.key, seq: d.editSeq, baseRev, text: d.text },
  ]);
}

function loseLease(d: Doc, reason: LeaseLoss): Step {
  const base = { ...d, leaseId: undefined, inflight: undefined };
  if (reason === "taken") return out({ ...base, state: "ReadOnly", readOnly: { reason: "leased_elsewhere" } });
  if (d.state === "Closed" || d.state === "ReadOnly") return out(base);
  return out({ ...base, state: d.state === "Saving" ? "Dirty" : d.state, retry: d.retry || d.state === "Saving" }, [
    claim(d),
  ]);
}

function edit(d: Doc, text: string): Step {
  if (!EDITING.has(d.state) && d.state !== "Clean") return out(d);
  const state = d.state === "Clean" ? "Dirty" : d.state;
  const next = { ...d, text, state, editSeq: d.editSeq + 1, closeRequested: false };
  const effects: Effect[] = [{ type: "scheduleJournal", ms: 300 }];
  return out(next, state === "Dirty" ? [...effects, ...autosave(d)] : effects);
}

function journalTick(d: Doc): Step {
  if (!EDITING.has(d.state) || d.editSeq <= d.journalSeq) return out(d);
  if (!d.leaseId) return offline(d);
  return out({ ...d, journalSeq: d.editSeq }, [
    { type: "journal", leaseId: d.leaseId, seq: d.editSeq, baseRev: d.diskRev, text: d.text },
  ]);
}

/** Loading → Clean, Recovering or ReadOnly. Sequences continue above every journal already written. */
function loaded(d: Doc, c: Claimed, entry: OfflineEntry | undefined): Step {
  const format = { diskRev: c.rev, eol: c.eol, bom: c.bom, error: undefined };
  if (c.readOnly || !c.leaseId)
    return out({ ...d, ...format, state: "ReadOnly", readOnly: c.readOnly ?? undefined, text: c.text ?? "" });
  const disk = c.text ?? "";
  const tomb = entry?.kind === "tombstone" ? entry.dropUpto : 0;
  const draft = entry?.kind === "draft" && entry.seq > tomb ? entry : undefined;
  const server = c.journal && c.journal.seq > tomb ? c.journal : undefined;
  const seen = Math.max(c.journal?.seq ?? 0, entry ? entrySeq(entry) : 0);
  // Offer every draft that differs from the disk: the newest first, a different second one too,
  // so discarding never removes a draft the user did not see.
  const drafts: Draft[] = [];
  if (draft) drafts.push({ seq: draft.seq, baseRev: draft.baseRev, text: draft.text });
  if (server)
    drafts.push({ seq: server.seq, baseRev: server.baseRev, text: server.deferred ? undefined : (server.text ?? "") });
  const [first, second] = drafts.filter((x) => x.text !== disk).sort((a, b) => b.seq - a.seq);
  const candidate = first && (second && second.text !== first.text ? { ...first, other: second } : first);
  const base: Doc = {
    ...d,
    ...format,
    state: "Clean",
    leaseId: c.leaseId,
    text: disk,
    editSeq: seen,
    savedSeq: seen,
    journalSeq: seen,
    readOnly: undefined,
  };
  if (candidate) {
    const effects: Effect[] = tomb ? [drop(base, tomb)] : [];
    if (candidate.text === undefined || ("other" in candidate && candidate.other?.text === undefined))
      effects.push({ type: "journalRead", leaseId: c.leaseId });
    return out({ ...base, state: "Recovering", recovery: candidate }, effects);
  }
  // Nothing worth recovering (same text, or a drop this device already chose): clear both journals.
  return out(base, seen > 0 ? [drop(base, seen)] : []);
}

/**
 * Sequences continue above a journal another window (or an earlier lease) wrote, so the next
 * journal is never `stale_journal`. Unsaved edits stay unsaved.
 */
function above(d: Doc, seq: number): Pick<Doc, "editSeq" | "savedSeq" | "journalSeq"> {
  const journalSeq = Math.max(d.journalSeq, seq);
  if (seq < d.editSeq) return { editSeq: d.editSeq, savedSeq: d.savedSeq, journalSeq };
  if (!unsaved(d)) return { editSeq: seq, savedSeq: seq, journalSeq };
  return { editSeq: seq + 1, savedSeq: Math.max(d.savedSeq, seq), journalSeq };
}

/**
 * A daemon journal newer than anything this window wrote is another window's draft (§8.4
 * "Loading with a journal → Recovering"). Offer it instead of silently writing over it; unsaved
 * edits here stay in the buffer until the user picks one ([restore] theirs, [discard] keeps ours).
 */
function foreignJournal(next: Doc, d: Doc, c: Claimed): Step | undefined {
  const j = c.journal;
  if (!j || j.seq <= d.journalSeq || !c.leaseId) return undefined;
  if (d.state === "Saving" || d.state === "Recovering") return undefined;
  const mine = unsaved(d) || d.state === "Conflict";
  const shown = mine ? d.text : (c.text ?? "");
  if (!j.deferred && j.text === shown) return undefined;
  const recovery = { seq: j.seq, baseRev: j.baseRev, text: j.deferred ? undefined : (j.text ?? "") };
  const base: Doc = { ...next, state: "Recovering", recovery, retry: false, closeRequested: false };
  const doc = mine ? base : { ...base, diskRev: c.rev, text: shown };
  const effects: Effect[] = shown === d.text ? [] : [apply(shown)];
  if (j.deferred) effects.push({ type: "journalRead", leaseId: c.leaseId });
  if (!mine) return out(doc, effects);
  // The daemon journal is theirs now: keep our buffer on this device until the user picks.
  const kept = offline(doc);
  return out(kept.doc, [...effects, ...kept.effects]);
}

/** A claim after reconnect, expiry or [take]: the buffer and its base rev stay. */
function reclaimed(d: Doc, c: Claimed): Step {
  if (c.readOnly || !c.leaseId) {
    const keep = unsaved(d) || d.state === "Recovering";
    const text = keep ? d.text : (c.text ?? d.text);
    const next: Doc = { ...d, state: "ReadOnly", leaseId: undefined, readOnly: c.readOnly ?? undefined, text };
    return out(keep ? next : { ...next, diskRev: c.rev }, text === d.text ? [] : [apply(text)]);
  }
  let next: Doc = { ...d, leaseId: c.leaseId, readOnly: undefined, error: undefined, ...above(d, c.journal?.seq ?? 0) };
  const offered = foreignJournal(next, d, c);
  if (offered) return offered;
  if (d.state === "ReadOnly") next = { ...next, state: unsaved(d) ? "Dirty" : "Clean" };
  if (next.state === "Recovering" && next.recovery?.text === undefined)
    return out(next, [{ type: "journalRead", leaseId: c.leaseId }]);
  if (next.state === "Clean" && !sameRev(c.rev, d.diskRev)) {
    if (!c.rev) return out({ ...next, state: "Orphaned", diskRev: null });
    const text = c.text ?? "";
    return out({ ...next, diskRev: c.rev, text }, text === d.text ? [] : [apply(text)]);
  }
  if ((next.state === "Dirty" || next.state === "Orphaned") && next.retry) return startSave(next);
  if (next.editSeq > next.journalSeq) return out(next, [{ type: "scheduleJournal", ms: 300 }]);
  return out(next);
}

function external(d: Doc, rev: DiskRev | null): Step {
  switch (d.state) {
    case "Saving":
      return out({ ...d, pendingExternal: [...d.pendingExternal, rev] });
    case "Clean":
    case "Dirty":
      if (!rev) return out({ ...d, state: "Orphaned", diskRev: null });
      if (sameRev(rev, d.diskRev)) return out(d);
      if (d.state === "Clean") return out(d, [readDisk(d)]);
      return out({ ...d, state: "Conflict", conflictRev: rev, disk: undefined }, [readDisk(d)]);
    case "Orphaned":
      if (!rev) return out(d);
      if (!unsaved(d)) return out({ ...d, state: "Clean" }, [readDisk(d)]);
      return out({ ...d, state: "Conflict", conflictRev: rev, disk: undefined }, [readDisk(d)]);
    case "Conflict":
      return out({ ...d, conflictRev: rev, disk: undefined }, [readDisk(d)]);
    case "Recovering":
      return sameRev(rev, d.diskRev) ? out(d) : out(d, [readDisk(d)]);
    case "ReadOnly":
      return unsaved(d) || sameRev(rev, d.diskRev) ? out(d) : out(d, [readDisk(d)]);
    default:
      return out(d);
  }
}

function diskLoaded(d: Doc, rev: DiskRev | null, text: string | null): Step {
  const changed = !sameRev(rev, d.diskRev);
  switch (d.state) {
    case "Clean":
    case "Recovering":
    case "ReadOnly": {
      if (!changed) return out(d);
      // Unsaved edits are never replaced: keep the old base so the next save compares (Conflict).
      if (d.state === "Recovering" && unsaved(d)) return out({ ...d, disk: { rev, text } });
      if (d.state === "ReadOnly" && unsaved(d)) return out(d);
      if (!rev && d.state === "Clean") return out({ ...d, state: "Orphaned", diskRev: null });
      const next = text ?? "";
      return out({ ...d, diskRev: rev, text: next }, next === d.text ? [] : [apply(next)]);
    }
    case "Dirty":
      if (!changed) return out(d);
      if (!rev) return out({ ...d, state: "Orphaned", diskRev: null });
      return out({ ...d, state: "Conflict", conflictRev: rev, disk: { rev, text } });
    case "Orphaned":
      if (!rev) return out(d);
      if (!unsaved(d)) return diskLoaded({ ...d, state: "Clean" }, rev, text);
      return out({ ...d, state: "Conflict", conflictRev: rev, disk: { rev, text } });
    case "Conflict":
      // The disk already holds our text (a retried save whose reply was lost): nothing to compare.
      if (rev && text === d.text) {
        const done: Doc = { ...d, state: "Clean", diskRev: rev, savedSeq: d.editSeq, retry: false };
        return out({ ...done, conflictRev: undefined, disk: undefined }, [drop(d, d.journalSeq)]);
      }
      return out({ ...d, conflictRev: rev, disk: { rev, text } });
    default:
      return out(d);
  }
}

function saveOk(d: Doc, rev: DiskRev): Step {
  if (d.state !== "Saving" || !d.inflight) return out(d);
  const seq = d.inflight.seq;
  const done = d.editSeq === seq;
  // Our own write echoes back with the new rev; a touch of the old content is no change either.
  const foreign = d.pendingExternal.filter((r) => !sameRev(r, rev) && !sameRev(r, d.inflight?.baseRev));
  const next: Doc = {
    ...d,
    diskRev: rev,
    savedSeq: seq,
    inflight: undefined,
    pendingExternal: [],
    retry: false,
    error: undefined,
    state: done ? "Clean" : "Dirty",
  };
  if (done && d.closing) return out(closed(next), [drop(d, seq), ...release(d)]);
  const effects = done ? [drop(d, seq)] : autosave(d);
  return out(next, foreign.length ? [...effects, readDisk(d)] : effects);
}

function saveFailed(d: Doc, e: DocEvent & { type: "saveFailed" }): Step {
  if (d.state !== "Saving" || !d.inflight) return out(d);
  const base: Doc = { ...d, state: "Dirty", inflight: undefined, pendingExternal: [], closing: false };
  // Classified by the generated error code only, never by message text (CV-F02).
  if (e.code === "documents.conflict")
    return out({ ...base, state: "Conflict", conflictRev: e.rev ?? null, disk: undefined }, [readDisk(d)]);
  if (e.code === "documents.lease_lost") return loseLease({ ...base, retry: true }, e.reason ?? "expired");
  if (TRANSIENT.has(e.code)) return offline({ ...base, retry: true, error: e.code }, d.inflight.baseRev);
  return out({ ...base, error: e.code });
}

function resolve(d: Doc, e: DocEvent & { type: "resolve" }): Step {
  if (d.state !== "Conflict") return out(d);
  const rev = d.conflictRev ?? null;
  if (e.choice === "mine") return startSave({ ...d, diskRev: rev, disk: undefined, conflictRev: undefined });
  if (e.choice === "merged") {
    const text = e.text ?? d.text;
    const next: Doc = { ...d, state: "Dirty", text, diskRev: rev, editSeq: d.editSeq + 1, disk: undefined };
    return out({ ...next, conflictRev: undefined }, [
      apply(text),
      { type: "scheduleJournal", ms: 300 },
      ...autosave(d),
    ]);
  }
  if (d.disk?.text == null) return out(d);
  const text = d.disk.text;
  const next: Doc = { ...d, state: "Clean", text, diskRev: d.disk.rev, savedSeq: d.editSeq, retry: false };
  // The rejected draft must not come back as a recovery candidate (AR-F13).
  return out({ ...next, disk: undefined, conflictRev: undefined }, [apply(text), drop(d, d.journalSeq)]);
}

function close(d: Doc, asked: CloseChoice | undefined): Step {
  if (d.state === "Closed") return out(d);
  // Recovering over another window's draft with our own unsaved buffer: two drafts are at stake,
  // so only the explicit "discard both" closes; everything else keeps the window open and asks.
  if (d.state === "Recovering" && unsaved(d)) {
    if (asked !== "discardAll") return out({ ...d, closeRequested: true });
    return out(closed(d), [drop(d, Math.max(d.journalSeq, d.editSeq)), ...release(d)]);
  }
  const choice = asked === "discardAll" ? "discard" : asked;
  const orphanedClean = d.state === "Orphaned" && !unsaved(d);
  const pending = (EDITING.has(d.state) && !orphanedClean) || (d.state === "ReadOnly" && unsaved(d));
  if (d.state === "Recovering" && choice === "discard") return out(closed(d), [drop(d, d.journalSeq), ...release(d)]);
  if (!pending) return out(closed(d), release(d));
  if (!choice) return out({ ...d, closeRequested: true });
  // Read-only: the journals belong to the window holding the lease, so this buffer can be neither
  // kept nor saved here. Only an explicit [discard] forgets it; otherwise the window stays open.
  if (d.state === "ReadOnly") return choice === "discard" ? out(closed(d)) : out({ ...d, closeRequested: true });
  // Without a lease (waiting to reclaim) the drop covers only what this window journaled.
  const upto = d.leaseId ? Math.max(d.journalSeq, d.editSeq) : d.journalSeq;
  if (choice === "discard") return out(closed(d), [drop(d, upto), ...release(d)]);
  if (choice === "keep") {
    const draft = d.editSeq > d.journalSeq ? offline(d).effects : [];
    return out(closed(d), [...draft, ...release(d)]);
  }
  if (d.state === "Saving") return out({ ...d, closing: true, closeRequested: false });
  if (d.state === "Dirty" || d.state === "Orphaned") {
    const step = startSave({ ...d, closing: true, closeRequested: false });
    return step.doc.state === "Saving" ? step : out(d);
  }
  return out(d);
}

/** The §8.4 document state machine. Pure: every side effect is returned for the effect runner. */
export function reduce(d: Doc, e: DocEvent): Step {
  switch (e.type) {
    case "open":
      return d.state === "Closed"
        ? out({ ...closedDoc(d.id, e.key, e.policy), state: "Loading" }, [
            claim({ ...d, key: e.key, policy: e.policy }),
          ])
        : out(d);
    case "claimed":
      if (d.state === "Loading") return loaded(d, e.claimed, e.offline);
      return d.state === "Closed" ? out(d) : reclaimed(d, e.claimed);
    case "claimFailed":
      return d.state === "Closed" ? out(d) : out({ ...d, error: e.code });
    case "edit":
      return edit(d, e.text);
    case "journalTick":
      return journalTick(d);
    case "journalOk":
      return out(d);
    case "journalFailed":
      if (e.code === "documents.lease_lost") return loseLease(d, e.reason ?? "expired");
      if (!EDITING.has(d.state)) return out(d);
      if (e.code === "documents.stale_journal") {
        // Another lease wrote a higher seq: keep the edit on this device and reseed from a claim.
        const kept = offline(d);
        return out(kept.doc, [...kept.effects, claim(d)]);
      }
      return TRANSIENT.has(e.code) ? offline(d) : out(d);
    case "journalLoaded":
      if (d.state !== "Recovering" || !d.recovery) return out(d);
      if (d.recovery.seq === e.seq) return out({ ...d, recovery: { ...d.recovery, text: e.text } });
      if (d.recovery.other?.seq !== e.seq) return out(d);
      return out({ ...d, recovery: { ...d.recovery, other: { ...d.recovery.other, text: e.text } } });
    case "saveTick":
      return d.state === "Dirty" && d.policy.kind === "note" ? startSave(d) : out(d);
    case "save":
      return d.state === "Dirty" || d.state === "Orphaned" ? startSave(d) : out(d);
    case "saveOk":
      return saveOk(d, e.rev);
    case "saveFailed":
      return saveFailed(d, e);
    case "reconnected":
      if (d.state === "Closed" || d.state === "ReadOnly" || d.state === "Saving") return out(d);
      return out({ ...d, leaseId: undefined }, [claim(d)]);
    case "resync":
      return d.state === "Closed" || d.state === "Loading" || d.state === "Saving" ? out(d) : out(d, [readDisk(d)]);
    case "external":
      return external(d, e.rev);
    case "diskLoaded":
      return diskLoaded(d, e.rev, e.text);
    case "restore": {
      const text = e.which === "other" ? d.recovery?.other?.text : d.recovery?.text;
      if (d.state !== "Recovering" || text === undefined) return out(d);
      const next: Doc = { ...d, state: "Dirty", text, recovery: undefined, editSeq: d.editSeq + 1 };
      return out(next, [apply(text), { type: "scheduleJournal", ms: 300 }, ...autosave(d)]);
    }
    case "discard":
      if (d.state !== "Recovering") return out(d);
      if (unsaved(d)) {
        // Kept our unsaved buffer over another window's draft: drop only theirs, journal ours above it.
        // Our offline copy (seq editSeq) stays until a save covers it.
        const theirs = d.recovery?.seq ?? d.journalSeq;
        const next: Doc = { ...d, state: "Dirty", recovery: undefined, editSeq: d.editSeq + 1 };
        return out(next, [drop(d, theirs), { type: "scheduleJournal", ms: 300 }, ...autosave(d)]);
      }
      return out({ ...d, state: "Clean", recovery: undefined }, [drop(d, d.journalSeq)]);
    case "resolve":
      return resolve(d, e);
    case "leaseLost":
      return loseLease(d, e.reason);
    case "take":
      return d.state === "ReadOnly" && d.readOnly?.reason === "leased_elsewhere" ? out(d, [claim(d, true)]) : out(d);
    case "moved":
      return d.state === "Closed" ? out(d) : out({ ...d, key: e.to });
    case "close":
      return close(d, e.choice);
  }
}
