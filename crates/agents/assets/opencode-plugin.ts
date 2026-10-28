// devbox opencode plugin. devbox writes this file and loads it only in its own tasks, through
// OPENCODE_CONFIG_CONTENT, next to the user's own plugins. It reports the task's session to
// devbox with `devbox agent event --from opencode <report>`, imports nothing but
// node:child_process, and never throws into opencode.
import { spawn } from "node:child_process";

/** One report: the last argument of `devbox agent event --from opencode`. */
export type DevboxPayload = {
  v: 1;
  event: "working" | "idle" | "needsInput" | "inputResolved";
  sessionId?: string;
  message?: string;
};

/** What the plugin remembers about the sessions of one opencode process. */
export type PluginState = {
  /** Sessions with a parent: subagents, whose turns are not the task's. */
  children: Set<string>;
  /** Root sessions reported as working. */
  busy: Set<string>;
  /** Open permission requests by id, with the session that asked. */
  pending: Map<string, string>;
};

type Fields = Record<string, unknown>;

/** How long one report may take before the next is sent anyway. `devbox` gives up after 3 s. */
const REPORT_WAIT_MS = 5000;

export function newState(): PluginState {
  return { children: new Set(), busy: new Set(), pending: new Map() };
}

function fields(value: unknown): Fields | undefined {
  return typeof value === "object" && value !== null ? (value as Fields) : undefined;
}

function text(value: unknown): string | undefined {
  return typeof value === "string" && value !== "" ? value : undefined;
}

/** The report one opencode event makes, if any. */
export function toDevboxEvent(event: unknown, state: PluginState): DevboxPayload | undefined {
  const properties = fields(fields(event)?.properties);
  if (!properties) return undefined;
  switch (fields(event)?.type) {
    case "session.created":
    case "session.updated": {
      const info = fields(properties.info);
      const id = text(info?.id);
      if (id && text(info?.parentID)) state.children.add(id);
      return undefined;
    }
    case "session.status": {
      const session = text(properties.sessionID);
      const status = text(fields(properties.status)?.type);
      if (!session || !status) return undefined;
      if (status === "busy" || status === "retry") {
        if (state.children.has(session) || state.busy.has(session)) return undefined;
        state.busy.add(session);
        return { v: 1, event: "working", sessionId: session };
      }
      if (status !== "idle") return undefined;
      // A session that stops drops its open requests without replying to them.
      let dropped = false;
      for (const [id, asker] of state.pending) {
        if (asker === session) {
          state.pending.delete(id);
          dropped = true;
        }
      }
      if (state.busy.delete(session)) return { v: 1, event: "idle" };
      return dropped && state.pending.size === 0 ? { v: 1, event: "inputResolved" } : undefined;
    }
    case "permission.asked": {
      // A subagent's request stops the task as well, so every session's requests count.
      const id = text(properties.id);
      const session = text(properties.sessionID);
      if (!id || !session || state.pending.has(id)) return undefined;
      state.pending.set(id, session);
      // The permission's name only: its patterns can hold a command line or a path.
      const permission = text(properties.permission);
      return permission ? { v: 1, event: "needsInput", message: permission } : { v: 1, event: "needsInput" };
    }
    case "permission.replied": {
      const id = text(properties.requestID);
      if (!id || !state.pending.delete(id)) return undefined;
      return state.pending.size === 0 ? { v: 1, event: "inputResolved" } : undefined;
    }
    default:
      return undefined;
  }
}

type DevboxHooks = { event?: (input: { event: unknown }) => Promise<void> };

export default {
  id: "devbox",
  async server(): Promise<DevboxHooks> {
    const bin = process.env.DEVBOX_BIN;
    // Outside a devbox task there is nothing to report to.
    if (!bin || !process.env.DEVBOX_AGENT_ID) return {};
    const state = newState();
    // Reports go one at a time, so devbox receives them in opencode's order.
    let queue = Promise.resolve();
    const report = (payload: DevboxPayload) =>
      new Promise<void>((done) => {
        try {
          const child = spawn(bin, ["agent", "event", "--from", "opencode", JSON.stringify(payload)], {
            stdio: "ignore",
          });
          const guard = setTimeout(done, REPORT_WAIT_MS);
          if (typeof guard === "object") guard.unref();
          const finish = () => {
            clearTimeout(guard);
            done();
          };
          child.on("error", finish);
          child.on("exit", finish);
          child.unref();
        } catch {
          done();
        }
      });
    return {
      async event({ event }) {
        try {
          const payload = toDevboxEvent(event, state);
          if (payload) queue = queue.then(() => report(payload));
        } catch {
          // Reporting is best-effort: opencode never sees an error from it.
        }
      },
    };
  },
};
