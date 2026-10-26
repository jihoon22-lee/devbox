import { decodeFrame, FRAME } from "../src/rpc/frame";

export type RpcMessage = { type?: string; id?: number; method?: string; daemonId?: string; error?: unknown };

/** Replaces the single owned anchor; any ambiguity is refused so the restore stays byte-identical. */
export function markSource(source: string, anchor: string, replacement: string): string {
  const at = source.indexOf(anchor);
  if (at < 0 || source.includes(anchor, at + 1)) throw new Error("SC6 source anchor must occur exactly once");
  if (source.includes(replacement)) throw new Error("SC6 marker is already present");
  return source.slice(0, at) + replacement + source.slice(at + anchor.length);
}

/** JSON control messages only; stream bytes and text frames are not RPC messages. */
export function rpcMessage(payload: string | Uint8Array): RpcMessage | undefined {
  if (typeof payload === "string") return undefined;
  try {
    const frame = decodeFrame(payload);
    return frame.kind === FRAME.json ? JSON.parse(new TextDecoder().decode(frame.body)) : undefined;
  } catch {
    return undefined;
  }
}

/** Done after a new daemon identity welcomes this page and answers a data request sent after that welcome. */
export class ReconnectWatch {
  private welcomed = false;
  private requests = new Set<number>();
  constructor(
    private previousDaemonId: string | undefined,
    private method = "agents.list",
  ) {}
  sent(message: RpcMessage): void {
    if (this.welcomed && message.type === "request" && message.method === this.method && message.id !== undefined)
      this.requests.add(message.id);
  }
  received(message: RpcMessage): boolean {
    if (message.type === "welcome") {
      this.welcomed = message.daemonId !== undefined && message.daemonId !== this.previousDaemonId;
      this.requests.clear();
      return false;
    }
    return (
      this.welcomed &&
      message.type === "response" &&
      message.id !== undefined &&
      this.requests.has(message.id) &&
      !message.error
    );
  }
}
