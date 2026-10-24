import type { ClientState } from "../../rpc/client";
import { RpcError } from "../../rpc/client";
import { type Drained, startActivityUplink, type UplinkClient } from "./uplink";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  label: "main",
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: mocks.label }) }));

const event = (seq: number) => ({
  seq,
  atMs: seq * 1000,
  kind: "foreground" as const,
  app: "code.exe",
  title: null,
  lastInputMs: seq * 1000,
});
const batch = (upto: number, dropped = 3): Drained => ({
  runId: "r1",
  events: Array.from({ length: upto }, (_, index) => event(index + 1)),
  dropped,
  upto,
});
const rules = { rev: "1", rules: { excludedProcesses: ["keepass.exe"] }, idleThresholdMs: 600_000 };

function client(state: ClientState = "ready") {
  const listeners = new Set<(state: ClientState) => void>();
  const topics = new Map<string, { onEvent: (rev: string, payload: unknown) => void; onResync: () => void }>();
  const fake = {
    state,
    topics,
    call: vi.fn(),
    subscribe: vi.fn((topic: string, onEvent: (rev: string, payload: unknown) => void, onResync: () => void) => {
      topics.set(topic, { onEvent, onResync });
      return () => topics.delete(topic);
    }),
    onState: vi.fn((cb: (state: ClientState) => void) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    }),
    set(next: ClientState) {
      fake.state = next;
      for (const cb of listeners) cb(next);
    },
  };
  return fake;
}
function drains(...batches: Drained[]) {
  mocks.invoke.mockImplementation(async (name: string) =>
    name === "activity_drain" ? (batches.length > 1 ? batches.shift() : batches[0]) : undefined,
  );
}
const calls = (name: string) => mocks.invoke.mock.calls.filter(([command]) => command === name);
const ingests = (c: ReturnType<typeof client>): unknown[][] =>
  c.call.mock.calls.filter(([method]) => method === "activity.ingest");
const tick = () => vi.advanceTimersByTimeAsync(1000);

beforeEach(() => {
  vi.useFakeTimers();
  mocks.label = "main";
  mocks.invoke.mockReset();
  mocks.listen.mockReset().mockResolvedValue(() => {});
  Object.assign(window, { __TAURI_INTERNALS__: {} });
});
afterEach(() => {
  vi.useRealTimers();
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
});

test("ingest failure is not acknowledged and the next success acks the same upto", async () => {
  drains(batch(2));
  const c = client();
  c.call.mockImplementation(async (method: string) => {
    if (method === "activity.rules") return rules;
    if (ingests(c).length === 1) throw new RpcError("internal");
    return { accepted: 2 };
  });
  const stop = startActivityUplink(c as UplinkClient);
  await tick();
  expect(calls("activity_configure")).toEqual([
    ["activity_configure", { rules: rules.rules, idleThresholdMs: 600_000 }],
  ]);
  expect(calls("activity_drain")).toEqual([["activity_drain", { max: 1000 }]]);
  expect(ingests(c)).toEqual([
    [
      "activity.ingest",
      { batch: { runId: "r1", upto: 2 }, events: batch(2).events, dropped: 3 },
      { clientRequestId: "activity-r1-2" },
    ],
  ]);
  expect(calls("activity_ack")).toEqual([]);
  await tick();
  expect(ingests(c)).toHaveLength(2);
  expect(calls("activity_ack")).toEqual([["activity_ack", { upto: 2 }]]);
  stop();
});

test("a lost response resends the held batch under the same clientRequestId", async () => {
  drains(batch(2), batch(3, 4), batch(3, 4));
  const c = client();
  c.call.mockImplementation(async (method: string) => {
    if (method === "activity.rules") return rules;
    if (ingests(c).length === 1) {
      c.set("reconnecting");
      throw new RpcError("connection_lost");
    }
    return { accepted: 2 };
  });
  const stop = startActivityUplink(c as UplinkClient);
  await tick();
  await tick();
  expect(ingests(c)).toHaveLength(1);
  expect(calls("activity_ack")).toEqual([]);
  c.set("ready");
  await tick();
  // Newer events waited natively; the uncertain batch went again unchanged.
  expect(calls("activity_drain")).toHaveLength(2);
  const [, params, opts] = ingests(c)[1] ?? [];
  expect(opts).toEqual({ clientRequestId: "activity-r1-2" });
  expect(params).toEqual({ batch: { runId: "r1", upto: 2 }, events: batch(2).events, dropped: 3 });
  expect(calls("activity_ack")).toEqual([["activity_ack", { upto: 2 }]]);
  // Fetched again after the reconnect; the same rules rev is not recompiled.
  expect(c.call.mock.calls.filter(([method]) => method === "activity.rules")).toHaveLength(2);
  expect(calls("activity_configure")).toHaveLength(1);
  await tick();
  expect(ingests(c)[2]?.[2]).toEqual({ clientRequestId: "activity-r1-3" });
  stop();
});

test("missing rules configure the native side with null and are fetched again", async () => {
  drains(batch(0));
  const c = client();
  c.call.mockRejectedValueOnce(new RpcError("internal")).mockResolvedValue(rules);
  const stop = startActivityUplink(c as UplinkClient);
  await tick();
  expect(calls("activity_configure")[0]).toEqual(["activity_configure", { rules: null, idleThresholdMs: null }]);
  await tick();
  expect(calls("activity_configure")[1]).toEqual([
    "activity_configure",
    { rules: rules.rules, idleThresholdMs: 600_000 },
  ]);
  expect(ingests(c)).toEqual([]);
  stop();
});

test("an error other than a lost connection drops the held batch for a fresh drain", async () => {
  drains(batch(2), batch(3));
  const c = client();
  c.call.mockImplementation(async (method: string) => {
    if (method === "activity.rules") return rules;
    if (ingests(c).length === 1) throw new RpcError("activity.invalid_input");
    return { accepted: 3 };
  });
  const stop = startActivityUplink(c as UplinkClient);
  await tick();
  await tick();
  // The newer events are sent with the rest, under the new upto's request id.
  expect(ingests(c)[1]?.[2]).toEqual({ clientRequestId: "activity-r1-3" });
  expect(calls("activity_ack")).toEqual([["activity_ack", { upto: 3 }]]);
  stop();
});

test("activity.changed refetches the rules and reconfigures the native side", async () => {
  drains(batch(0));
  const c = client();
  const next = { rev: "2", rules: { maskAllTitles: true }, idleThresholdMs: 300_000 };
  c.call.mockResolvedValueOnce(rules).mockResolvedValue(next);
  const stop = startActivityUplink(c as UplinkClient);
  await tick();
  expect(c.subscribe).toHaveBeenCalledWith("activity.changed", expect.any(Function), expect.any(Function));
  await tick();
  expect(calls("activity_configure")).toHaveLength(1);
  c.topics.get("activity.changed")?.onEvent("5", { rev: "5" });
  await tick();
  expect(calls("activity_configure")[1]).toEqual([
    "activity_configure",
    { rules: next.rules, idleThresholdMs: 300_000 },
  ]);
  // A sessions-only change keeps the same rules rev: fetched, not recompiled.
  c.topics.get("activity.changed")?.onResync();
  await tick();
  expect(c.call.mock.calls.filter(([method]) => method === "activity.rules")).toHaveLength(3);
  expect(calls("activity_configure")).toHaveLength(2);
  stop();
  expect(c.topics.size).toBe(0);
});

test("withdrawn consent forgets a held batch", async () => {
  // The native queue was emptied, then numbering went on.
  drains(batch(2), batch(0), { ...batch(4), events: [event(3), event(4)] });
  const c = client();
  c.call.mockImplementation(async (method: string) => {
    if (method === "activity.rules") return rules;
    throw new RpcError("connection_lost");
  });
  const stop = startActivityUplink(c as UplinkClient);
  await tick();
  await tick();
  expect(calls("activity_drain")).toHaveLength(2);
  expect(ingests(c)).toHaveLength(1);
  await tick();
  expect(ingests(c)[1]?.[1]).toEqual({ batch: { runId: "r1", upto: 4 }, events: [event(3), event(4)], dropped: 3 });
  stop();
});

test("browser mode and non-main windows never drain", async () => {
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  const c = client();
  let stop = startActivityUplink(c as UplinkClient);
  await tick();
  await tick();
  stop();
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  for (const label of ["capture", "popout-note-1"]) {
    mocks.label = label;
    stop = startActivityUplink(c as UplinkClient);
    await tick();
    await tick();
    stop();
  }
  expect(mocks.invoke).not.toHaveBeenCalled();
  expect(mocks.listen).not.toHaveBeenCalled();
  expect(c.call).not.toHaveBeenCalled();
  expect(c.subscribe).not.toHaveBeenCalled();
});

test("stop ends the loop", async () => {
  drains(batch(0));
  const c = client();
  c.call.mockResolvedValue(rules);
  const stop = startActivityUplink(c as UplinkClient);
  await tick();
  stop();
  const before = mocks.invoke.mock.calls.length;
  await tick();
  await tick();
  expect(mocks.invoke.mock.calls.length).toBe(before);
});
