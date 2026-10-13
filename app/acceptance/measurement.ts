import type { ClientStream } from "../src/rpc/client-streams";

export function percentile(samples: number[], fraction: number): number {
  if (!samples.length || samples.some((n) => !Number.isFinite(n) || n < 0)) throw new Error("Invalid samples");
  if (fraction <= 0 || fraction > 1) throw new Error("Invalid percentile");
  const sorted = [...samples].sort((a, b) => a - b);
  return sorted[Math.ceil(sorted.length * fraction) - 1]!;
}

/** One outstanding byte; timestamp reception before ACK or screen drawing. */
export class ByteEcho {
  private pending?: { byte: number; started: number; resolve: (ms: number) => void; reject: (e: Error) => void };
  constructor(private now = () => performance.now()) {}
  receive(bytes: Uint8Array): void {
    const p = this.pending;
    if (!p || !bytes.includes(p.byte)) return;
    this.pending = undefined;
    p.resolve(this.now() - p.started);
  }
  fail(): void {
    const p = this.pending;
    this.pending = undefined;
    p?.reject(new Error("Echo stream ended or timed out"));
  }
  async send(stream: Pick<ClientStream, "write">, byte: number, timeout = 1000): Promise<number> {
    if (this.pending) throw new Error("Overlapping echo samples");
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      return await new Promise<number>((resolve, reject) => {
        this.pending = { byte, started: this.now(), resolve, reject };
        timer = setTimeout(() => this.fail(), timeout);
        try {
          stream.write(Uint8Array.of(byte));
        } catch (error) {
          this.pending = undefined;
          reject(error);
        }
      });
    } finally {
      clearTimeout(timer);
    }
  }
}

export type PingSample = { started: number; finished: number; ms: number };
export function floodVerdict(samples: PingSample[], started: number, finished: number, recoveryMs: number) {
  // Include every ping begun during output, even when its response arrives later.
  const during = samples.filter((s) => s.started >= started && s.started <= finished);
  if (during.length < 20) throw new Error(`Insufficient concurrent ping samples: ${during.length}`);
  const p95 = percentile(
    during.map((s) => s.ms),
    0.95,
  );
  return { samples: during, p95, recoveryMs, pass: p95 <= 100 && recoveryMs <= 1000 };
}
