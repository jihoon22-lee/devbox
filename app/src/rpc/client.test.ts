import fixtures from "../../../crates/protocol/fixtures/envelopes.json";
import { RpcClient, type Transport, type TransportState } from "./client";
import { decodeFrame, encodeFrame, jsonFrame } from "./frame";

class FakeTransport implements Transport {
  sent: Record<string, unknown>[] = [];
  frames = new Set<(f: Uint8Array) => void>();
  states = new Set<(s: TransportState) => void>();
  connect = vi.fn(() => this.emit("open"));
  close = vi.fn(() => this.emit("closed"));
  reconnect = vi.fn(() => this.close());
  failSend = false;
  send(f: Uint8Array) {
    if (this.failSend) throw Error("send failed");
    this.sent.push(JSON.parse(new TextDecoder().decode(decodeFrame(f).body)));
  }
  onFrame(cb: (f: Uint8Array) => void) {
    this.frames.add(cb);
    return () => {
      this.frames.delete(cb);
    };
  }
  onState(cb: (s: TransportState) => void) {
    this.states.add(cb);
    return () => {
      this.states.delete(cb);
    };
  }
  emit(s: TransportState) {
    for (const cb of this.states) cb(s);
  }
  raw(f: Uint8Array) {
    for (const cb of this.frames) cb(f);
  }
  deliver(msg: unknown) {
    this.raw(jsonFrame(msg));
  }
  welcome(daemonId = "d-1") {
    this.deliver({ ...fixtures.server.welcome, daemonId });
  }
  last() {
    return this.sent.at(-1)!;
  }
}
function setup(ready = true) {
  const t = new FakeTransport();
  const c = new RpcClient(t, { version: "1.0.0-dev", instance: "test", client: "test" });
  c.start();
  if (ready) t.welcome();
  return { t, c };
}
test("start/stop are idempotent and can restart without duplicate listeners", async () => {
  const { t, c } = setup();
  c.start();
  expect(t.connect).toHaveBeenCalledTimes(1);
  expect(t.sent[0]).toMatchObject({ type: "hello", protocol: 1 });
  const p = c.call("system.ping", {});
  c.stop();
  c.stop();
  await expect(p).rejects.toMatchObject({ code: "connection_lost" });
  expect(t.frames.size).toBe(0);
  c.start();
  t.welcome();
  expect(c.state).toBe("ready");
  expect(t.frames.size).toBe(1);
});
test("results, null and typed errors retain diagnostic detail", async () => {
  const { t, c } = setup();
  const p = c.call("system.ping", {});
  t.deliver({ type: "response", id: t.last().id, result: null });
  await expect(p).resolves.toBeNull();
  const bad = c.call("projects.add", { path: "rel" }, { clientRequestId: "stable" });
  expect(t.last().clientRequestId).toBe("stable");
  t.deliver({
    type: "response",
    id: t.last().id,
    error: { code: "projects.path_invalid", detail: { path: "rel" } },
    diagnosticId: "diag",
  });
  await expect(bad).rejects.toMatchObject({
    code: "projects.path_invalid",
    detail: { path: "rel" },
    diagnosticId: "diag",
  });
});
test("reconnect rejects calls, resubscribes, resyncs and resets only on daemon change", async () => {
  const { t, c } = setup();
  const event = vi.fn(),
    resync = vi.fn(),
    reset = vi.fn();
  c.onReset(reset);
  c.subscribe("projects.changed", event, resync);
  t.deliver({ type: "subscribed", id: t.last().id, subId: 7, rev: "1" });
  t.deliver({ type: "event", subId: 7, rev: "2", payload: { rev: "2" } });
  expect(event).toHaveBeenCalledWith("2", { rev: "2" });
  t.deliver({ type: "resync", subId: 7 });
  expect(resync).toHaveBeenCalledTimes(1);
  const p = c.call("projects.list", {});
  t.emit("closed");
  await expect(p).rejects.toMatchObject({ code: "connection_lost" });
  expect(c.state).toBe("reconnecting");
  t.emit("open");
  t.welcome();
  expect(t.last()).toMatchObject({ type: "subscribe", topic: "projects.changed" });
  expect(resync).toHaveBeenCalledTimes(1);
  expect(c.state).toBe("reconnecting");
  t.deliver({ type: "subscribed", id: t.last().id, subId: 8, rev: "2" });
  expect(c.state).toBe("ready");
  expect(resync).toHaveBeenCalledTimes(2);
  expect(reset).not.toHaveBeenCalled();
  t.emit("closed");
  t.emit("open");
  t.welcome("d-2");
  expect(reset).toHaveBeenCalledTimes(1);
});
test.each([{ version: "old" }, { protocol: 2 }])("mismatch closes and stays terminal: %j", (override) => {
  const { t, c } = setup(false);
  t.deliver({ ...fixtures.server.welcome, ...override });
  expect(c.state).toBe("versionMismatch");
  expect(t.close).toHaveBeenCalledOnce();
  t.emit("open");
  expect(t.sent).toHaveLength(1);
});
test("late subscription acknowledgement is unsubscribed after local disposal", () => {
  const { t, c } = setup();
  const off = c.subscribe("projects.changed", vi.fn(), vi.fn());
  const id = t.last().id;
  off();
  off();
  t.deliver({ type: "subscribed", id, subId: 42, rev: "1" });
  expect(t.last()).toEqual({ type: "unsubscribe", subId: 42 });
});
test("abort before send rejects; in-flight cancel waits for authoritative mutation outcome", async () => {
  const { t, c } = setup();
  const a = new AbortController();
  a.abort();
  await expect(c.call("projects.add", { path: "/p" }, { signal: a.signal })).rejects.toMatchObject({
    code: "cancelled",
  });
  expect(t.sent).toHaveLength(1);
  const b = new AbortController();
  const remove = vi.spyOn(b.signal, "removeEventListener");
  const p = c.call("projects.add", { path: "/p" }, { signal: b.signal });
  const id = t.last().id;
  b.abort();
  expect(t.last()).toEqual({ type: "cancel", id });
  t.deliver({ type: "response", id, result: { id: "created" } });
  await expect(p).resolves.toEqual({ id: "created" });
  expect(remove).toHaveBeenCalled();
  const d = new AbortController();
  const q = c.call("system.ping", {}, { signal: d.signal });
  t.deliver({ type: "response", id: t.last().id, error: { code: "cancelled" } });
  await expect(q).rejects.toMatchObject({ code: "cancelled" });
  const count = t.sent.length;
  d.abort();
  expect(t.sent).toHaveLength(count);
});
test("send failure settles pending calls and closes cleanly", async () => {
  const { t, c } = setup();
  const p = c.call("system.ping", {});
  t.failSend = true;
  const q = c.call("system.ping", {});
  await expect(q).rejects.toMatchObject({ code: "connection_lost" });
  await expect(p).rejects.toMatchObject({ code: "connection_lost" });
  expect(t.close).toHaveBeenCalled();
});
test.each([
  null,
  {},
  { type: "unknown" },
  { type: "response", id: 1 },
  { type: "response", id: -1, result: 1 },
  { type: "response", id: 1, result: 1, error: { code: "internal" } },
  { type: "event", subId: 1, rev: 2, payload: {} },
  { type: "welcome", protocol: 1 },
  { ...fixtures.server.welcome },
])("bad envelopes fail the live connection: %j", async (msg) => {
  const { t, c } = setup();
  const p = c.call("system.ping", {});
  expect(() => t.deliver(msg)).not.toThrow();
  await expect(p).rejects.toMatchObject({ code: "connection_lost" });
  expect(t.close).toHaveBeenCalled();
});
test.each([new Uint8Array([255]), new TextEncoder().encode("{")])(
  "bad UTF8/JSON rejects pending without escaping callback",
  async (body) => {
    const { t, c } = setup();
    const p = c.call("system.ping", {});
    expect(() => t.raw(encodeFrame({ kind: 0, channel: 0, body }))).not.toThrow();
    await expect(p).rejects.toMatchObject({ code: "connection_lost" });
  },
);
test("welcome before open is ignored and response before welcome is rejected", () => {
  const { t, c } = setup(false);
  t.emit("closed");
  t.welcome();
  expect(c.state).not.toBe("ready");
  t.emit("open");
  t.deliver({ type: "response", id: 1, result: 1 });
  expect(t.close).toHaveBeenCalled();
});

test("initial registration and restoration wait for all live subscriptions; disposal and rejection drain barrier", () => {
  const { t, c } = setup(false);
  const resync = vi.fn();
  const off = c.subscribe("projects.changed", vi.fn(), resync);
  c.subscribe("projects.changed", vi.fn(), resync);
  t.welcome();
  expect(c.state).toBe("connecting");
  const first = t.sent.at(-2)!.id,
    second = t.last().id;
  t.deliver({ type: "subscribed", id: first, subId: 1, rev: "1" });
  expect(c.state).toBe("connecting");
  t.deliver({ type: "subscribed", id: second, subId: 2, rev: "1" });
  expect(c.state).toBe("ready");
  t.emit("closed");
  t.emit("open");
  t.welcome();
  const late = t.sent.at(-2)!.id,
    rejected = t.last().id;
  off();
  expect(c.state).toBe("reconnecting");
  t.deliver({ type: "response", id: rejected, error: { code: "not_found" } });
  expect(c.state).toBe("ready");
  expect(resync).toHaveBeenCalledTimes(1);
  t.deliver({ type: "subscribed", id: late, subId: 3, rev: "1" });
  expect(t.last()).toEqual({ type: "unsubscribe", subId: 3 });
  expect(resync).toHaveBeenCalledTimes(1);
});
test("request IDs wrap within u32 and skip pending IDs", async () => {
  const { t, c } = setup();
  const first = c.call("system.ping", {});
  const firstId = t.last().id;
  Object.assign(c, { nextId: 0xffffffff });
  const last = c.call("system.ping", {});
  expect(t.last().id).toBe(0xffffffff);
  const wrapped = c.call("system.ping", {});
  expect(t.last().id).toBe(2);
  for (const id of [firstId, 0xffffffff, 2]) t.deliver({ type: "response", id, result: null });
  await Promise.all([first, last, wrapped]);
});

test("transient send failure allows transport recovery without restarting client", async () => {
  const { t, c } = setup();
  t.failSend = true;
  const p = c.call("system.ping", {});
  await expect(p).rejects.toMatchObject({ code: "connection_lost" });
  t.failSend = false;
  t.emit("open");
  t.welcome();
  expect(c.state).toBe("ready");
  expect(t.frames.size).toBe(1);
});
test.each(["bad", "-1", "1.5", ""])("malformed revision %s cannot escape through a consumer", (rev) => {
  const { t, c } = setup();
  const event = vi.fn();
  c.subscribe("projects.changed", event, vi.fn());
  t.deliver({ type: "subscribed", id: t.last().id, subId: 1, rev: "1" });
  expect(() => t.deliver({ type: "event", subId: 1, rev, payload: {} })).not.toThrow();
  expect(event).not.toHaveBeenCalled();
  expect(t.close).toHaveBeenCalled();
});
test("throwing reset listeners do not interrupt restoration or other listeners", () => {
  const { t, c } = setup();
  const log = vi.spyOn(console, "error").mockImplementation(() => {});
  const reset = vi.fn();
  c.onReset(() => {
    throw Error("consumer failed");
  });
  c.onReset(reset);
  t.emit("closed");
  t.emit("open");
  expect(() => t.welcome("d-2")).not.toThrow();
  expect(c.state).toBe("ready");
  expect(reset).toHaveBeenCalledOnce();
  log.mockRestore();
});
test("fallback reconnect is delayed, bounded, and stop cancels its timer", async () => {
  vi.useFakeTimers();
  try {
    const { t, c } = setup();
    Object.assign(t, { reconnect: undefined });
    t.failSend = true;
    const p = c.call("system.ping", {});
    await expect(p).rejects.toMatchObject({ code: "connection_lost" });
    for (const delay of [200, 400, 800, 1600, 3200, 5000, 5000]) {
      const count = t.connect.mock.calls.length;
      vi.advanceTimersByTime(delay - 1);
      expect(t.connect).toHaveBeenCalledTimes(count);
      vi.advanceTimersByTime(1);
      expect(t.connect).toHaveBeenCalledTimes(count + 1);
    }
    c.stop();
    const count = t.connect.mock.calls.length;
    vi.runAllTimers();
    expect(t.connect).toHaveBeenCalledTimes(count);
    expect(t.frames.size).toBe(0);
  } finally {
    vi.useRealTimers();
  }
});
test("a subscription created by reset listener is registered once", () => {
  const { t, c } = setup();
  c.onReset(() => c.subscribe("projects.changed", vi.fn(), vi.fn()));
  t.emit("closed");
  t.emit("open");
  t.welcome("d-2");
  expect(t.sent.filter((m) => m.type === "subscribe")).toHaveLength(1);
  expect(c.state).toBe("reconnecting");
  t.deliver({ type: "subscribed", id: t.last().id, subId: 1, rev: "1" });
  expect(c.state).toBe("ready");
});

describe("Welcome deadline", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  test("silent open connection retries after a bounded wait and fallback backoff", () => {
    const { t, c } = setup(false);
    Object.assign(t, { reconnect: undefined });
    vi.advanceTimersByTime(4999);
    expect(t.close).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(t.close).toHaveBeenCalledOnce();
    vi.advanceTimersByTime(199);
    expect(t.connect).toHaveBeenCalledOnce();
    vi.advanceTimersByTime(1);
    expect(t.connect).toHaveBeenCalledTimes(2);
    t.welcome();
    expect(c.state).toBe("ready");
    expect(vi.getTimerCount()).toBe(0);
    c.stop();
  });

  test("timeout delegates to transport reconnect and ignores its stale callback", () => {
    const timer = vi.spyOn(globalThis, "setTimeout");
    const { t, c } = setup(false);
    const stale = timer.mock.calls.at(-1)![0] as () => void;
    vi.advanceTimersByTime(5000);
    expect(t.reconnect).toHaveBeenCalledOnce();
    t.emit("open");
    stale();
    expect(t.reconnect).toHaveBeenCalledOnce();
    t.welcome();
    expect(c.state).toBe("ready");
    expect(vi.getTimerCount()).toBe(0);
    c.stop();
    timer.mockRestore();
  });

  test.each(["welcome", "closed", "stop", "mismatch"])("%s cancels the deadline", (event) => {
    const { t, c } = setup(false);
    expect(vi.getTimerCount()).toBe(1);
    if (event === "welcome") t.welcome();
    if (event === "closed") t.emit("closed");
    if (event === "stop") c.stop();
    if (event === "mismatch") t.deliver({ ...fixtures.server.welcome, version: "old" });
    expect(vi.getTimerCount()).toBe(0);
    vi.advanceTimersByTime(5000);
    expect(t.reconnect).not.toHaveBeenCalled();
    c.stop();
  });

  test("synchronous Welcome during Hello send leaves no timer", () => {
    const t = new FakeTransport();
    const send = t.send.bind(t);
    t.send = (frame) => {
      send(frame);
      t.welcome();
    };
    const c = new RpcClient(t, { version: "1.0.0-dev", instance: "test", client: "test" });
    c.start();
    expect(c.state).toBe("ready");
    expect(vi.getTimerCount()).toBe(0);
    vi.advanceTimersByTime(5000);
    expect(t.reconnect).not.toHaveBeenCalled();
    c.stop();
  });
});
