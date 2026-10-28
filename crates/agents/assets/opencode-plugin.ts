// devbox opencode plugin. devbox writes this file and loads it only in its own tasks, through
// OPENCODE_CONFIG_CONTENT, next to the user's own plugins. It reports the task's session to
// devbox with `devbox agent event --from opencode <report>`, imports nothing but
// node:child_process, and never throws into opencode.
import { spawn } from "node:child_process";

/** One report: the last argument of `devbox agent event --from opencode`. */
export type DevboxPayload = {
  v: 1;
  event: "working" | "idle" | "needsInput" | "inputResolved";
  /** What a `needsInput` waits on. */
  kind?: InputKind;
  sessionId?: string;
  message?: string;
};

/** A permission prompt or a question dialog: either stops the task until the user answers. */
type InputKind = "permission" | "question";

/** An open request: the session that asked, and what its wait shows. */
type Request = { session: string; kind: InputKind; message?: string };

/** What the plugin remembers about the sessions of one opencode process. */
export type PluginState = {
  /** Sessions without a parent. */
  roots: Set<string>;
  /** Sessions with a parent: subagents, whose turns are not the task's. */
  children: Set<string>;
  /** Root sessions reported as working. */
  busy: Set<string>;
  /** Open permission and question requests by kind and id, in the order they were asked. */
  pending: Map<string, Request>;
};

type Fields = Record<string, unknown>;

/** How long one report may take before the next is sent anyway. `devbox` gives up after 3 s. */
const REPORT_WAIT_MS = 5000;
/** How many code points of a request's text a report carries. */
const MESSAGE_CODE_POINTS = 200;

export function newState(): PluginState {
  return { roots: new Set(), children: new Set(), busy: new Set(), pending: new Map() };
}

function fields(value: unknown): Fields | undefined {
  return typeof value === "object" && value !== null ? (value as Fields) : undefined;
}

function text(value: unknown): string | undefined {
  return typeof value === "string" && value !== "" ? value : undefined;
}

/** A request's text, cut between code points: half an emoji would make the report unreadable. */
function cut(message: string): string {
  return Array.from(message).slice(0, MESSAGE_CODE_POINTS).join("");
}

/** The wait a request shows: its kind, and its text when it has any. */
function waiting({ kind, message }: Request): DevboxPayload {
  return message ? { v: 1, event: "needsInput", kind, message } : { v: 1, event: "needsInput", kind };
}

/** A request that waits for the user, from any session: a subagent's stops the task as well. */
function asked(state: PluginState, kind: InputKind, properties: Fields, message?: string): DevboxPayload | undefined {
  const id = text(properties.id);
  const session = text(properties.sessionID);
  if (!id || !session || state.pending.has(`${kind}:${id}`)) return undefined;
  const request: Request = message ? { session, kind, message: cut(message) } : { session, kind };
  state.pending.set(`${kind}:${id}`, request);
  return waiting(request);
}

/** A request answered: the task goes on once none is left open. Until then the wait shows the
 * latest request still open, so its text is never one already answered. */
function answered(state: PluginState, kind: InputKind, properties: Fields): DevboxPayload | undefined {
  const id = text(properties.requestID);
  if (!id || !state.pending.delete(`${kind}:${id}`)) return undefined;
  const latest = [...state.pending.values()].pop();
  return latest ? waiting(latest) : { v: 1, event: "inputResolved" };
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
      if (!id) return undefined;
      if (text(info?.parentID)) {
        // A session taken for a root until its parent was known: its turns were never the task's.
        state.children.add(id);
        state.roots.delete(id);
        state.busy.delete(id);
      } else if (!state.children.has(id)) {
        state.roots.add(id);
      }
      return undefined;
    }
    case "session.status": {
      const session = text(properties.sessionID);
      const status = text(fields(properties.status)?.type);
      if (!session || !status) return undefined;
      if (status === "busy" || status === "retry") {
        if (state.children.has(session) || state.busy.has(session)) return undefined;
        // A session never announced that starts while a root works is that root's subagent.
        if (!state.roots.has(session) && state.busy.size > 0) {
          state.children.add(session);
          return undefined;
        }
        state.busy.add(session);
        return { v: 1, event: "working", sessionId: session };
      }
      if (status !== "idle") return undefined;
      // A session that stops drops its open requests without answering them.
      let dropped = false;
      for (const [key, request] of state.pending) {
        if (request.session === session) {
          state.pending.delete(key);
          dropped = true;
        }
      }
      if (state.busy.delete(session)) return { v: 1, event: "idle" };
      return dropped && state.pending.size === 0 ? { v: 1, event: "inputResolved" } : undefined;
    }
    case "permission.asked":
      // The permission's name only: its patterns can hold a command line or a path.
      return asked(state, "permission", properties, text(properties.permission));
    case "permission.replied":
      return answered(state, "permission", properties);
    case "question.asked": {
      const questions = Array.isArray(properties.questions) ? properties.questions : [];
      return asked(state, "question", properties, text(fields(questions[0])?.question));
    }
    case "question.replied":
    case "question.rejected":
      return answered(state, "question", properties);
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
