// @vitest-environment node
import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import devbox, {
  type EventContext,
  newState,
  toDevboxEvent,
  toDevboxEvents,
} from "../../../../../crates/agents/assets/pi-extension";

const spawn = vi.hoisted(() => vi.fn());
vi.mock("node:child_process", () => ({ spawn, default: { spawn } }));

const RECORDING = "../../../../../crates/agents/tests/fixtures/pi-1.3.0/events.jsonl";
/** The report `parse_plugin_event` is tested with, in Rust, for a cut at an emoji. */
const EMOJI_CUT = "../../../../../crates/agents/tests/fixtures/plugin-report-emoji-cut.json";
const fixture = (path: string) => readFileSync(fileURLToPath(new URL(path, import.meta.url)), "utf8");
const ENV = ["DEVBOX_AGENT_ID", "DEVBOX_BIN"] as const;
let saved: Record<string, string | undefined> = {};
beforeEach(() => {
  saved = Object.fromEntries(ENV.map((key) => [key, process.env[key]]));
  spawn.mockReset();
});
afterEach(() => {
  for (const key of ENV) {
    if (saved[key] === undefined) delete process.env[key];
    else process.env[key] = saved[key];
  }
});

/** One recorded line: the event a handler got, and what the handler read from its context. */
type Recorded = { event: unknown; context: EventContext };
/** Each `# run N:` line of the recording starts the events one pi process received. */
function recordedRuns(): Recorded[][] {
  const runs: Recorded[][] = [];
  for (const line of fixture(RECORDING).split("\n")) {
    if (/^# run \d+:/.test(line)) runs.push([]);
    else if (line.trim() && !line.startsWith("#")) runs.at(-1)?.push(JSON.parse(line));
  }
  return runs;
}
const replay = (lines: Recorded[], state = newState()) =>
  lines.flatMap(({ event, context }) => toDevboxEvents(event, context, state));

const session = (n: number) => `00000000-0000-4000-8000-00000000000${n}`;
const trust = { v: 1, event: "needsInput", message: "project_trust" };
const resolved = { v: 1, event: "inputResolved" };
const idle = { v: 1, event: "idle" };
const ready = { v: 1, event: "ready" };
const working = (sessionId: string) => ({ v: 1, event: "working", sessionId });
const permission = (message: string) => ({ v: 1, event: "needsInput", kind: "permission", message });
const question = (message: string) => ({ v: 1, event: "needsInput", kind: "question", message });
const asked = { type: "project_trust", cwd: "/w" };
const startup = { type: "session_start", reason: "startup" };
const started = { type: "agent_start" };
const settled = (aborted = false) => ({ type: "agent_settled", aborted });
const dialog = (type: "ui_prompt_start" | "ui_prompt_end", kind: string, title?: string) => ({
  type,
  reason: "ui_prompt",
  kind,
  ...(title === undefined ? {} : { title }),
});
const ui: EventContext = { hasUI: true };
const run: EventContext = { sessionId: session(1), hasUI: true };

test("recorded pi events become devbox reports in order", () => {
  expect(recordedRuns().map((events) => replay(events))).toEqual([
    [trust, resolved, ready, working(session(1)), idle],
    [trust, resolved, ready, working(session(1)), idle],
    [trust, resolved, ready, working(session(3)), permission("Allow the fixture command?"), resolved, idle],
    // After /new a turn names pi's new session. pi was ready already.
    [trust, resolved, ready, working(session(4)), idle, working("00000000-0000-7000-8000-000000000005"), idle],
  ]);
});

test("agent_start and agent_settled become working and idle", () => {
  const state = newState();
  expect(toDevboxEvent(started, run, state)).toEqual(working(session(1)));
  expect(toDevboxEvent(settled(), run, state)).toEqual(idle);
  // A run stopped with Escape settles as well (타입 기반: `aborted` from the 1.3.0 types, not recorded).
  expect(toDevboxEvent(started, run, state)).toEqual(working(session(1)));
  expect(toDevboxEvent(settled(true), run, state)).toEqual(idle);
  // Without a session from pi, the run is still reported.
  expect(toDevboxEvent(started, ui, newState())).toEqual({ v: 1, event: "working" });
  // Nothing else is sent.
  for (const type of [
    "session_start",
    "before_agent_start",
    "agent_end",
    "turn_start",
    "turn_end",
    "message_start",
    "message_end",
    "tool_call",
    "tool_execution_start",
    "tool_execution_end",
    "session_shutdown",
    "input",
  ]) {
    expect(toDevboxEvent({ type, reason: "quit" }, run, state)).toBeUndefined();
  }
});

test("project_trust becomes needsInput", () => {
  expect(toDevboxEvent(asked, { hasUI: true }, newState())).toEqual(trust);
  // Without a UI pi decides alone (print mode, --help): nobody is asked (실제 실행 L7, 미커밋).
  expect(toDevboxEvent(asked, { hasUI: false }, newState())).toBeUndefined();
  expect(toDevboxEvent(asked, {}, newState())).toBeUndefined();
});

test("the trust decision ends the wait", () => {
  // pi starts its session once the user has decided.
  const state = newState();
  expect(toDevboxEvent(asked, ui, state)).toEqual(trust);
  expect(toDevboxEvent(startup, ui, state)).toEqual(resolved);
  expect(toDevboxEvent(started, run, state)).toEqual(working(session(1)));
  // A run that comes first ends it as well, and the session start after it adds nothing.
  const early = newState();
  toDevboxEvent(asked, ui, early);
  expect(toDevboxEvent(started, run, early)).toEqual(working(session(1)));
  expect(toDevboxEvent(startup, run, early)).toBeUndefined();
  expect(toDevboxEvent(settled(), run, early)).toEqual(idle);
});

test("pi says it is ready once it has started, whether or not it asked about trust", () => {
  const asking = newState();
  expect(toDevboxEvents(asked, ui, asking)).toEqual([trust]);
  expect(toDevboxEvents(startup, ui, asking)).toEqual([resolved, ready]);
  // The folder's trust was decided before: pi asks nobody and starts.
  expect(toDevboxEvents(startup, ui, newState())).toEqual([ready]);
  // A switch to another session or a reload is not a start: pi has been reading its input.
  for (const reason of ["new", "resume", "fork", "reload"]) {
    expect(toDevboxEvents({ type: "session_start", reason }, run, newState())).toEqual([]);
  }
  // Every other event makes the one report toDevboxEvent makes, if any.
  const state = newState();
  expect(toDevboxEvents(started, run, state)).toEqual([working(session(1))]);
  expect(toDevboxEvents(settled(), run, state)).toEqual([idle]);
  expect(toDevboxEvents({ type: "turn_start" }, run, state)).toEqual([]);
  expect(toDevboxEvents(null, run, state)).toEqual([]);
});

test("a dialog waits with its kind and the first line of its title", () => {
  const state = newState();
  toDevboxEvent(started, run, state);
  expect(toDevboxEvent(dialog("ui_prompt_start", "confirm", "Allow the fixture command?"), run, state)).toEqual(
    permission("Allow the fixture command?"),
  );
  expect(toDevboxEvent(dialog("ui_prompt_end", "confirm", "Allow the fixture command?"), run, state)).toEqual(resolved);
  // The confirm above was recorded (run 3). The other kinds and the titles below are 타입 기반:
  // built from UIPromptKind in the 1.3.0 types and `withUIPrompt` in its runner, not recorded.
  // A permission gate can put the command below its first line.
  const gate = "⚠️ Dangerous command:\n\n  rm -rf build\n\nAllow?";
  for (const kind of ["select", "input", "editor"]) {
    expect(toDevboxEvent(dialog("ui_prompt_start", kind, gate), run, state)).toEqual(question("⚠️ Dangerous command:"));
    expect(toDevboxEvent(dialog("ui_prompt_end", kind), run, state)).toEqual(resolved);
  }
  // A custom dialog has no title, and a blank first line shows nothing.
  for (const [kind, title] of [
    ["custom", undefined],
    ["input", "  \nsecond line"],
  ] as const) {
    expect(toDevboxEvent(dialog("ui_prompt_start", kind, title), run, state)).toEqual({
      v: 1,
      event: "needsInput",
      kind: "question",
    });
    expect(toDevboxEvent(dialog("ui_prompt_end", kind), run, state)).toEqual(resolved);
  }
  // An end without a start changes nothing.
  expect(toDevboxEvent(dialog("ui_prompt_end", "confirm"), run, state)).toBeUndefined();
});

test("a dialog that outlasts its run ends idle", () => {
  // 타입 기반, not recorded: in the recording every dialog ended inside its run.
  const state = newState();
  toDevboxEvent(started, run, state);
  toDevboxEvent(dialog("ui_prompt_start", "confirm", "Proceed?"), run, state);
  // The run settles while the dialog is still up: the task keeps waiting.
  expect(toDevboxEvent(settled(), run, state)).toBeUndefined();
  expect(toDevboxEvent(dialog("ui_prompt_end", "confirm", "Proceed?"), run, state)).toEqual(idle);
  // A command's dialog while pi is idle.
  expect(toDevboxEvent(dialog("ui_prompt_start", "select", "Restore code state?"), run, state)).toEqual(
    question("Restore code state?"),
  );
  expect(toDevboxEvent(dialog("ui_prompt_end", "select"), run, state)).toEqual(idle);
  // pi is idle after it switches sessions too.
  const switched = newState();
  toDevboxEvent({ type: "session_start", reason: "new" }, run, switched);
  toDevboxEvent(dialog("ui_prompt_start", "confirm", "Clear?"), run, switched);
  expect(toDevboxEvent(dialog("ui_prompt_end", "confirm"), run, switched)).toEqual(idle);
});

test("a run that starts under a dialog keeps the wait and names its session", () => {
  // 타입 기반, not recorded: an extension can open a dialog from session_start before a run.
  const state = newState();
  toDevboxEvent(startup, ui, state);
  toDevboxEvent(dialog("ui_prompt_start", "confirm", "Proceed?"), ui, state);
  expect(toDevboxEvent(started, run, state)).toEqual({ ...permission("Proceed?"), sessionId: session(1) });
  expect(toDevboxEvent(dialog("ui_prompt_end", "confirm"), run, state)).toEqual(resolved);
});

test("titles are cut at 200 code points, never inside an emoji", () => {
  const title = `${"가".repeat(199)}\u{1F600} and more`;
  const report = toDevboxEvent(dialog("ui_prompt_start", "select", title), run, newState());
  // A cut through the emoji's UTF-16 pair would send a lone surrogate the daemon rejects.
  expect(JSON.stringify(report)).toBe(fixture(EMOJI_CUT).trimEnd());
  const long = toDevboxEvent(dialog("ui_prompt_start", "confirm", "x".repeat(250)), run, newState());
  expect(long).toEqual(permission("x".repeat(200)));
});

test("garbage events are ignored", () => {
  const state = newState();
  for (const event of [null, "agent_start", 7, {}, { type: 7 }, { type: "ui_prompt_start", kind: 3 }]) {
    expect(toDevboxEvent(event, run, state)).toBeUndefined();
  }
  expect(toDevboxEvent(started, { sessionId: "" }, state)).toEqual({ v: 1, event: "working" });
});

type Handler = (event: unknown, ctx: unknown) => unknown;
/** Loads the extension into a stand-in for pi, returning its handlers by event. */
function load(): Map<string, Handler> {
  const handlers = new Map<string, Handler>();
  devbox({ on: (name: string, handler: Handler) => handlers.set(name, handler) });
  return handlers;
}
const context = (id = session(1)) => ({ hasUI: true, sessionManager: { getSessionId: () => id } });

test("extension_is_inert_without_devbox_agent_id", () => {
  for (const env of [
    { DEVBOX_BIN: "/owned/devbox" },
    { DEVBOX_BIN: "/owned/devbox", DEVBOX_AGENT_ID: "" },
    { DEVBOX_AGENT_ID: "a1" },
    { DEVBOX_AGENT_ID: "a1", DEVBOX_BIN: "" },
    {},
  ]) {
    for (const key of ENV) delete process.env[key];
    Object.assign(process.env, env);
    const pi = { on: vi.fn() };
    expect(() => devbox(pi)).not.toThrow();
    expect(pi.on).not.toHaveBeenCalled();
  }
  expect(spawn).not.toHaveBeenCalled();
});

/** A spawned `devbox agent event` that ends when the test says so. */
function child() {
  return Object.assign(new EventEmitter(), { unref: vi.fn() });
}

test("reports go to devbox one at a time, in order", async () => {
  Object.assign(process.env, { DEVBOX_AGENT_ID: "a1", DEVBOX_BIN: "/owned/devbox" });
  const handlers = load();
  expect([...handlers.keys()].sort()).toEqual([
    "agent_settled",
    "agent_start",
    "project_trust",
    "session_start",
    "ui_prompt_end",
    "ui_prompt_start",
  ]);
  const first = child();
  const second = child();
  spawn.mockReturnValueOnce(first).mockReturnValueOnce(second);
  await handlers.get("agent_start")?.(started, context());
  await handlers.get("agent_settled")?.(settled(), context());
  await vi.waitFor(() => expect(spawn).toHaveBeenCalledTimes(1));
  expect(spawn).toHaveBeenCalledWith(
    "/owned/devbox",
    ["agent", "event", "--from", "pi", JSON.stringify(working(session(1)))],
    { stdio: "ignore" },
  );
  expect(first.unref).toHaveBeenCalled();
  first.emit("exit", 0);
  await vi.waitFor(() => expect(spawn).toHaveBeenCalledTimes(2));
  expect(spawn.mock.calls[1]?.[1]).toEqual(["agent", "event", "--from", "pi", JSON.stringify(idle)]);
});

test("pi shows its trust question only once devbox has the wait", async () => {
  Object.assign(process.env, { DEVBOX_AGENT_ID: "a1", DEVBOX_BIN: "/owned/devbox" });
  const handlers = load();
  const reporting = child();
  spawn.mockReturnValueOnce(reporting);
  let answered = false;
  const decision = Promise.resolve(
    handlers.get("project_trust")?.(asked, { cwd: "/w", mode: "tui", hasUI: true, ui: {} }),
  ).then((result) => {
    answered = true;
    return result;
  });
  await vi.waitFor(() => expect(spawn).toHaveBeenCalledTimes(1));
  expect(spawn.mock.calls[0]?.[1]).toEqual(["agent", "event", "--from", "pi", JSON.stringify(trust)]);
  await new Promise((done) => setTimeout(done, 20));
  expect(answered).toBe(false);
  reporting.emit("exit", 0);
  // The user decides in the terminal: the extension never does.
  await expect(decision).resolves.toEqual({ trusted: "undecided" });
  // Without a UI nobody is asked, and nothing waits (실제 실행 L7, 미커밋).
  await expect(Promise.resolve(handlers.get("project_trust")?.(asked, { hasUI: false }))).resolves.toEqual({
    trusted: "undecided",
  });
  expect(spawn).toHaveBeenCalledTimes(1);
});

test("pi stays running while devbox takes the trust report", async () => {
  // Found live: before its terminal UI starts pi has nothing else holding it open, and the
  // report's process is unref'd, so pi exited with code 0 while the question waited.
  Object.assign(process.env, { DEVBOX_AGENT_ID: "a1", DEVBOX_BIN: "/owned/devbox" });
  const handlers = load();
  const reporting = child();
  spawn.mockReturnValueOnce(reporting);
  const holding = () => process.getActiveResourcesInfo().filter((kind) => kind === "Timeout").length;
  const before = holding();
  const decision = Promise.resolve(handlers.get("project_trust")?.(asked, { hasUI: true }));
  await vi.waitFor(() => expect(spawn).toHaveBeenCalledTimes(1));
  expect(holding()).toBe(before + 1);
  reporting.emit("exit", 0);
  await expect(decision).resolves.toEqual({ trusted: "undecided" });
  expect(holding()).toBe(before);
});

test("pi is held open until the trust report has ended or been given up on", async () => {
  vi.useFakeTimers();
  try {
    Object.assign(process.env, { DEVBOX_AGENT_ID: "a1", DEVBOX_BIN: "/owned/devbox" });
    const handlers = load();
    // A report devbox never answers.
    spawn.mockReturnValueOnce(child());
    const decision = Promise.resolve(handlers.get("project_trust")?.(asked, { hasUI: true }));
    await vi.advanceTimersByTimeAsync(0);
    expect(spawn).toHaveBeenCalledTimes(1);
    // The hold, and the report's wait.
    expect(vi.getTimerCount()).toBe(2);
    // The report is given up on after 5 s, and pi is still held while that ends the wait.
    vi.advanceTimersByTime(5000);
    expect(vi.getTimerCount()).toBe(1);
    await expect(decision).resolves.toEqual({ trusted: "undecided" });
    expect(vi.getTimerCount()).toBe(0);
  } finally {
    vi.useRealTimers();
  }
});

test("spawn_errors_are_swallowed", async () => {
  Object.assign(process.env, { DEVBOX_AGENT_ID: "a1", DEVBOX_BIN: "/owned/missing-devbox" });
  const handlers = load();
  const failing = child();
  const saying = child();
  spawn
    .mockImplementationOnce(() => {
      throw new Error("spawn EACCES");
    })
    .mockReturnValueOnce(failing)
    .mockReturnValueOnce(saying)
    .mockReturnValueOnce(child());
  await expect(Promise.resolve(handlers.get("project_trust")?.(asked, { hasUI: true }))).resolves.toEqual({
    trusted: "undecided",
  });
  await expect(Promise.resolve(handlers.get("session_start")?.(startup, context()))).resolves.toBeUndefined();
  await vi.waitFor(() => expect(spawn).toHaveBeenCalledTimes(2));
  expect(() => failing.emit("error", new Error("spawn ENOENT"))).not.toThrow();
  // The report that pi is ready still follows.
  await vi.waitFor(() => expect(spawn).toHaveBeenCalledTimes(3));
  expect(spawn.mock.calls[2]?.[1]?.at(-1)).toBe(JSON.stringify(ready));
  saying.emit("exit", 0);
  // A context pi broke, or none at all, still reports the run.
  const broken = {
    hasUI: true,
    sessionManager: {
      getSessionId: () => {
        throw new Error("gone");
      },
    },
  };
  await expect(Promise.resolve(handlers.get("agent_start")?.(started, broken))).resolves.toBeUndefined();
  await vi.waitFor(() => expect(spawn).toHaveBeenCalledTimes(4));
  expect(spawn.mock.calls[3]?.[1]?.at(-1)).toBe(JSON.stringify({ v: 1, event: "working" }));
  // No handler throws into pi, whatever it is given.
  for (const [name, handler] of handlers) {
    const outcome = new Promise((done) => done(handler(null, null))).then(() => name);
    await expect(outcome).resolves.toBe(name);
  }
});
