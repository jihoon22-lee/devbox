import { type ClientStream, ClientStreams, type StreamHandlers } from "./client-streams";
import { decodeFrame, FRAME, jsonFrame } from "./frame";
import type { Methods, RpcErrorCode, Topics } from "./gen/rpc";
import { parseServerMessage, type ServerMessage } from "./server-message";
export type TransportState = "connecting" | "open" | "closed";
export interface Transport {
  connect(): void;
  close(): void;
  /** Drop the current connection and retry with backoff. */
  reconnect?(): void;
  send(frame: Uint8Array): void;
  onFrame(cb: (frame: Uint8Array) => void): () => void;
  onState(cb: (state: TransportState) => void): () => void;
}
export type ClientState = "connecting" | "ready" | "reconnecting" | "versionMismatch";
type StreamMethod = { [M in keyof Methods]: Methods[M]["kind"] extends "stream" ? M : never }[keyof Methods];
export type ErrorCode = RpcErrorCode | "connection_lost";
export class RpcError extends Error {
  constructor(
    readonly code: ErrorCode,
    readonly detail?: unknown,
    readonly diagnosticId?: string,
  ) {
    super(code);
    this.name = "RpcError";
  }
}
type Pending = { resolve: (v: unknown) => void; reject: (e: RpcError) => void; cleanup?: () => void; subKey?: number };
type Sub = { topic: string; onEvent: (rev: string, payload: unknown) => void; onResync: () => void; subId?: number };
function notify(cb: () => void): void {
  try {
    cb();
  } catch (error) {
    console.error("RPC listener failed", error);
  }
}
export class RpcClient {
  state: ClientState = "connecting";
  daemonId?: string;
  daemonVersion?: string;
  private streams = new ClientStreams((frame) => this.sendRaw(frame));
  private nextId = 1;
  private nextSubKey = 1;
  private pending = new Map<number, Pending>();
  private subs = new Map<number, Sub>();
  private bySubId = new Map<number, number>();
  private stateCbs = new Set<(s: ClientState) => void>();
  private resetCbs = new Set<() => void>();
  private offs: (() => void)[] = [];
  private retryTimer?: ReturnType<typeof setTimeout>;
  private welcomeTimer?: ReturnType<typeof setTimeout>;
  private connectionEpoch = 0;
  private retryAttempt = 0;
  private running = false;
  private live = false;
  private welcomed = false;
  private registering = false;
  private everReady = false;
  private restoring = false;
  constructor(
    private transport: Transport,
    private opts: { version: string; instance: string; client: "app" | "dev" | "test" },
  ) {}
  start(): void {
    if (this.running || this.state === "versionMismatch") return;
    this.running = true;
    this.offs = [
      this.transport.onFrame((raw) => this.onFrame(raw)),
      this.transport.onState((s) => {
        if (!this.running) return;
        if (s === "open" && !this.live) {
          if (this.retryTimer !== undefined) clearTimeout(this.retryTimer);
          this.retryTimer = undefined;
          this.live = true;
          const epoch = ++this.connectionEpoch;
          // Arm before send: an in-process transport can synchronously return Welcome.
          this.welcomeTimer = setTimeout(() => {
            if (epoch !== this.connectionEpoch || !this.running || !this.live || this.welcomed) return;
            this.failConnection();
          }, 5000);
          this.send({
            type: "hello",
            version: this.opts.version,
            protocol: 1,
            client: this.opts.client,
            instance: this.opts.instance,
          });
        } else if (s === "closed") this.onClosed();
      }),
    ];
    try {
      this.transport.connect();
    } catch {
      this.failConnection();
    }
  }
  stop(): void {
    if (!this.running) return;
    this.running = false;
    if (this.retryTimer !== undefined) clearTimeout(this.retryTimer);
    this.retryTimer = undefined;
    this.retryAttempt = 0;
    for (const off of this.offs) off();
    this.offs = [];
    this.onClosed();
    this.transport.close();
  }
  onState(cb: (s: ClientState) => void): () => void {
    this.stateCbs.add(cb);
    return () => {
      this.stateCbs.delete(cb);
    };
  }
  onReset(cb: () => void): () => void {
    this.resetCbs.add(cb);
    return () => {
      this.resetCbs.delete(cb);
    };
  }
  call<M extends keyof Methods>(
    method: M,
    params: Methods[M]["params"],
    opts: { clientRequestId?: string; signal?: AbortSignal } = {},
  ): Promise<Methods[M]["result"]> {
    if (opts.signal?.aborted) return Promise.reject(new RpcError("cancelled"));
    if (!this.running || !this.live || !this.welcomed || this.state !== "ready")
      return Promise.reject(new RpcError("connection_lost"));
    const id = this.allocateId();
    return new Promise((resolve, reject) => {
      // Cancellation is cooperative: an accepted mutation may still commit and return success.
      const abort = () => {
        this.send({ type: "cancel", id });
      };
      this.pending.set(id, {
        resolve: resolve as (v: unknown) => void,
        reject,
        cleanup: () => opts.signal?.removeEventListener("abort", abort),
      });
      opts.signal?.addEventListener("abort", abort, { once: true });
      this.send({
        type: "request",
        id,
        method,
        params,
        ...(opts.clientRequestId !== undefined ? { clientRequestId: opts.clientRequestId } : {}),
      });
    });
  }
  async openStream<M extends StreamMethod>(
    method: M,
    params: Methods[M]["params"],
    handlers: StreamHandlers,
  ): Promise<ClientStream & { result: Methods[M]["result"] }> {
    if (!this.running || !this.live || !this.welcomed || this.state !== "ready") throw new RpcError("connection_lost");
    const epoch = this.connectionEpoch;
    const entry = this.streams.begin(handlers);
    let result: Methods[M]["result"];
    try {
      result = await this.call(method, params);
    } catch (error) {
      this.streams.reject(entry);
      throw error;
    }
    if (epoch !== this.connectionEpoch || !this.streams.current(entry)) throw new RpcError("connection_lost");
    let stream: ClientStream;
    try {
      stream = this.streams.attach(entry, result);
    } catch {
      this.failConnection();
      throw new RpcError("connection_lost");
    }
    // Callbacks while replaying early data may close the connection synchronously.
    if (epoch !== this.connectionEpoch) throw new RpcError("connection_lost");
    return { ...stream, result };
  }
  subscribe<T extends keyof Topics>(
    topic: T,
    onEvent: (rev: string, payload: Topics[T]) => void,
    onResync: () => void,
  ): () => void {
    const key = this.nextSubKey++;
    this.subs.set(key, { topic, onEvent: onEvent as Sub["onEvent"], onResync });
    if (this.welcomed) this.sendSubscribe(key);
    return () => {
      const sub = this.subs.get(key);
      if (!sub) return;
      this.subs.delete(key);
      if (sub.subId !== undefined) {
        this.bySubId.delete(sub.subId);
        this.send({ type: "unsubscribe", subId: sub.subId });
      }
      // Keep pending subscription metadata to clean up a late acknowledgement.
      this.finishRegistration();
    };
  }
  private allocateId(): number {
    while (this.pending.has(this.nextId)) this.nextId = this.nextId === 0xffffffff ? 1 : this.nextId + 1;
    const id = this.nextId;
    this.nextId = id === 0xffffffff ? 1 : id + 1;
    return id;
  }
  private sendSubscribe(key: number): void {
    const sub = this.subs.get(key);
    if (!sub || !this.welcomed || sub.subId !== undefined) return;
    for (const p of this.pending.values()) if (p.subKey === key) return;
    const id = this.allocateId();
    this.pending.set(id, { resolve: () => {}, reject: () => {}, subKey: key });
    this.send({ type: "subscribe", id, topic: sub.topic });
  }
  private send(msg: unknown): void {
    try {
      this.sendRaw(jsonFrame(msg));
    } catch {
      this.failConnection();
    }
  }
  private sendRaw(frame: Uint8Array): boolean {
    if (!this.running || !this.live) return false;
    try {
      this.transport.send(frame);
      return true;
    } catch {
      this.failConnection();
      return false;
    }
  }
  private setState(s: ClientState): void {
    if (this.state === s) return;
    this.state = s;
    for (const cb of this.stateCbs) notify(() => cb(s));
  }
  private take(id: number): Pending | undefined {
    const p = this.pending.get(id);
    this.pending.delete(id);
    p?.cleanup?.();
    return p;
  }
  private onClosed(): void {
    this.connectionEpoch++;
    this.clearWelcomeTimer();
    this.live = false;
    this.welcomed = false;
    this.restoring = false;
    this.streams.reset();
    for (const id of this.pending.keys()) this.take(id)?.reject(new RpcError("connection_lost"));
    this.bySubId.clear();
    for (const sub of this.subs.values()) sub.subId = undefined;
    if (this.state !== "versionMismatch") this.setState(this.everReady ? "reconnecting" : "connecting");
  }
  private clearWelcomeTimer(): void {
    if (this.welcomeTimer !== undefined) clearTimeout(this.welcomeTimer);
    this.welcomeTimer = undefined;
  }
  private failConnection(): void {
    if (!this.running) return;
    this.onClosed();
    if (this.transport.reconnect) {
      this.transport.reconnect();
      return;
    }
    this.transport.close();
    if (this.retryTimer !== undefined) return;
    this.retryTimer = setTimeout(
      () => {
        this.retryTimer = undefined;
        if (!this.running) return;
        try {
          this.transport.connect();
        } catch {
          this.failConnection();
        }
      },
      Math.min(200 * 2 ** Math.min(this.retryAttempt++, 5), 5000),
    );
  }
  private finishRegistration(): void {
    if (!this.welcomed || this.registering) return;
    for (const p of this.pending.values()) if (p.subKey !== undefined && this.subs.has(p.subKey)) return;
    const resync = this.restoring;
    this.restoring = false;
    this.everReady = true;
    this.retryAttempt = 0;
    this.setState("ready");
    if (resync && this.welcomed) for (const sub of this.subs.values()) notify(sub.onResync);
  }
  private onFrame(raw: Uint8Array): void {
    if (!this.running || !this.live) return;
    let msg: ServerMessage;
    try {
      const frame = decodeFrame(raw);
      if (frame.kind === FRAME.stream) {
        if (!this.welcomed) throw new Error("Stream before Welcome");
        this.streams.receive(frame.body);
        return;
      }
      if (frame.kind !== FRAME.json) return;
      msg = parseServerMessage(frame.body);
      if ((msg.type === "welcome") === this.welcomed) throw new Error("Unexpected handshake state");
    } catch {
      this.failConnection();
      return;
    }
    this.receive(msg);
  }
  private receive(msg: ServerMessage): void {
    switch (msg.type) {
      case "welcome": {
        this.clearWelcomeTimer();
        this.daemonVersion = msg.version;
        if (msg.protocol !== 1 || msg.version !== this.opts.version) {
          this.setState("versionMismatch");
          this.stop();
          return;
        }
        const changed = this.daemonId !== undefined && this.daemonId !== msg.daemonId;
        this.daemonId = msg.daemonId;
        this.welcomed = true;
        this.restoring = this.everReady;
        this.registering = true;
        if (changed) for (const cb of this.resetCbs) notify(cb);
        for (const key of this.subs.keys()) this.sendSubscribe(key);
        this.registering = false;
        this.finishRegistration();
        return;
      }
      case "response": {
        const p = this.pending.get(msg.id);
        if (!p) return;
        if (p.subKey !== undefined && !msg.error) {
          this.failConnection();
          return;
        }
        this.take(msg.id);
        if (msg.error) p.reject(new RpcError(msg.error.code, msg.error.detail, msg.diagnosticId));
        else p.resolve(msg.result);
        this.finishRegistration();
        return;
      }
      case "subscribed": {
        const p = this.pending.get(msg.id);
        if (!p || p.subKey === undefined || this.bySubId.has(msg.subId)) {
          this.failConnection();
          return;
        }
        this.take(msg.id);
        const sub = this.subs.get(p.subKey);
        if (sub) {
          sub.subId = msg.subId;
          this.bySubId.set(msg.subId, p.subKey);
        } else this.send({ type: "unsubscribe", subId: msg.subId });
        this.finishRegistration();
        return;
      }
      case "event": {
        const key = this.bySubId.get(msg.subId);
        if (key !== undefined) notify(() => this.subs.get(key)?.onEvent(msg.rev, msg.payload));
        return;
      }
      case "resync": {
        const key = this.bySubId.get(msg.subId);
        if (key !== undefined) notify(() => this.subs.get(key)?.onResync());
      }
    }
  }
}
