// @vitest-environment node
import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import plugin, { newState, toDevboxEvent } from "../../../../../crates/agents/assets/opencode-plugin";

const spawn = vi.hoisted(() => vi.fn());
vi.mock("node:child_process", () => ({ spawn, default: { spawn } }));

const RECORDING = "../../../../../crates/agents/tests/fixtures/opencode-1.19.3/events.jsonl";
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

/** Each `# run N:` line of the recording starts the events one opencode process received. */
function recordedRuns(): unknown[][] {
  const text = fixture(RECORDING);
  const runs: unknown[][] = [];
  for (const line of text.split("\n")) {
    if (/^# run \d+:/.test(line)) runs.push([]);
    else if (line.trim() && !line.startsWith("#")) runs.at(-1)?.push(JSON.parse(line));
  }
  return runs;
}
const payloads = (events: unknown[], state = newState()) => events.flatMap((e) => toDevboxEvent(e, state) ?? []);
const status = (sessionID: string, type: string) => ({
  type: "session.status",
  properties: { sessionID, status: { type } },
});
const created = (id: string, parentID?: string) => ({
  type: "session.created",
  properties: { sessionID: id, info: parentID ? { id, parentID } : { id } },
});
const asked = (id: string, sessionID: string, permission: string) => ({
  type: "permission.asked",
  properties: { id, sessionID, permission, patterns: [`secret ${id}`], always: ["*"], metadata: {} },
});
const replied = (requestID: string, sessionID: string) => ({
  type: "permission.replied",
  properties: { sessionID, requestID, reply: "once" },
});
const question = (id: string, sessionID: string, text?: string) => ({
  type: "question.asked",
  properties: {
    id,
    sessionID,
    questions: text === undefined ? [] : [{ question: text, header: "Fixture", options: [] }],
  },
});
const answered = (requestID: string, sessionID: string, type = "question.replied") => ({
  type,
  properties: { sessionID, requestID, answers: [["Alpha"]] },
});
const permission = (message: string) => ({ v: 1, event: "needsInput", kind: "permission", message });

test("recorded opencode events become devbox reports in order", () => {
  expect(recordedRuns().map((run) => payloads(run))).toEqual([
    [
      { v: 1, event: "working", sessionId: "ses_errorturn" },
      { v: 1, event: "idle" },
    ],
    [
      { v: 1, event: "working", sessionId: "ses_replyok" },
      { v: 1, event: "idle" },
    ],
    [
      { v: 1, event: "working", sessionId: "ses_taskroot" },
      { v: 1, event: "idle" },
    ],
    [
      { v: 1, event: "working", sessionId: "ses_bashroot" },
      permission("bash"),
      { v: 1, event: "inputResolved" },
      { v: 1, event: "idle" },
    ],
    [
      { v: 1, event: "working", sessionId: "ses_questionroot" },
      { v: 1, event: "needsInput", kind: "question", message: "Which option should the fixture use?" },
      { v: 1, event: "inputResolved" },
      { v: 1, event: "idle" },
    ],
  ]);
});

test("child_session_events_are_ignored", () => {
  const subagent = recordedRuns()[2] ?? [];
  expect(JSON.stringify(payloads(subagent))).not.toContain("ses_taskchild");
  // A child known only from an update is a child too, and its retries are not the task's.
  const updated = { ...created("ses_child", "ses_root"), type: "session.updated" };
  expect(
    payloads([
      updated,
      status("ses_child", "busy"),
      status("ses_child", "retry"),
      status("ses_child", "idle"),
      status("ses_root", "retry"),
      status("ses_root", "busy"),
      status("ses_root", "idle"),
    ]),
  ).toEqual([
    { v: 1, event: "working", sessionId: "ses_root" },
    { v: 1, event: "idle" },
  ]);
});

test("a permission asked in a child session waits for input", () => {
  const state = newState();
  payloads([status("ses_root", "busy"), created("ses_child", "ses_root")], state);
  expect(toDevboxEvent(asked("per_child", "ses_child", "edit"), state)).toEqual(permission("edit"));
});

test("only the reply that leaves no request open resolves the wait", () => {
  const state = newState();
  payloads([status("ses_root", "busy"), created("ses_child", "ses_root")], state);
  expect(
    [
      asked("per_root", "ses_root", "bash"),
      asked("per_child", "ses_child", "edit"),
      asked("per_root", "ses_root", "bash"),
      replied("per_root", "ses_root"),
      replied("per_child", "ses_child"),
    ].map((event) => toDevboxEvent(event, state)),
  ).toEqual([permission("bash"), permission("edit"), undefined, permission("edit"), { v: 1, event: "inputResolved" }]);
});

test("a reply for a request the plugin never saw changes nothing", () => {
  const state = newState();
  expect(toDevboxEvent(replied("per_unknown", "ses_root"), state)).toBeUndefined();
  toDevboxEvent(asked("per_open", "ses_root", "bash"), state);
  expect(toDevboxEvent(replied("per_unknown", "ses_root"), state)).toBeUndefined();
  expect(toDevboxEvent(replied("per_open", "ses_root"), state)).toEqual({ v: 1, event: "inputResolved" });
});

test("requests a session drops when it goes idle no longer hold the wait", () => {
  // opencode forgets an interrupted request without permission.replied.
  const state = newState();
  payloads([status("ses_root", "busy"), created("ses_child", "ses_root"), status("ses_child", "busy")], state);
  expect(
    [
      asked("per_child", "ses_child", "edit"),
      status("ses_child", "idle"),
      asked("per_root", "ses_root", "bash"),
      status("ses_root", "idle"),
      status("ses_root", "busy"),
      asked("per_next", "ses_root", "bash"),
      replied("per_next", "ses_root"),
    ].map((event) => toDevboxEvent(event, state)),
  ).toEqual([
    permission("edit"),
    { v: 1, event: "inputResolved" },
    permission("bash"),
    { v: 1, event: "idle" },
    { v: 1, event: "working", sessionId: "ses_root" },
    permission("bash"),
    { v: 1, event: "inputResolved" },
  ]);
});

test("a question waits for input with its first question", () => {
  const state = newState();
  payloads([status("ses_root", "busy")], state);
  expect(toDevboxEvent(question("que_1", "ses_root", "Which option?"), state)).toEqual({
    v: 1,
    event: "needsInput",
    kind: "question",
    message: "Which option?",
  });
  expect(toDevboxEvent(question("que_2", "ses_root"), state)).toEqual({ v: 1, event: "needsInput", kind: "question" });
  expect(toDevboxEvent(question("que_1", "ses_root", "Which option?"), state)).toBeUndefined();
});

test("a question and a permission open together resolve only when both are answered", () => {
  // question.replied was recorded (run 6). question.rejected is 타입 기반: built from the 1.19.3
  // types ({sessionID, requestID}), not recorded.
  for (const ending of ["question.replied", "question.rejected"]) {
    const state = newState();
    payloads([status("ses_root", "busy"), created("ses_child", "ses_root")], state);
    expect(
      [
        asked("per_1", "ses_root", "bash"),
        question("que_1", "ses_child", "Which option?"),
        replied("per_1", "ses_root"),
        answered("que_1", "ses_child", ending),
      ].map((event) => toDevboxEvent(event, state)),
    ).toEqual([
      permission("bash"),
      { v: 1, event: "needsInput", kind: "question", message: "Which option?" },
      { v: 1, event: "needsInput", kind: "question", message: "Which option?" },
      { v: 1, event: "inputResolved" },
    ]);
  }
  // A question's id never answers a permission, or the other way round.
  const state = newState();
  toDevboxEvent(asked("same", "ses_root", "bash"), state);
  expect(toDevboxEvent(answered("same", "ses_root"), state)).toBeUndefined();
  expect(toDevboxEvent(replied("same", "ses_root"), state)).toEqual({ v: 1, event: "inputResolved" });
});

test("an answer that leaves requests open shows the latest one still asked", () => {
  const state = newState();
  payloads([status("ses_root", "busy"), created("ses_child", "ses_root")], state);
  const which = { v: 1, event: "needsInput", kind: "question", message: "Which option?" };
  expect(
    [
      asked("per_1", "ses_root", "bash"),
      question("que_1", "ses_child", "Which option?"),
      asked("per_2", "ses_root", "edit"),
      replied("per_2", "ses_root"),
      answered("que_1", "ses_child"),
      replied("per_1", "ses_root"),
    ].map((event) => toDevboxEvent(event, state)),
  ).toEqual([
    permission("bash"),
    which,
    permission("edit"),
    which,
    permission("bash"),
    { v: 1, event: "inputResolved" },
  ]);
  // The latest asked, whichever was answered: here the one already shown, sent again unchanged.
  expect(
    [
      asked("per_3", "ses_root", "bash"),
      question("que_2", "ses_root"),
      replied("per_3", "ses_root"),
      answered("que_2", "ses_root"),
    ].map((event) => toDevboxEvent(event, state)),
  ).toEqual([
    permission("bash"),
    { v: 1, event: "needsInput", kind: "question" },
    { v: 1, event: "needsInput", kind: "question" },
    { v: 1, event: "inputResolved" },
  ]);
});

test("a session that goes idle drops its open questions and permissions", () => {
  // 타입 기반, not recorded: in 1.19.3 an interrupted question or permission is deleted with no
  // event, so only its session's idle arrives.
  const state = newState();
  payloads([status("ses_root", "busy"), created("ses_child", "ses_root"), status("ses_child", "busy")], state);
  expect(
    [
      question("que_1", "ses_child", "Which option?"),
      asked("per_1", "ses_child", "edit"),
      status("ses_child", "idle"),
      question("que_2", "ses_root", "Proceed?"),
      status("ses_root", "idle"),
    ].map((event) => toDevboxEvent(event, state)),
  ).toEqual([
    { v: 1, event: "needsInput", kind: "question", message: "Which option?" },
    permission("edit"),
    { v: 1, event: "inputResolved" },
    { v: 1, event: "needsInput", kind: "question", message: "Proceed?" },
    { v: 1, event: "idle" },
  ]);
  expect(toDevboxEvent(answered("que_2", "ses_root"), state)).toBeUndefined();
});

test("messages are cut at 200 code points, never inside an emoji", () => {
  const text = `${"가".repeat(199)}\u{1F600} and more`;
  const report = toDevboxEvent(question("que_1", "ses_root", text), newState());
  // A cut through the emoji's UTF-16 pair would send a lone surrogate the daemon rejects.
  expect(JSON.stringify(report)).toBe(fixture(EMOJI_CUT).trimEnd());
  const long = toDevboxEvent(asked("per_1", "ses_root", "x".repeat(250)), newState());
  expect(long).toEqual(permission("x".repeat(200)));
});

test("a session learned to be a child after it turned busy never ends the turn", () => {
  // Its parent arrives late: until then it looked like a root of its own.
  expect(
    payloads([
      status("ses_child", "busy"),
      created("ses_child", "ses_root"),
      status("ses_root", "busy"),
      status("ses_child", "idle"),
      status("ses_root", "idle"),
    ]),
  ).toEqual([
    { v: 1, event: "working", sessionId: "ses_child" },
    { v: 1, event: "working", sessionId: "ses_root" },
    { v: 1, event: "idle" },
  ]);
});

test("an unknown session that turns busy while the root works is a child", () => {
  // Its session.created comes after its first status, or not at all.
  expect(
    payloads([
      created("ses_root"),
      status("ses_root", "busy"),
      status("ses_child", "busy"),
      status("ses_child", "idle"),
      created("ses_child", "ses_root"),
      status("ses_child", "busy"),
      status("ses_child", "idle"),
      status("ses_root", "idle"),
    ]),
  ).toEqual([
    { v: 1, event: "working", sessionId: "ses_root" },
    { v: 1, event: "idle" },
  ]);
  // A root the plugin already knows is still the task's, even while another one works.
  expect(payloads([created("ses_a"), created("ses_b"), status("ses_a", "busy"), status("ses_b", "busy")])).toEqual([
    { v: 1, event: "working", sessionId: "ses_a" },
    { v: 1, event: "working", sessionId: "ses_b" },
  ]);
});

test("reports carry the permission name, never its patterns", () => {
  const sent = JSON.stringify(recordedRuns().flatMap((run) => payloads(run)));
  expect(sent).not.toContain("echo devbox-fixture");
  expect(JSON.stringify(toDevboxEvent(asked("per_1", "ses_root", "bash"), newState()))).not.toContain("secret");
});

test("garbage events are ignored", () => {
  const state = newState();
  for (const event of [
    null,
    "session.status",
    7,
    {},
    { type: "session.status" },
    status("", "busy"),
    asked("", "s", "x"),
  ]) {
    expect(toDevboxEvent(event, state)).toBeUndefined();
  }
});

test("plugin_is_inert_without_devbox_agent_id", async () => {
  for (const env of [{ DEVBOX_BIN: "/owned/devbox" }, { DEVBOX_BIN: "/owned/devbox", DEVBOX_AGENT_ID: "" }, {}]) {
    for (const key of ENV) delete process.env[key];
    Object.assign(process.env, env);
    const hooks = await plugin.server();
    expect(hooks).toEqual({});
  }
  expect(spawn).not.toHaveBeenCalled();
});

/** A spawned `devbox agent event` that ends when the test says so. */
function child() {
  return Object.assign(new EventEmitter(), { unref: vi.fn() });
}

test("reports go to devbox one at a time, in order", async () => {
  Object.assign(process.env, { DEVBOX_AGENT_ID: "a1", DEVBOX_BIN: "/owned/devbox" });
  const first = child();
  const second = child();
  spawn.mockReturnValueOnce(first).mockReturnValueOnce(second);
  const hooks = await plugin.server();
  await hooks.event?.({ event: status("ses_root", "busy") });
  await hooks.event?.({ event: asked("per_1", "ses_root", "bash") });
  await vi.waitFor(() => expect(spawn).toHaveBeenCalledTimes(1));
  expect(spawn).toHaveBeenCalledWith(
    "/owned/devbox",
    ["agent", "event", "--from", "opencode", JSON.stringify({ v: 1, event: "working", sessionId: "ses_root" })],
    { stdio: "ignore" },
  );
  expect(first.unref).toHaveBeenCalled();
  first.emit("exit", 0);
  await vi.waitFor(() => expect(spawn).toHaveBeenCalledTimes(2));
  expect(spawn.mock.calls[1]?.[1]).toEqual([
    "agent",
    "event",
    "--from",
    "opencode",
    JSON.stringify(permission("bash")),
  ]);
});

test("spawn_errors_are_swallowed", async () => {
  Object.assign(process.env, { DEVBOX_AGENT_ID: "a1", DEVBOX_BIN: "/owned/missing-devbox" });
  const failing = child();
  spawn
    .mockImplementationOnce(() => {
      throw new Error("spawn EACCES");
    })
    .mockReturnValueOnce(failing)
    .mockReturnValueOnce(child());
  const hooks = await plugin.server();
  await expect(hooks.event?.({ event: status("ses_root", "busy") })).resolves.toBeUndefined();
  await expect(hooks.event?.({ event: asked("per_1", "ses_root", "bash") })).resolves.toBeUndefined();
  await vi.waitFor(() => expect(spawn).toHaveBeenCalledTimes(2));
  expect(() => failing.emit("error", new Error("spawn ENOENT"))).not.toThrow();
  await expect(hooks.event?.({ event: replied("per_1", "ses_root") })).resolves.toBeUndefined();
  await vi.waitFor(() => expect(spawn).toHaveBeenCalledTimes(3));
});
