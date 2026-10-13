import { readdirSync, readFileSync } from "node:fs";
import { setTimeout as delay } from "node:timers/promises";

interface ProcessIdentity {
  pid: number;
  session: number;
  started: string;
  state: string;
}
function processIdentity(pid: number): ProcessIdentity | undefined {
  try {
    const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
    const fields = stat
      .slice(stat.lastIndexOf(")") + 2)
      .trim()
      .split(/\s+/);
    if (fields.length < 20 || !/^\d+$/.test(fields[19]!) || !Number.isSafeInteger(Number(fields[3])))
      throw new Error(`Invalid process identity for owned pane ${pid}`);
    return { pid, session: Number(fields[3]), started: fields[19]!, state: fields[0]! };
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return;
    throw error;
  }
}

export function ownedPaneSessions(pids: number[]): ProcessIdentity[] {
  return pids.flatMap((pid) => {
    const process = processIdentity(pid);
    if (!process) throw new Error(`Cannot establish owned pane ${pid} identity; preserving fixture HOME`);
    // tmux starts each pane in its own session. Never wait on a shared host session.
    if (process.session !== pid) throw new Error(`Owned pane ${pid} does not own its process session`);
    return [process];
  });
}

/** Kill-server replies before pane HUP/EXIT finalizers finish writing fixture HOME. */
export async function waitPaneShutdown(owned: ProcessIdentity[], timeout = 5000): Promise<void> {
  if (!owned.length) return;
  const deadline = Date.now() + timeout;
  const terminated = new Set<string>();
  let emptyObserved = false;
  while (true) {
    const sessions = new Set(
      owned
        .filter((pane) => {
          const current = processIdentity(pane.pid);
          // A reused root PID belongs to a new session, not this fixture.
          return !current || current.started === pane.started;
        })
        .map((pane) => pane.session),
    );
    let raced = false;
    const live: ProcessIdentity[] = [];
    for (const name of readdirSync("/proc").filter((name) => /^\d+$/.test(name))) {
      const process = processIdentity(Number(name));
      if (!process) {
        raced = true;
        continue;
      }
      if (!sessions.has(process.session)) continue;
      if (["Z", "X"].includes(process.state)) {
        const identity = `${process.pid}:${process.started}`;
        if (!terminated.has(identity)) raced = true;
        terminated.add(identity);
      } else live.push(process);
    }
    // A listed process may fork a finalizer and exit before stat is read. Its
    // child was absent from that directory snapshot; require a fresh scan.
    if (!live.length && !raced) {
      if (emptyObserved) return;
      emptyObserved = true;
    } else emptyObserved = false;
    if (Date.now() >= deadline)
      throw new Error(
        `Owned tmux pane processes did not stop; preserving fixture HOME: ${live.map((process) => process.pid).join(", ") || "process snapshot changed during enumeration"}`,
      );
    await delay(20);
  }
}
