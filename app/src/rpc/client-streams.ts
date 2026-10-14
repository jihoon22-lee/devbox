import { FRAME, jsonFrame, MAX_STREAM_CHUNK, parseStreamFrame, streamFrame } from "./frame";

export interface StreamHandlers {
  onData(bytes: Uint8Array, ack: () => void): void;
  onEnd(): void;
}
export interface ClientStream {
  streamId: number;
  write(bytes: Uint8Array): void;
  /** Dispose both directions; FIN only ends the peer's output. */
  end(): void;
}
interface Entry {
  handlers: StreamHandlers;
  generation: number;
  closed: boolean;
  unacked: number;
  id?: number;
}
const WINDOW = 256 * 1024;
const MAX_EARLY_IDS = 64;
const MAX_EARLY_BYTES = 16 * WINDOW;

/** Fixed-size pages bound allocation even for a full window of one-byte frames. */
class Early {
  pages: Uint8Array[] = [];
  bytes = 0;
  ended = false;
  append(data: Uint8Array, fin: boolean): void {
    if (this.ended && data.length) throw new Error("Data after stream FIN");
    if (this.bytes + data.length > WINDOW) throw new Error("Early stream exceeds credit window");
    let offset = 0;
    while (offset < data.length) {
      const used = this.bytes % MAX_STREAM_CHUNK;
      if (!used) this.pages.push(new Uint8Array(MAX_STREAM_CHUNK));
      const size = Math.min(data.length - offset, MAX_STREAM_CHUNK - used);
      this.pages[this.pages.length - 1]!.set(data.subarray(offset, offset + size), used);
      this.bytes += size;
      offset += size;
    }
    this.ended ||= fin;
  }
}
function notify(callback: () => void): void {
  try {
    callback();
  } catch (error) {
    console.error("RPC stream listener failed", error);
  }
}

/** Connection-local ownership; old handles never send on a replacement transport. */
export class ClientStreams {
  private generation = 0;
  private opening = new Set<Entry>();
  private active = new Map<number, Entry>();
  private early = new Map<number, Early>();
  private earlyBytes = 0;
  constructor(private send: (frame: Uint8Array) => boolean) {}

  begin(handlers: StreamHandlers): Entry {
    const entry: Entry = { handlers, generation: this.generation, closed: false, unacked: 0 };
    this.opening.add(entry);
    return entry;
  }
  current(entry: Entry): boolean {
    return !entry.closed && entry.generation === this.generation;
  }
  reject(entry: Entry): void {
    this.opening.delete(entry);
    this.finish(entry);
    this.clearOrphans();
  }
  attach(entry: Entry, result: unknown): ClientStream {
    if (!this.current(entry)) throw new Error("Stream connection closed");
    const id = typeof result === "object" && result !== null && "streamId" in result ? result.streamId : undefined;
    if (typeof id !== "number" || !Number.isInteger(id) || id < 0 || id > 0xffffffff || this.active.has(id)) {
      throw new Error("Invalid or duplicate stream ID");
    }
    entry.id = id;
    this.opening.delete(entry);
    this.active.set(id, entry);
    const early = this.early.get(id);
    this.early.delete(id);
    if (early) {
      this.earlyBytes -= early.bytes;
      // One delivery preserves order if ACK synchronously produces more live data.
      // Consumers observe bytes, not transport frame boundaries.
      const data = new Uint8Array(early.bytes);
      for (const [index, page] of early.pages.entries()) {
        const offset = index * MAX_STREAM_CHUNK;
        data.set(page.subarray(0, Math.min(data.length - offset, MAX_STREAM_CHUNK)), offset);
      }
      this.deliver(entry, data, early.ended);
    }
    this.clearOrphans();
    return {
      streamId: id,
      write: (bytes) => {
        for (let offset = 0; offset < bytes.length && this.current(entry); offset += MAX_STREAM_CHUNK) {
          if (!this.send(streamFrame(id, 0, bytes.subarray(offset, offset + MAX_STREAM_CHUNK)))) break;
        }
      },
      end: () => {
        if (!this.current(entry)) return;
        // Invalidate first: send may synchronously drop/reopen the transport.
        entry.closed = true;
        this.active.delete(id);
        this.send(jsonFrame({ type: "streamClose", stream: id }, FRAME.control));
        notify(() => entry.handlers.onEnd());
      },
    };
  }
  receive(body: Uint8Array): void {
    const { streamId, flags, data } = parseStreamFrame(body);
    const fin = flags === 1;
    const entry = this.active.get(streamId);
    if (entry) {
      this.deliver(entry, data, fin);
      return;
    }
    // Late data after disposal is harmless. Only outstanding opens can own early data.
    if (!this.opening.size || (!data.length && !fin)) return;
    let early = this.early.get(streamId);
    if (!early) {
      if (this.early.size >= MAX_EARLY_IDS) throw new Error("Too many early stream IDs");
      early = new Early();
      this.early.set(streamId, early);
    }
    if (this.earlyBytes + data.length > MAX_EARLY_BYTES) throw new Error("Early stream buffer full");
    early.append(data, fin);
    this.earlyBytes += data.length;
  }
  reset(): void {
    this.generation++;
    const entries = [...this.opening, ...this.active.values()];
    this.opening.clear();
    this.active.clear();
    this.early.clear();
    this.earlyBytes = 0;
    for (const entry of entries) entry.closed = true;
    for (const entry of entries) notify(() => entry.handlers.onEnd());
  }
  private finish(entry: Entry): void {
    if (entry.closed) return;
    entry.closed = true;
    if (entry.id !== undefined && this.active.get(entry.id) === entry) this.active.delete(entry.id);
    notify(() => entry.handlers.onEnd());
  }
  private deliver(entry: Entry, data: Uint8Array, fin: boolean): void {
    if (!this.current(entry)) return;
    if (entry.unacked + data.length > WINDOW) throw new Error("Stream exceeds credit window");
    if (data.length) {
      const size = data.length;
      entry.unacked += size;
      let used = false;
      notify(() =>
        entry.handlers.onData(data, () => {
          if (used || !this.current(entry)) return;
          used = true;
          entry.unacked -= size;
          this.send(jsonFrame({ type: "streamCredit", stream: entry.id, bytes: size }, FRAME.control));
        }),
      );
    }
    if (fin) this.finish(entry);
  }
  private clearOrphans(): void {
    if (this.opening.size) return;
    const ids = [...this.early.keys()];
    this.early.clear();
    this.earlyBytes = 0;
    const generation = this.generation;
    for (const id of ids) {
      if (generation !== this.generation || !this.send(jsonFrame({ type: "streamClose", stream: id }, FRAME.control)))
        break;
    }
  }
}
