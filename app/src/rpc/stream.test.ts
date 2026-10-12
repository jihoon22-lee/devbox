import { RpcClient, type Transport, type TransportState } from "./client";
import { decodeFrame, encodeFrame, FRAME, jsonFrame } from "./frame";

class StreamTransport implements Transport {
  sent: Uint8Array[] = [];
  frames = new Set<(frame: Uint8Array) => void>();
  states = new Set<(state: TransportState) => void>();
  failSend = false;
  connect = vi.fn(() => this.emit("open"));
  close = vi.fn(() => this.emit("closed"));
  reconnect = vi.fn(() => this.close());
  send(frame: Uint8Array) {
    if (this.failSend) throw new Error("send failed");
    this.sent.push(frame);
  }
  onFrame(cb: (frame: Uint8Array) => void) {
    this.frames.add(cb);
    return () => {
      this.frames.delete(cb);
    };
  }
  onState(cb: (state: TransportState) => void) {
    this.states.add(cb);
    return () => {
      this.states.delete(cb);
    };
  }
  emit(state: TransportState) {
    for (const cb of this.states) cb(state);
  }
  raw(frame: Uint8Array) {
    for (const cb of this.frames) cb(frame);
  }
  deliver(message: unknown) {
    this.raw(jsonFrame(message));
  }
  welcome() {
    this.deliver({ type: "welcome", version: "v", protocol: 1, daemonId: "daemon" });
  }
  lastRequest() {
    return this.messages(FRAME.json)
      .filter((message) => message.type === "request")
      .at(-1)!;
  }
  messages(kind: number) {
    return this.sent
      .map(decodeFrame)
      .filter((frame) => frame.kind === kind)
      .map((frame) => JSON.parse(new TextDecoder().decode(frame.body)) as Record<string, unknown>);
  }
  response(streamId: unknown) {
    this.deliver({ type: "response", id: this.lastRequest().id, result: { streamId } });
  }
}
function chunk(streamId: number, data: Uint8Array = new Uint8Array(), flags = 0) {
  const body = new Uint8Array(5 + data.length);
  new DataView(body.buffer).setUint32(0, streamId, true);
  body[4] = flags;
  body.set(data, 5);
  return encodeFrame({ kind: FRAME.stream, channel: 0, body });
}
function setup(ready = true) {
  const t = new StreamTransport();
  const c = new RpcClient(t, { version: "v", instance: "test-stream", client: "test" });
  c.start();
  if (ready) t.welcome();
  return { t, c };
}
const handlers = () => ({ onData: vi.fn<(bytes: Uint8Array, ack: () => void) => void>(), onEnd: vi.fn() });
async function opened() {
  const { t, c } = setup();
  const h = handlers();
  const p = c.openStream("debug.echo", { bytes: 0 }, h);
  t.response(9);
  return { t, c, h, stream: await p };
}

test.each([false, true])("early data and FIN retain order, including FIN-only=%s", async (finOnly) => {
  const { t, c } = setup();
  const order: string[] = [];
  const h = {
    onData: (bytes: Uint8Array, ack: () => void) => {
      order.push(`data:${[...bytes]}`);
      ack();
    },
    onEnd: () => {
      order.push("end");
    },
  };
  const p = c.openStream("debug.echo", { bytes: 0 }, h);
  t.raw(chunk(9, finOnly ? new Uint8Array() : new Uint8Array([1, 2]), 1));
  t.response(9);
  const stream = await p;
  expect(order).toEqual(finOnly ? ["end"] : ["data:1,2", "end"]);
  expect(stream.streamId).toBe(9);
  const before = t.sent.length;
  stream.end();
  stream.write(new Uint8Array([7]));
  t.emit("closed");
  expect(t.sent).toHaveLength(before);
  expect(order.filter((value) => value === "end")).toHaveLength(1);
});

test("credit is returned only once per consumed chunk; delayed ACK after FIN is inert", async () => {
  const { t, h } = await opened();
  t.raw(chunk(9, new Uint8Array([1, 2])));
  expect(t.messages(FRAME.control)).toEqual([]);
  const ack = h.onData.mock.calls[0]![1];
  ack();
  ack();
  expect(t.messages(FRAME.control)).toEqual([{ type: "streamCredit", stream: 9, bytes: 2 }]);
  t.raw(chunk(9, new Uint8Array([3]), 1));
  h.onData.mock.calls[1]![1]();
  expect(t.messages(FRAME.control)).toHaveLength(1);
  expect(h.onEnd).toHaveBeenCalledOnce();
});

test("end sends full StreamClose once and late frames do not resurrect disposed streams", async () => {
  const { t, c, h, stream } = await opened();
  stream.end();
  stream.end();
  expect(t.messages(FRAME.control)).toEqual([{ type: "streamClose", stream: 9 }]);
  expect(h.onEnd).toHaveBeenCalledOnce();
  t.raw(chunk(9, new Uint8Array([1]), 1));
  expect(h.onData).not.toHaveBeenCalled();
  const next = handlers();
  const p = c.openStream("debug.echo", { bytes: 0 }, next);
  t.response(10);
  await p;
  expect(next.onData).not.toHaveBeenCalled();
});

test("old writes, ends and ACKs cannot affect a reconnected stream", async () => {
  const { t, c, h, stream } = await opened();
  t.raw(chunk(9, new Uint8Array([1])));
  const ack = h.onData.mock.calls[0]![1];
  t.emit("closed");
  t.emit("open");
  t.welcome();
  const fresh = handlers();
  const p = c.openStream("debug.echo", { bytes: 0 }, fresh);
  t.response(9);
  const next = await p;
  const before = t.sent.length;
  stream.write(new Uint8Array([3]));
  stream.end();
  ack();
  expect(t.sent).toHaveLength(before);
  expect(h.onEnd).toHaveBeenCalledOnce();
  next.write(new Uint8Array([4]));
  expect(decodeFrame(t.sent.at(-1)!).body).toEqual(new Uint8Array([9, 0, 0, 0, 0, 4]));
});

test("disconnect between response and promise continuation never registers a dead stream", async () => {
  const { t, c } = setup();
  const h = handlers();
  const p = c.openStream("debug.echo", { bytes: 0 }, h);
  t.response(9);
  t.emit("closed");
  await expect(p).rejects.toMatchObject({ code: "connection_lost" });
  expect(h.onEnd).toHaveBeenCalledOnce();
  t.emit("open");
  t.welcome();
  t.raw(chunk(9, new Uint8Array([1])));
  expect(h.onData).not.toHaveBeenCalled();
});

test.each(["write", "ack", "end"] as const)(
  "%s send failure closes streams and calls, then reconnects",
  async (action) => {
    const { t, c, h, stream } = await opened();
    t.raw(chunk(9, new Uint8Array([1])));
    const rpc = c.call("system.ping", {});
    t.failSend = true;
    expect(() => {
      if (action === "write") stream.write(new Uint8Array([1]));
      if (action === "ack") h.onData.mock.calls[0]![1]();
      if (action === "end") stream.end();
    }).not.toThrow();
    await expect(rpc).rejects.toMatchObject({ code: "connection_lost" });
    expect(h.onEnd).toHaveBeenCalledOnce();
    expect(t.reconnect).toHaveBeenCalled();
    t.failSend = false;
    t.emit("open");
    t.welcome();
    expect(c.state).toBe("ready");
  },
);

test("writes split at 64 KiB and full close follows the final chunk", async () => {
  const { t, stream } = await opened();
  stream.write(new Uint8Array(65537).fill(7));
  stream.end();
  const data = t.sent.map(decodeFrame).filter((frame) => frame.kind === FRAME.stream);
  expect(data.map((frame) => frame.body.length)).toEqual([65541, 6]);
  expect(data.every((frame) => frame.body[4] === 0)).toBe(true);
  expect(t.messages(FRAME.control)).toEqual([{ type: "streamClose", stream: 9 }]);
});

test.each([-1, 1.5, 2 ** 32, "9", null, undefined])("invalid open result %s fails the connection", async (streamId) => {
  const { t, c } = setup();
  const h = handlers();
  const p = c.openStream("debug.echo", { bytes: 0 }, h);
  t.response(streamId);
  await expect(p).rejects.toMatchObject({ code: "connection_lost" });
  expect(t.reconnect).toHaveBeenCalledOnce();
  expect(h.onEnd).toHaveBeenCalledOnce();
});

test("duplicate active stream IDs fail without replacing the first handlers", async () => {
  const { t, c, h } = await opened();
  const second = handlers();
  const p = c.openStream("debug.echo", { bytes: 0 }, second);
  t.response(9);
  await expect(p).rejects.toMatchObject({ code: "connection_lost" });
  expect(h.onEnd).toHaveBeenCalledOnce();
  expect(second.onEnd).toHaveBeenCalledOnce();
});

test.each([new Uint8Array(4), new Uint8Array([1, 0, 0, 0, 2])])(
  "malformed stream body fails live RPC",
  async (body) => {
    const { t, c } = setup();
    const p = c.call("system.ping", {});
    t.raw(encodeFrame({ kind: FRAME.stream, channel: 0, body }));
    await expect(p).rejects.toMatchObject({ code: "connection_lost" });
  },
);

test("stream frames before Welcome violate the handshake", () => {
  const { t, c } = setup(false);
  t.raw(chunk(9));
  expect(t.reconnect).toHaveBeenCalledOnce();
  c.stop();
});

test("an entire 256 KiB window of one-byte early frames is coalesced without loss", async () => {
  const { t, c } = setup();
  let count = 0;
  const h = {
    onData: (bytes: Uint8Array, ack: () => void) => {
      count += bytes.length;
      ack();
    },
    onEnd: vi.fn(),
  };
  const p = c.openStream("debug.echo", { bytes: 0 }, h);
  const one = chunk(9, new Uint8Array([7]));
  for (let i = 0; i < 256 * 1024; i++) t.raw(one);
  t.raw(chunk(9, new Uint8Array(), 1));
  t.response(9);
  await p;
  expect(count).toBe(256 * 1024);
  expect(h.onEnd).toHaveBeenCalledOnce();
  expect(t.reconnect).not.toHaveBeenCalled();
});

test.each([false, true])("unacknowledged bytes cannot exceed one window, early=%s", async (early) => {
  const { t, c } = setup();
  const h = handlers();
  const p = c.openStream("debug.echo", { bytes: 0 }, h);
  if (!early) {
    t.response(9);
    await p;
  }
  for (let i = 0; i < 4; i++) t.raw(chunk(9, new Uint8Array(65536)));
  t.raw(chunk(9, new Uint8Array([1])));
  if (early) await expect(p).rejects.toMatchObject({ code: "connection_lost" });
  expect(t.reconnect).toHaveBeenCalledOnce();
  expect(h.onEnd).toHaveBeenCalledOnce();
});

test("orphan early streams are closed when the last open settles", async () => {
  const { t, c } = setup();
  const p = c.openStream("debug.echo", { bytes: 0 }, handlers());
  t.raw(chunk(19, new Uint8Array([1])));
  t.response(9);
  await p;
  expect(t.messages(FRAME.control)).toContainEqual({ type: "streamClose", stream: 19 });
  t.raw(chunk(19, new Uint8Array([2])));
  expect(t.messages(FRAME.control)).toHaveLength(1);
});

test("unknown FIN-only IDs cannot grow an unbounded early map", async () => {
  const { t, c } = setup();
  const p = c.openStream("debug.echo", { bytes: 0 }, handlers());
  for (let id = 1; id <= 1000; id++) t.raw(chunk(id, new Uint8Array(), 1));
  await expect(p).rejects.toMatchObject({ code: "connection_lost" });
  expect(t.reconnect).toHaveBeenCalledOnce();
});

test("listener exceptions do not block other stream endings or subscription recovery", async () => {
  const { t, c, h } = await opened();
  const log = vi.spyOn(console, "error").mockImplementation(() => {});
  h.onData.mockImplementation(() => {
    throw new Error("bad consumer");
  });
  h.onEnd.mockImplementation(() => {
    throw new Error("bad consumer");
  });
  const second = handlers();
  const p = c.openStream("debug.echo", { bytes: 0 }, second);
  t.response(10);
  await p;
  t.raw(chunk(9, new Uint8Array([1])));
  expect(t.reconnect).not.toHaveBeenCalled();
  t.emit("closed");
  expect(second.onEnd).toHaveBeenCalledOnce();
  t.emit("open");
  t.welcome();
  expect(c.state).toBe("ready");
  log.mockRestore();
});

test("synchronous ACK transport cannot reorder live data ahead of remaining early bytes", async () => {
  const { t, c } = setup();
  const seen: number[] = [];
  const p = c.openStream(
    "debug.echo",
    { bytes: 0 },
    {
      onData(bytes, ack) {
        for (const byte of bytes) seen.push(byte);
        ack();
      },
      onEnd: vi.fn(),
    },
  );
  t.raw(chunk(9, new Uint8Array(65536).fill(1)));
  t.raw(chunk(9, new Uint8Array([2])));
  const send = t.send.bind(t);
  let injected = false;
  t.send = (frame) => {
    send(frame);
    if (!injected && decodeFrame(frame).kind === FRAME.control) {
      injected = true;
      t.raw(chunk(9, new Uint8Array([3])));
    }
  };
  t.response(9);
  await p;
  expect(seen.slice(-3)).toEqual([1, 2, 3]);
});

test("synchronous onEnd cannot submit a call or stream while disconnect is clearing state", async () => {
  const { t, c, h } = await opened();
  let rpc: Promise<unknown> | undefined;
  let stream: Promise<unknown> | undefined;
  h.onEnd.mockImplementation(() => {
    rpc = c.call("system.ping", {});
    stream = c.openStream("debug.echo", { bytes: 0 }, handlers());
  });
  const before = t.sent.length;
  t.emit("closed");
  await expect(rpc).rejects.toMatchObject({ code: "connection_lost" });
  await expect(stream).rejects.toMatchObject({ code: "connection_lost" });
  expect(t.sent).toHaveLength(before);
});

test("the aggregate early byte budget is bounded across stream IDs", async () => {
  const { t, c } = setup();
  const p = c.openStream("debug.echo", { bytes: 0 }, handlers());
  for (let id = 1; id <= 17; id++) {
    for (let part = 0; part < 4; part++) t.raw(chunk(id, new Uint8Array(65536)));
  }
  await expect(p).rejects.toMatchObject({ code: "connection_lost" });
  expect(t.reconnect).toHaveBeenCalledOnce();
});

test("RPC open rejection closes early orphans and preserves the authoritative error", async () => {
  const { t, c } = setup();
  const h = handlers();
  const p = c.openStream("debug.echo", { bytes: 0 }, h);
  t.raw(chunk(9, new Uint8Array([1])));
  t.deliver({ type: "response", id: t.lastRequest().id, error: { code: "not_found" } });
  await expect(p).rejects.toMatchObject({ code: "not_found" });
  expect(t.messages(FRAME.control)).toContainEqual({ type: "streamClose", stream: 9 });
  expect(h.onEnd).toHaveBeenCalledOnce();
  expect(c.state).toBe("ready");
});
