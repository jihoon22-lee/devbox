import { type ClientState, RpcError } from "../../rpc/client";
import type { DiskRev, DocKey, Methods, Topics } from "../../rpc/gen/rpc";
import { keyId } from "./offlineJournal";
import type { DocumentsClient } from "./store";

type Journal = { seq: number; baseRev: DiskRev | null; text: string };
/** The union of every documents request field this fake reads. */
type Params = {
  key: DocKey;
  windowId: string;
  leaseId: string;
  leaseIds: string[];
  baseRev: DiskRev | null;
  expect: "present" | "absent";
  text: string;
  seq: number;
  uptoSeq: number;
};
type Call = { method: string; params: Record<string, unknown> };
type Sub = { topic: string; onEvent: (rev: string, payload: unknown) => void; onResync: () => void };

let hashes = 0;
export function revOf(text: string): DiskRev {
  hashes += 1;
  return {
    hash: `h${text.length}-${[...text].reduce((a, c) => a + c.charCodeAt(0), 0)}`,
    size: text.length,
    mtimeNs: `${hashes}`,
    ino: "1",
  };
}

/** A scripted in-memory documents daemon behind the RpcClient call/subscribe/onState surface. */
export class FakeDocumentsDaemon implements DocumentsClient {
  state: ClientState = "ready";
  files = new Map<string, { rev: DiskRev; text: string }>();
  journals = new Map<string, Journal>();
  leases = new Map<string, { key: DocKey; window: string; floor: number }>();
  calls: Call[] = [];
  /** Methods that fail with this code while set (for example `documents.journal_drop`). */
  failing = new Map<string, string>();
  private subs = new Set<Sub>();
  private states = new Set<(s: ClientState) => void>();
  private nextLease = 1;
  private rev = 1;

  write(key: DocKey, text: string): DiskRev {
    const rev = revOf(text);
    this.files.set(keyId(key), { rev, text });
    return rev;
  }
  count(method: string) {
    return this.calls.filter((c) => c.method === method).length;
  }
  of(method: string) {
    return this.calls.filter((c) => c.method === method).map((c) => c.params);
  }
  disconnect() {
    this.state = "reconnecting";
    this.leases.clear();
    for (const cb of this.states) cb(this.state);
  }
  reconnect() {
    this.state = "ready";
    for (const cb of this.states) cb(this.state);
  }
  /** Moves a file or folder like `documents.rename` and announces it once with the folder key. */
  move(from: DocKey, to: DocKey) {
    const under = (k: DocKey) => k.root === from.root && (k.path === from.path || k.path.startsWith(`${from.path}/`));
    const rekey = (k: DocKey): DocKey => ({ root: to.root, path: to.path + k.path.slice(from.path.length) });
    const parse = (id: string): DocKey => {
      const [root, path] = JSON.parse(id) as [string, string];
      return { root, path };
    };
    for (const map of [this.files, this.journals] as Map<string, unknown>[])
      for (const [id, value] of [...map]) {
        const k = parse(id);
        if (!under(k)) continue;
        map.delete(id);
        map.set(keyId(rekey(k)), value);
      }
    for (const lease of this.leases.values()) if (under(lease.key)) lease.key = rekey(lease.key);
    this.emit("documents.moved", { from, to });
  }
  /** Tells every subscriber it missed events. */
  resync() {
    for (const sub of this.subs) sub.onResync();
  }
  emit<T extends keyof Topics>(topic: T, payload: Topics[T]) {
    this.rev += 1;
    for (const sub of this.subs) if (sub.topic === topic) sub.onEvent(`${this.rev}`, payload);
  }
  onState(cb: (s: ClientState) => void) {
    this.states.add(cb);
    return () => {
      this.states.delete(cb);
    };
  }
  subscribe<T extends keyof Topics>(
    topic: T,
    onEvent: (rev: string, payload: Topics[T]) => void,
    onResync: () => void,
  ) {
    const sub: Sub = { topic, onEvent: onEvent as Sub["onEvent"], onResync };
    this.subs.add(sub);
    return () => {
      this.subs.delete(sub);
    };
  }
  call<M extends keyof Methods>(method: M, params: Methods[M]["params"]): Promise<Methods[M]["result"]> {
    this.calls.push({ method, params: params as Record<string, unknown> });
    if (this.state !== "ready") return Promise.reject(new RpcError("connection_lost"));
    const failure = this.failing.get(method);
    if (failure) return Promise.reject(new RpcError(failure as RpcError["code"]));
    try {
      return Promise.resolve(this.handle(method, params as unknown as Params) as Methods[M]["result"]);
    } catch (error) {
      return Promise.reject(error);
    }
  }
  private lease(id: string) {
    const lease = this.leases.get(id);
    if (!lease) throw new RpcError("documents.lease_lost", { reason: "expired" });
    return lease;
  }
  private handle(method: string, p: Params): unknown {
    switch (method) {
      case "documents.claim":
      case "documents.take": {
        const key = p.key;
        const file = this.files.get(keyId(key));
        const holder = [...this.leases].find(([, l]) => keyId(l.key) === keyId(key));
        if (holder && holder[1].window !== p.windowId && method === "documents.claim")
          return {
            key,
            leaseId: null,
            rev: file?.rev ?? null,
            text: file?.text ?? null,
            eol: "lf",
            bom: false,
            readOnly: { reason: "leased_elsewhere" },
            journal: null,
          };
        if (holder) this.leases.delete(holder[0]);
        const leaseId = `L${this.nextLease++}`;
        this.leases.set(leaseId, { key, window: p.windowId, floor: 0 });
        const j = this.journals.get(keyId(key));
        return {
          key,
          leaseId,
          rev: file?.rev ?? null,
          text: file?.text ?? "",
          eol: "lf",
          bom: false,
          readOnly: null,
          journal: j ? { seq: j.seq, baseRev: j.baseRev, text: j.text, deferred: false } : null,
        };
      }
      case "documents.save": {
        const lease = this.lease(p.leaseId);
        const { key } = lease;
        const file = this.files.get(keyId(key));
        const base = p.baseRev;
        const same = file
          ? !!base && base.hash === file.rev.hash && base.size === file.rev.size
          : p.expect === "absent";
        if (!same) throw new RpcError("documents.conflict", { rev: file?.rev ?? null });
        const rev = this.write(key, p.text);
        const j = this.journals.get(keyId(key));
        if (j && j.seq <= p.seq) this.journals.delete(keyId(key));
        lease.floor = Math.max(lease.floor, p.seq);
        return { rev };
      }
      case "documents.journal": {
        const lease = this.lease(p.leaseId);
        const { key } = lease;
        const j = this.journals.get(keyId(key));
        if ((j && p.seq <= j.seq) || p.seq <= lease.floor) throw new RpcError("documents.stale_journal");
        this.journals.set(keyId(key), { seq: p.seq, baseRev: p.baseRev, text: p.text });
        return { rev: `${this.rev}` };
      }
      case "documents.journal_drop": {
        const id = keyId(p.key);
        const holder = [...this.leases].find(([, l]) => keyId(l.key) === id);
        if (holder && holder[0] !== p.leaseId) throw new RpcError("documents.lease_lost", { reason: "taken" });
        if (holder) holder[1].floor = Math.max(holder[1].floor, p.uptoSeq);
        const j = this.journals.get(id);
        if (j && j.seq <= p.uptoSeq) this.journals.delete(id);
        return { rev: `${this.rev}` };
      }
      case "documents.journal_read": {
        const { key } = this.lease(p.leaseId);
        const j = this.journals.get(keyId(key));
        if (!j) throw new RpcError("documents.not_found");
        return j;
      }
      case "documents.read": {
        const key = p.key;
        const file = this.files.get(keyId(key));
        return { key, rev: file?.rev ?? null, text: file?.text ?? null, readOnly: null };
      }
      case "documents.release":
        this.leases.delete(p.leaseId);
        return { rev: `${this.rev}` };
      case "documents.renew":
        return { lost: p.leaseIds.filter((id) => !this.leases.has(id)) };
      default:
        throw new RpcError("method_not_found");
    }
  }
}
