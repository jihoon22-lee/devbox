import { jsonFrame } from "./frame";
import { wsTransport } from "./transport-ws";

class Socket {
  static OPEN = 1;
  static instances: Socket[] = [];
  readyState = 0;
  binaryType = "";
  onopen?: () => void;
  onclose?: () => void;
  onerror?: () => void;
  onmessage?: (e: { data: unknown }) => void;
  send = vi.fn();
  close = vi.fn(() => {
    this.readyState = 3;
    this.onclose?.();
  });
  constructor(readonly url: string) {
    Socket.instances.push(this);
  }
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
}
beforeEach(() => {
  vi.useFakeTimers();
  Socket.instances = [];
  vi.stubGlobal("WebSocket", Socket);
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
const latest = () => Socket.instances.at(-1)!;
test("connect is idempotent; sends only while open and relays binary frames", () => {
  const t = wsTransport("ws://test");
  const frame = vi.fn();
  t.onFrame(frame);
  expect(() => t.send(jsonFrame({}))).toThrow();
  t.connect();
  t.connect();
  expect(Socket.instances).toHaveLength(1);
  expect(latest().binaryType).toBe("arraybuffer");
  latest().open();
  const raw = jsonFrame({});
  t.send(raw);
  expect(latest().send).toHaveBeenCalledWith(raw);
  latest().onmessage?.({ data: raw.buffer });
  expect(frame).toHaveBeenCalledWith(raw);
});
test("close cancels reconnect and obsolete socket callbacks cannot affect restart", () => {
  const t = wsTransport("ws://test");
  const state = vi.fn();
  t.onState(state);
  t.connect();
  const old = latest();
  old.close();
  t.close();
  vi.runAllTimers();
  expect(Socket.instances).toHaveLength(1);
  t.connect();
  latest().open();
  state.mockClear();
  old.onclose?.();
  old.onopen?.();
  old.onmessage?.({ data: "bad" });
  vi.advanceTimersByTime(10000);
  expect(Socket.instances).toHaveLength(2);
  expect(state).not.toHaveBeenCalled();
});
test("reconnect uses bounded delays and error without close also reconnects", () => {
  const t = wsTransport("ws://test");
  t.connect();
  for (const delay of [200, 500, 1000, 2000, 5000, 5000]) {
    const n = Socket.instances.length;
    latest().onerror?.();
    vi.advanceTimersByTime(delay - 1);
    expect(Socket.instances).toHaveLength(n);
    vi.advanceTimersByTime(1);
    expect(Socket.instances).toHaveLength(n + 1);
  }
  t.close();
});
test.each(["text", "short", "blob"])("rejects malformed incoming payload %s gracefully", (kind) => {
  const data = kind === "text" ? "text" : kind === "short" ? new ArrayBuffer(2) : new Blob(["bad"]);
  const t = wsTransport("ws://test");
  const frame = vi.fn();
  t.onFrame(frame);
  t.connect();
  const s = latest();
  s.open();
  expect(() => s.onmessage?.({ data })).not.toThrow();
  expect(s.close).toHaveBeenCalled();
  expect(frame).not.toHaveBeenCalled();
  t.close();
});
test("explicit reconnect drops the old socket and schedules recovery; close cancels it", () => {
  const t = wsTransport("ws://test");
  t.connect();
  const old = latest();
  old.open();
  t.reconnect?.();
  expect(old.close).toHaveBeenCalledOnce();
  expect(Socket.instances).toHaveLength(1);
  vi.advanceTimersByTime(200);
  expect(Socket.instances).toHaveLength(2);
  latest().open();
  t.reconnect?.();
  t.close();
  vi.runAllTimers();
  expect(Socket.instances).toHaveLength(2);
});
