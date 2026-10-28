// @vitest-environment node
import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import plugin, { newState, toDevboxEvent } from "../../../../../crates/agents/assets/opencode-plugin";

const spawn = vi.hoisted(() => vi.fn());
vi.mock("node:child_process", () => ({ spawn, default: { spawn } }));

const RECORDING = "../../../../../crates/agents/tests/fixtures/opencode-1.19.3/events.jsonl";
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
  const text = readFileSync(fileURLToPath(new URL(RECORDING, import.meta.url)), "utf8");
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
      { v: 1, event: "needsInput", message: "bash" },
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
  expect(toDevboxEvent(asked("per_child", "ses_child", "edit"), state)).toEqual({
    v: 1,
    event: "needsInput",
    message: "edit",
  });
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
  ).toEqual([
    { v: 1, event: "needsInput", message: "bash" },
    { v: 1, event: "needsInput", message: "edit" },
    undefined,
    undefined,
    { v: 1, event: "inputResolved" },
  ]);
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
    { v: 1, event: "needsInput", message: "edit" },
    { v: 1, event: "inputResolved" },
    { v: 1, event: "needsInput", message: "bash" },
    { v: 1, event: "idle" },
    { v: 1, event: "working", sessionId: "ses_root" },
    { v: 1, event: "needsInput", message: "bash" },
    { v: 1, event: "inputResolved" },
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
    JSON.stringify({ v: 1, event: "needsInput", message: "bash" }),
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
