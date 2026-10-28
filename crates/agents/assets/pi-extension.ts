// devbox pi extension. devbox writes this file and loads it only in its own tasks, with
// `pi -e <file>`, which pi loads next to the user's own extensions. It reports the task's state
// to devbox with `devbox agent event --from pi <report>`, imports nothing but
// node:child_process, and never throws into pi. It decides nothing for pi: the user answers
// pi's questions in the terminal.
import { spawn } from "node:child_process";

/** One report: the last argument of `devbox agent event --from pi`. */
export type DevboxPayload = {
  v: 1;
  event: "working" | "idle" | "needsInput" | "inputResolved";
  /** What a `needsInput` waits on. The trust question has none. */
  kind?: InputKind;
  sessionId?: string;
  message?: string;
};

/** A dialog that asks yes or no is an approval; any other dialog asks a question. */
type InputKind = "permission" | "question";

/** Something that stops the task until the user answers it in the terminal. */
type Wait = { kind?: InputKind; message?: string };

/** What the extension reads from a handler's context. */
export type EventContext = { sessionId?: string; hasUI?: boolean };

/** What the extension remembers. pi loads it again for each session it switches to. */
export type ExtensionState = {
  /** pi's folder trust question while it is asked, and the extension dialog that is up. */
  open: Map<"trust" | "dialog", Wait>;
  /** No run is going, so a wait that ends leaves pi idle. */
  settled: boolean;
};

type Fields = Record<string, unknown>;

/** How long one report may take before the next is sent anyway. `devbox` gives up after 3 s. */
const REPORT_WAIT_MS = 5000;
/** How many code points of a dialog's title a report carries. */
const MESSAGE_CODE_POINTS = 200;
/** The events the extension reports from. */
const EVENTS = ["session_start", "agent_start", "agent_settled", "ui_prompt_start", "ui_prompt_end"];

export function newState(): ExtensionState {
  return { open: new Map(), settled: false };
}

function fields(value: unknown): Fields | undefined {
  return typeof value === "object" && value !== null ? (value as Fields) : undefined;
}

function text(value: unknown): string | undefined {
  return typeof value === "string" && value !== "" ? value : undefined;
}

/** The first line of a dialog's title, cut between code points: half an emoji would make the
 * report unreadable. A permission gate can put the command below that line. */
function titleLine(title: unknown): string | undefined {
  const line = text(title)?.split(/\r?\n/, 1)[0]?.trim();
  return line ? Array.from(line).slice(0, MESSAGE_CODE_POINTS).join("") : undefined;
}

function waiting({ kind, message }: Wait): DevboxPayload {
  const payload: DevboxPayload = { v: 1, event: "needsInput" };
  if (kind) payload.kind = kind;
  if (message) payload.message = message;
  return payload;
}

/** A wait has ended: the latest one still open shows, or else pi goes on, or is idle again. */
function close(state: ExtensionState, key: "trust" | "dialog"): DevboxPayload | undefined {
  if (!state.open.delete(key)) return undefined;
  const latest = [...state.open.values()].pop();
  if (latest) return waiting(latest);
  return state.settled ? { v: 1, event: "idle" } : { v: 1, event: "inputResolved" };
}

/** The report one pi event makes, if any. */
export function toDevboxEvent(event: unknown, context: EventContext, state: ExtensionState): DevboxPayload | undefined {
  const fired = fields(event);
  switch (fired?.type) {
    case "project_trust": {
      // Without a UI pi decides alone, as in print mode or for --help: nobody is asked.
      if (fields(context)?.hasUI !== true) return undefined;
      const trust: Wait = { message: "project_trust" };
      state.open.set("trust", trust);
      return waiting(trust);
    }
    case "session_start":
      // pi starts its session once the user has decided about trust. After a switch (/new,
      // /resume, /fork) or a reload, no run is going.
      state.settled = fired.reason !== "startup";
      return close(state, "trust");
    case "agent_start": {
      // A run starts only once trust is decided. A dialog still up keeps the task waiting.
      state.settled = false;
      state.open.delete("trust");
      const dialog = state.open.get("dialog");
      const payload: DevboxPayload = dialog ? waiting(dialog) : { v: 1, event: "working" };
      const sessionId = text(fields(context)?.sessionId);
      return sessionId ? { ...payload, sessionId } : payload;
    }
    case "agent_settled":
      // pi will not go on by itself; a dialog still up keeps the task waiting until it ends.
      state.settled = true;
      return state.open.size > 0 ? undefined : { v: 1, event: "idle" };
    case "ui_prompt_start": {
      // pi reports only the outermost of nested dialogs, and ends it when none is left.
      const kind = text(fired.kind);
      if (!kind) return undefined;
      const message = titleLine(fired.title);
      const dialog: Wait = { kind: kind === "confirm" ? "permission" : "question" };
      if (message) dialog.message = message;
      state.open.delete("dialog");
      state.open.set("dialog", dialog);
      return waiting(dialog);
    }
    case "ui_prompt_end":
      return close(state, "dialog");
    default:
      return undefined;
  }
}

/** What the extension needs from a handler's context. A context pi broke reads as empty. */
function contextOf(ctx: unknown): EventContext {
  const read: EventContext = { hasUI: fields(ctx)?.hasUI === true };
  try {
    const manager = fields(fields(ctx)?.sessionManager);
    const getSessionId = manager?.getSessionId;
    const sessionId = typeof getSessionId === "function" ? text(getSessionId.call(manager)) : undefined;
    if (sessionId) read.sessionId = sessionId;
  } catch {
    // The run is reported without its session.
  }
  return read;
}

type Handler = (event: unknown, ctx: unknown) => unknown;

export default function devbox(pi: unknown): void {
  try {
    const bin = process.env.DEVBOX_BIN;
    // Outside a devbox task there is nothing to report to, so nothing is registered.
    if (!bin || !process.env.DEVBOX_AGENT_ID) return;
    const on = fields(pi)?.on;
    if (typeof on !== "function") return;
    const listen = (name: string, handler: Handler) => {
      try {
        on.call(pi, name, handler);
      } catch {
        // An event this pi does not have is simply not reported.
      }
    };
    const state = newState();
    // Reports go one at a time, so devbox receives them in pi's order.
    let queue = Promise.resolve();
    const report = (payload: DevboxPayload) =>
      new Promise<void>((done) => {
        try {
          const child = spawn(bin, ["agent", "event", "--from", "pi", JSON.stringify(payload)], {
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
    // The queue up to and including this event's report, when it makes one.
    const handle = (event: unknown, ctx: unknown): Promise<void> | undefined => {
      try {
        const payload = toDevboxEvent(event, contextOf(ctx), state);
        if (!payload) return undefined;
        queue = queue.then(() => report(payload));
        return queue;
      } catch {
        // Reporting is best-effort: pi never sees an error from it.
        return undefined;
      }
    };
    for (const name of EVENTS) {
      listen(name, (event, ctx) => {
        void handle(event, ctx);
      });
    }
    // pi shows its trust question after this returns, so devbox has the wait before any
    // instruction could reach the question. The answer is always the user's.
    listen("project_trust", async (event, ctx) => {
      // pi is not reading the terminal yet, and the report's process does not hold pi open:
      // without this timer pi would exit while it waits.
      const hold = setTimeout(() => undefined, REPORT_WAIT_MS);
      try {
        await handle(event, ctx);
      } finally {
        clearTimeout(hold);
      }
      return { trusted: "undecided" };
    });
  } catch {
    // Loading is best-effort as well.
  }
}
