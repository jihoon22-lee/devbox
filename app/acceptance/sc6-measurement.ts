import { decodeFrame, FRAME } from "../src/rpc/frame";

export interface ProcReader {
  fields(pid: number, name: "cmdline" | "environ"): string[];
  cwd(pid: number): string;
}

/**
 * SIGUSR1 restarts a dev-session's daemon, so never adopt an arbitrary (possibly live) session:
 * the process must be this checkout's scripts/dev-session.py, started with the same one-off
 * DEVBOX_SC6_SESSION_TOKEN that the harness was given (an explicit owned marker).
 */
export function ownedSession(env: Record<string, string | undefined>, root: string, proc: ProcReader): number {
  const pid = Number(env.DEVBOX_SC6_SESSION_PID);
  const token = env.DEVBOX_SC6_SESSION_TOKEN ?? "";
  const refuse = new Error(
    "Set DEVBOX_SC6_SESSION_PID and DEVBOX_SC6_SESSION_TOKEN to a scripts/dev-session.py of this checkout started with that token",
  );
  if (!Number.isInteger(pid) || pid <= 1 || !/^[A-Za-z0-9_-]{16,128}$/.test(token)) throw refuse;
  const safely = <T>(read: () => T, fallback: T) => {
    try {
      return read();
    } catch {
      return fallback;
    }
  };
  const owned =
    safely(() => proc.fields(pid, "cmdline"), []).some((part) => part.endsWith("scripts/dev-session.py")) &&
    safely(() => proc.fields(pid, "environ"), []).includes(`DEVBOX_SC6_SESSION_TOKEN=${token}`) &&
    safely(() => proc.cwd(pid), "") === root;
  if (!owned) throw refuse;
  return pid;
}

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
