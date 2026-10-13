import { type RpcClient, RpcError } from "../../rpc/client";
import type { ClientStream } from "../../rpc/client-streams";

export interface TerminalSink {
  write(data: Uint8Array, done: () => void): void;
  reset(): void;
  resize?(cols: number, rows: number): void;
}
export type LinkState = "connecting" | "live" | "lost" | "ended";
interface Options {
  id: string;
  readOnly: boolean;
  sink: TerminalSink;
  onState(state: LinkState): void;
  onInputDropped(): void;
}
interface Attachment {
  closed: boolean;
  stream?: ClientStream;
  cols: number;
  rows: number;
}

/** Owns one terminal attachment; rendering completion returns that attachment's credit. */
export class TerminalLink {
  private state?: LinkState;
  private attachment?: Attachment;
  private cols = 80;
  private rows = 24;
  private started = false;
  private disposed = false;
  private attached = false;
  private warned = false;
  private recoveries = 1;
  private resizeTimer?: ReturnType<typeof setTimeout>;
  private recoveryTimer?: ReturnType<typeof setTimeout>;
  private off: () => void;
  constructor(
    private client: RpcClient,
    private options: Options,
  ) {
    this.off = client.onState((state) => {
      if (!this.started || this.disposed || this.state === "ended") return;
      if (state !== "ready") {
        this.invalidate();
        this.setState("lost");
        return;
      }
      this.recoveries = 1;
      void this.attach(this.attached);
    });
  }
  async open(cols: number, rows: number): Promise<void> {
    if (this.disposed || this.state === "ended") return;
    this.cols = cols;
    this.rows = rows;
    this.started = true;
    this.recoveries = 1;
    if (this.client.state !== "ready") {
      this.invalidate();
      this.setState("connecting");
      return;
    }
    await this.attach(this.attached);
  }
  /** Uint8Array preserves xterm onBinary input; strings use UTF-8. */
  input(text: string | Uint8Array): void {
    if (this.disposed || this.options.readOnly) return;
    const a = this.attachment;
    if (this.state !== "live" || !a?.stream || a.closed) {
      this.inputDropped();
      return;
    }
    try {
      a.stream.write(typeof text === "string" ? new TextEncoder().encode(text) : text);
      if (!this.current(a)) this.inputDropped();
    } catch {
      this.invalidate();
      this.setState("lost");
      this.inputDropped();
    }
  }
  resize(cols: number, rows: number): void {
    if (this.disposed) return;
    this.cols = cols;
    this.rows = rows;
    this.scheduleResize();
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.off();
    this.invalidate();
  }
  private current(a: Attachment): boolean {
    return !this.disposed && this.attachment === a && !a.closed;
  }
  private setState(state: LinkState): void {
    if (this.disposed || this.state === state) return;
    this.state = state;
    this.options.onState(state);
  }
  private inputDropped(): void {
    if (this.warned || this.disposed) return;
    this.warned = true;
    this.options.onInputDropped();
  }
  private clearTimers(): void {
    if (this.resizeTimer !== undefined) clearTimeout(this.resizeTimer);
    if (this.recoveryTimer !== undefined) clearTimeout(this.recoveryTimer);
    this.resizeTimer = undefined;
    this.recoveryTimer = undefined;
  }
  private close(stream: ClientStream | undefined): void {
    try {
      stream?.end();
    } catch {
      /* Already disconnected. */
    }
  }
  private invalidate(): void {
    this.clearTimers();
    const old = this.attachment;
    this.attachment = undefined;
    if (!old) return;
    old.closed = true;
    this.close(old.stream);
  }
  private ended(a: Attachment): void {
    if (!this.current(a)) return;
    a.closed = true;
    this.clearTimers();
    this.setState("lost");
    // RpcClient resets streams before publishing reconnecting. Defer the decision.
    // One probe per connection/open detects not_found without looping on repeated EOF.
    if (this.disposed || this.attachment !== a || !this.recoveries) return;
    this.recoveryTimer = setTimeout(() => {
      this.recoveryTimer = undefined;
      if (this.disposed || this.attachment !== a || this.client.state !== "ready" || this.state === "ended") return;
      this.recoveries--;
      void this.attach(true);
    }, 100);
  }
  private async attach(reset: boolean): Promise<void> {
    this.invalidate();
    const a: Attachment = { closed: false, cols: this.cols, rows: this.rows };
    this.attachment = a;
    if (reset) {
      try {
        this.options.sink.reset();
      } catch {
        a.closed = true;
        this.setState("lost");
        return;
      }
    }
    if (!this.current(a)) return;
    this.setState("connecting");
    if (!this.current(a)) return;
    this.attached = true;
    try {
      const stream = await this.client.openStream(
        "terminal.attach",
        {
          id: this.options.id,
          cols: a.cols,
          rows: a.rows,
          readOnly: this.options.readOnly,
        },
        {
          onOpened: ({ cols, rows }) => {
            if (!this.current(a) || !this.options.readOnly) return;
            if (
              !Number.isInteger(cols) ||
              !Number.isInteger(rows) ||
              cols < 1 ||
              rows < 1 ||
              cols > 65535 ||
              rows > 65535
            )
              throw new Error("Invalid terminal grid");
            this.options.sink.resize?.(cols, rows);
          },
          onData: (data, ack) => {
            if (!this.current(a)) return;
            let done = false;
            try {
              this.options.sink.write(data, () => {
                if (done || !this.current(a)) return;
                done = true;
                ack();
              });
            } catch {
              if (!this.current(a)) return;
              a.closed = true;
              this.clearTimers();
              this.close(a.stream);
              this.setState("lost");
            }
          },
          onEnd: () => this.ended(a),
        },
      );
      if (!this.current(a)) {
        this.close(stream);
        return;
      }
      a.stream = stream;
      this.warned = false;
      this.setState("live");
      if (this.current(a) && (a.cols !== this.cols || a.rows !== this.rows)) this.scheduleResize();
    } catch (error) {
      // onEnd can precede rejection, so identity (not closed) determines ownership.
      if (this.disposed || this.attachment !== a) return;
      a.closed = true;
      this.clearTimers();
      this.setState(error instanceof RpcError && error.code === "terminal.not_found" ? "ended" : "lost");
    }
  }
  private scheduleResize(): void {
    if (this.resizeTimer !== undefined) clearTimeout(this.resizeTimer);
    this.resizeTimer = undefined;
    const a = this.attachment;
    if (this.options.readOnly || !a?.stream || !this.current(a) || this.state !== "live") return;
    this.resizeTimer = setTimeout(() => {
      this.resizeTimer = undefined;
      if (!this.current(a) || !a.stream || this.state !== "live") return;
      void this.client
        .call("terminal.resize", { streamId: a.stream.streamId, cols: this.cols, rows: this.rows })
        .catch(() => {});
    }, 100);
  }
}
