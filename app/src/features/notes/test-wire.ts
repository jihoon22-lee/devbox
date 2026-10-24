import { RpcClient, RpcError, type Transport, type TransportState } from "../../rpc/client";
import { decodeFrame, jsonFrame } from "../../rpc/frame";
import type { Methods, Topics, TreeItem } from "../../rpc/gen/rpc";
import { FakeDocumentsDaemon } from "../documents/test-daemon";

type Handler = (params: Record<string, unknown>) => unknown;
type Request = { id: number; method: string; params: Record<string, unknown> };

/**
 * A scripted daemon for notes screens: `documents.*` go to a {@link FakeDocumentsDaemon},
 * `notes.*` to the handlers below (override any of them per test). Topics are delivered with {@link emit}.
 */
export class NotesWire implements Transport {
  docs = new FakeDocumentsDaemon();
  sent: Request[] = [];
  tree: TreeItem[] = [];
  handlers: Record<string, Handler> = {
    "notes.tree": () => ({ rev: "1", items: this.tree, nextCursor: null, truncated: false }),
    "notes.tags": () => ({ rev: "1", items: [], truncated: false }),
    "notes.backlinks": () => ({ rev: "1", items: [], truncated: false }),
    "notes.render": () => ({ html: "", mermaid: [] }),
    "notes.templates": () => ({ items: [] }),
  };
  client = new RpcClient(this, { version: "1.0.0-dev", instance: "notes-test", client: "test" });
  private frames = new Set<(frame: Uint8Array) => void>();
  private states = new Set<(state: TransportState) => void>();
  private topics = new Map<string, number[]>();
  connect() {
    for (const cb of this.states) cb("open");
  }
  close() {
    for (const cb of this.states) cb("closed");
  }
  onFrame(cb: (frame: Uint8Array) => void) {
    this.frames.add(cb);
    return () => {
      this.frames.delete(cb);
    };
  }
  onState(cb: (state: TransportState) => void) {
    this.states.add(cb);
    return () => {
      this.states.delete(cb);
    };
  }
  send(frame: Uint8Array) {
    const msg = JSON.parse(new TextDecoder().decode(decodeFrame(frame).body));
    if (msg.type === "hello") {
      queueMicrotask(() => this.deliver({ type: "welcome", protocol: 1, version: "1.0.0-dev", daemonId: "notes" }));
      return;
    }
    if (msg.type === "subscribe") {
      this.topics.set(msg.topic, [...(this.topics.get(msg.topic) ?? []), msg.id]);
      queueMicrotask(() => this.deliver({ type: "subscribed", id: msg.id, subId: msg.id, rev: "1" }));
      return;
    }
    if (typeof msg.method !== "string") return;
    this.sent.push(msg);
    void this.answer(msg);
  }
  private async answer(msg: Request) {
    try {
      let result: unknown;
      if (msg.method.startsWith("documents."))
        result = await this.docs.call(msg.method as keyof Methods, msg.params as never);
      else {
        const handler = this.handlers[msg.method];
        if (!handler) throw new RpcError("method_not_found");
        result = await handler(msg.params);
      }
      this.deliver({ type: "response", id: msg.id, result });
    } catch (error) {
      const e = error instanceof RpcError ? error : new RpcError("internal");
      this.deliver({ type: "response", id: msg.id, error: { code: e.code, detail: e.detail } });
    }
  }
  deliver(msg: unknown) {
    for (const cb of this.frames) cb(jsonFrame(msg));
  }
  emit<T extends keyof Topics>(topic: T, payload: Topics[T], rev = "2") {
    for (const subId of this.topics.get(topic) ?? []) this.deliver({ type: "event", subId, rev, payload });
  }
  requests(method: string) {
    return this.sent.filter((m) => m.method === method).map((m) => m.params);
  }
}
