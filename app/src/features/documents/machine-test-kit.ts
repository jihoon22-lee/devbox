import type { Claimed, DiskRev, DocPolicy } from "../../rpc/gen/rpc";
import { closedDoc, type Doc, type DocEvent, type Effect, reduce } from "./machine";

/** Shared reducer fixtures for the machine tests. */

export const key = { root: "vault", path: "a.md" };
export const note: DocPolicy = { kind: "note" };
export const code: DocPolicy = { kind: "code" };
export const diskRev = (hash: string, mtime = "1"): DiskRev => ({ hash, size: hash.length, mtimeNs: mtime, ino: "7" });
export const d1 = diskRev("d1");

export function claimed(over: Partial<Claimed> = {}): Claimed {
  return {
    key,
    leaseId: "L1",
    rev: d1,
    text: "a",
    eol: "lf",
    bom: false,
    readOnly: null,
    journal: null,
    ...over,
  };
}
export function run(doc: Doc, ...events: DocEvent[]) {
  const effects: Effect[] = [];
  let current = doc;
  for (const event of events) {
    const step = reduce(current, event);
    current = step.doc;
    effects.push(...step.effects);
  }
  return { doc: current, effects };
}
export function opened(
  over: Partial<Claimed> = {},
  policy: DocPolicy = note,
  offline?: Parameters<typeof withOffline>[0],
) {
  return run(closedDoc("1", key, policy), { type: "open", key, policy }, withOffline(offline, over));
}
export function withOffline(offline: (DocEvent & { type: "claimed" })["offline"], over: Partial<Claimed>): DocEvent {
  return { type: "claimed", claimed: claimed(over), offline };
}
export function clean(text = "a", policy: DocPolicy = note): Doc {
  return opened({ text }, policy).doc;
}
export const types = (effects: Effect[]) => effects.map((e) => e.type);
export const drops = (effects: Effect[]) => effects.filter((e) => e.type === "journalDrop");
