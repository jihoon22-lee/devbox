import { RpcClient, type Transport, type TransportState } from "../../rpc/client";
import { decodeFrame, jsonFrame } from "../../rpc/frame";
import { trustPreview } from "./trust-fixtures";
export class ProjectWire implements Transport {
  constructor(private options: { trustNotRequired?: boolean } = {}) {}
  sent: Record<string, unknown>[] = [];
  frames = new Set<(frame: Uint8Array) => void>();
  states = new Set<(state: TransportState) => void>();
  client = new RpcClient(this, { version: "1.0.0-dev", instance: "projects-test", client: "test" });
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
    this.sent.push(msg);
    if (msg.method === "projects.trust_preview" && this.options.trustNotRequired)
      queueMicrotask(() =>
        this.deliver({
          type: "response",
          id: msg.id,
          result: trustPreview(msg.params.id, { requiresApproval: false }),
        }),
      );
    if (msg.type === "hello")
      queueMicrotask(() => this.deliver({ type: "welcome", protocol: 1, version: "1.0.0-dev", daemonId: "projects" }));
    if (msg.type === "subscribe")
      queueMicrotask(() => this.deliver({ type: "subscribed", id: msg.id, subId: msg.id, rev: "1" }));
  }
  deliver(msg: unknown) {
    for (const cb of this.frames) cb(jsonFrame(msg));
  }
  requests(method = "projects.list") {
    return this.sent.filter((m) => m.method === method);
  }
  request(method = "projects.list", index = 0) {
    const request = this.requests(method).at(index);
    if (!request) throw new Error(`Missing ${method} request at index ${index}`);
    return request;
  }
  reply(method: string, result: unknown) {
    this.deliver({ type: "response", id: this.request(method, -1).id, result });
  }
  fail(method: string) {
    this.deliver({ type: "response", id: this.request(method, -1).id, error: { code: "internal" } });
  }
}
export const projects = [
  { id: "a", name: "devbox", path: "/fixture/devbox", favorite: false, addedMs: 1 },
  { id: "b", name: "family", path: "/fixture/family", favorite: false, addedMs: 2 },
];
