import { type ClientState, RpcClient, RpcError, type Transport, type TransportState } from "../../rpc/client";
import type { ClientStream, StreamHandlers } from "../../rpc/client-streams";
import { decodeFrame, FRAME, jsonFrame, streamFrame } from "../../rpc/frame";
import type { Methods } from "../../rpc/gen/rpc";
import { type LinkState, TerminalLink } from "./link";

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
type Opened = ClientStream & { result: Methods["terminal.attach"]["result"] };
function fixture(state: ClientState = "ready") {
  const stateCallbacks = new Set<(state: ClientState) => void>();
  const opened: { handlers: StreamHandlers; stream: Opened; pending: ReturnType<typeof deferred<Opened>> }[] = [];
  let automatic = true;
  let early: ((handlers: StreamHandlers) => void) | undefined;
  const client = {
    state,
    onState: (callback: (state: ClientState) => void) => {
      stateCallbacks.add(callback);
      return () => {
        stateCallbacks.delete(callback);
      };
    },
    call: vi.fn().mockResolvedValue({}),
    openStream: vi.fn(
      (_method: "terminal.attach", _params: Methods["terminal.attach"]["params"], handlers: StreamHandlers) => {
        const id = opened.length + 1;
        const stream: Opened = {
          streamId: id,
          result: { streamId: id },
          write: vi.fn(),
          end: vi.fn(() => handlers.onEnd()),
        };
        const pending = deferred<Opened>();
        opened.push({ handlers, stream, pending });
        early?.(handlers);
        if (automatic) pending.resolve(stream);
        return pending.promise;
      },
    ),
  };
  const done: (() => void)[] = [];
  const sink = {
    write: vi.fn((_data: Uint8Array, ack: () => void) => {
      done.push(ack);
    }),
    reset: vi.fn(),
  };
  const states: LinkState[] = [];
  const dropped = vi.fn();
  const make = (readOnly = false) =>
    new TerminalLink(client as unknown as RpcClient, {
      id: "t1",
      readOnly,
      sink,
      onState: (state) => states.push(state),
      onInputDropped: dropped,
    });
  return {
    client,
    opened,
    done,
    sink,
    states,
    dropped,
    make,
    manual: () => {
      automatic = false;
    },
    early: (callback: (handlers: StreamHandlers) => void) => {
      early = callback;
    },
    state: (state: ClientState) => {
      client.state = state;
      for (const cb of stateCallbacks) cb(state);
    },
  };
}
const flush = async () => {
  await Promise.resolve();
  await Promise.resolve();
};
beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

test("attaches, credits only after rendering, writes text/raw bytes and coalesces dimensions", async () => {
  const f = fixture();
  const link = f.make();
  await link.open(80, 24);
  expect(f.states).toEqual(["connecting", "live"]);
  expect(f.client.openStream).toHaveBeenCalledWith(
    "terminal.attach",
    { id: "t1", cols: 80, rows: 24, readOnly: false },
    expect.anything(),
  );
  const ack = vi.fn();
  f.opened[0]!.handlers.onData(new Uint8Array([65]), ack);
  expect(ack).not.toHaveBeenCalled();
  f.done[0]!();
  f.done[0]!();
  expect(ack).toHaveBeenCalledOnce();
  link.input("한글\r");
  link.input(new Uint8Array([0x80, 0xff]));
  expect(f.opened[0]!.stream.write).toHaveBeenNthCalledWith(1, new TextEncoder().encode("한글\r"));
  expect(f.opened[0]!.stream.write).toHaveBeenNthCalledWith(2, new Uint8Array([0x80, 0xff]));
  link.resize(100, 30);
  link.resize(120, 40);
  await vi.advanceTimersByTimeAsync(100);
  expect(f.client.call).toHaveBeenCalledExactlyOnceWith("terminal.resize", { streamId: 1, cols: 120, rows: 40 });
  link.dispose();
});
test("initial open waits for ready with no immediate onState callback", async () => {
  const f = fixture("connecting");
  const link = f.make();
  await link.open(80, 24);
  expect(f.opened).toHaveLength(0);
  f.state("ready");
  await flush();
  expect(f.opened).toHaveLength(1);
  expect(f.states.at(-1)).toBe("live");
  expect(f.sink.reset).not.toHaveBeenCalled();
  link.dispose();
});
test("reset onEnd before client state changes remains recoverable; old data/acks/resizes are discarded", async () => {
  const f = fixture();
  const link = f.make();
  await link.open(80, 24);
  const ack = vi.fn();
  f.opened[0]!.handlers.onData(new Uint8Array([65]), ack);
  link.resize(120, 40);
  f.opened[0]!.handlers.onEnd(); // RpcClient.reset invokes this while state is still ready.
  f.state("reconnecting");
  link.input("a");
  link.input("b");
  expect(f.dropped).toHaveBeenCalledOnce();
  f.state("ready");
  await flush();
  expect(f.opened).toHaveLength(2);
  expect(f.sink.reset).toHaveBeenCalledOnce();
  f.done[0]!();
  f.opened[0]!.handlers.onData(new Uint8Array([66]), vi.fn());
  expect(ack).not.toHaveBeenCalled();
  expect(f.sink.write).toHaveBeenCalledOnce();
  await vi.advanceTimersByTimeAsync(100);
  expect(f.client.call).not.toHaveBeenCalled();
  expect(f.client.openStream.mock.calls[1]?.[1]).toMatchObject({ cols: 120, rows: 40 });
  f.state("reconnecting");
  link.input("c");
  expect(f.dropped).toHaveBeenCalledTimes(2);
  link.dispose();
});
test("read-only calls never send input or resize", async () => {
  const f = fixture();
  const link = f.make(true);
  await link.open(80, 24);
  link.input("a");
  link.input(new Uint8Array([255]));
  link.resize(120, 40);
  await vi.advanceTimersByTimeAsync(100);
  expect(f.opened[0]!.stream.write).not.toHaveBeenCalled();
  expect(f.client.call).not.toHaveBeenCalled();
  f.state("reconnecting");
  link.input("b");
  expect(f.dropped).not.toHaveBeenCalled();
  link.dispose();
});
test("obsolete pending opens and not_found failures cannot replace a newer stream", async () => {
  const f = fixture();
  f.manual();
  const link = f.make();
  const first = link.open(80, 24);
  const second = link.open(100, 30);
  f.opened[1]!.pending.resolve(f.opened[1]!.stream);
  await second;
  f.opened[0]!.pending.resolve(f.opened[0]!.stream);
  await first;
  expect(f.opened[0]!.stream.end).toHaveBeenCalledOnce();
  expect(f.states.at(-1)).toBe("live");
  const third = link.open(120, 40);
  const fourth = link.open(140, 50);
  f.opened[3]!.pending.resolve(f.opened[3]!.stream);
  await fourth;
  f.opened[2]!.pending.reject(new RpcError("terminal.not_found"));
  await third;
  expect(f.states.at(-1)).toBe("live");
  link.dispose();
});
test("pending attachment adopts latest dimensions once live", async () => {
  const f = fixture();
  f.manual();
  const link = f.make();
  const opening = link.open(80, 24);
  link.resize(120, 40);
  await vi.advanceTimersByTimeAsync(100);
  expect(f.client.call).not.toHaveBeenCalled();
  f.opened[0]!.pending.resolve(f.opened[0]!.stream);
  await opening;
  await vi.advanceTimersByTimeAsync(100);
  expect(f.client.call).toHaveBeenCalledWith("terminal.resize", { streamId: 1, cols: 120, rows: 40 });
  link.dispose();
});
test("early FIN never becomes live; bounded reattach detects terminal.not_found", async () => {
  const f = fixture();
  f.early((h) => h.onEnd());
  const link = f.make();
  await link.open(80, 24);
  expect(f.states).not.toContain("live");
  f.client.openStream.mockRejectedValueOnce(new RpcError("terminal.not_found"));
  await vi.advanceTimersByTimeAsync(100);
  expect(f.states.at(-1)).toBe("ended");
  f.state("reconnecting");
  f.state("ready");
  await flush();
  expect(f.client.openStream).toHaveBeenCalledTimes(2);
  link.dispose();
});
test("repeated EOF while ready does not hot-loop or leave connecting", async () => {
  const f = fixture();
  f.early((h) => h.onEnd());
  const link = f.make();
  await link.open(80, 24);
  await vi.advanceTimersByTimeAsync(10000);
  expect(f.client.openStream).toHaveBeenCalledTimes(2);
  expect(f.states.at(-1)).toBe("lost");
  link.dispose();
});
test("sink failure closes current/pending stream without acknowledging or becoming live", async () => {
  const f = fixture();
  const ack = vi.fn();
  f.sink.write.mockImplementation(() => {
    throw new Error("renderer failed");
  });
  f.early((h) => h.onData(new Uint8Array([65]), ack));
  const link = f.make();
  await link.open(80, 24);
  expect(f.opened[0]!.stream.end).toHaveBeenCalledOnce();
  expect(ack).not.toHaveBeenCalled();
  expect(f.states.at(-1)).toBe("lost");
  await vi.advanceTimersByTimeAsync(1000);
  expect(f.opened).toHaveLength(1);
  link.dispose();
});
test("dispose during pending attach closes late handle and ignores all subsequent callbacks", async () => {
  const f = fixture();
  f.manual();
  const link = f.make();
  const opening = link.open(80, 24);
  link.dispose();
  f.opened[0]!.pending.resolve(f.opened[0]!.stream);
  await opening;
  const states = [...f.states];
  f.opened[0]!.handlers.onData(new Uint8Array([65]), vi.fn());
  f.state("ready");
  link.input("a");
  link.resize(120, 40);
  await vi.advanceTimersByTimeAsync(1000);
  expect(f.opened[0]!.stream.end).toHaveBeenCalledOnce();
  expect(f.sink.write).not.toHaveBeenCalled();
  expect(f.states).toEqual(states);
  expect(f.dropped).not.toHaveBeenCalled();
});
test("a synchronous input send disconnect warns once immediately", async () => {
  const f = fixture();
  const link = f.make();
  await link.open(80, 24);
  vi.mocked(f.opened[0]!.stream.write).mockImplementation(() => f.state("reconnecting"));
  link.input("a");
  link.input("b");
  expect(f.dropped).toHaveBeenCalledOnce();
  expect(f.states.at(-1)).toBe("lost");
  link.dispose();
});
test("sink reset disposal prevents reattach and state callbacks can dispose before opening", async () => {
  const f = fixture();
  const link = f.make();
  await link.open(80, 24);
  f.state("reconnecting");
  f.sink.reset.mockImplementation(() => link.dispose());
  f.state("ready");
  await flush();
  expect(f.opened).toHaveLength(1);
  const other = fixture();
  let disposed!: TerminalLink;
  disposed = new TerminalLink(other.client as unknown as RpcClient, {
    id: "t1",
    readOnly: false,
    sink: other.sink,
    onState: () => disposed.dispose(),
    onInputDropped: vi.fn(),
  });
  await disposed.open(80, 24);
  expect(other.opened).toHaveLength(0);
});
test("resize rejection is handled and a late write callback cannot credit after disposal", async () => {
  const f = fixture();
  const link = f.make();
  await link.open(80, 24);
  const ack = vi.fn();
  f.opened[0]!.handlers.onData(new Uint8Array([65]), ack);
  f.client.call.mockRejectedValueOnce(new RpcError("connection_lost"));
  link.resize(100, 30);
  await vi.advanceTimersByTimeAsync(100);
  link.dispose();
  f.done[0]!();
  expect(ack).not.toHaveBeenCalled();
});

function realClientFixture(earlyFin = false) {
  const frames = new Set<(bytes: Uint8Array) => void>();
  const states = new Set<(state: TransportState) => void>();
  const sent: Record<string, unknown>[] = [];
  let streamId = 0;
  const emit = (state: TransportState) => {
    for (const cb of states) cb(state);
  };
  const receive = (bytes: Uint8Array) => {
    for (const cb of frames) cb(bytes);
  };
  const transport: Transport = {
    connect: () => emit("open"),
    close: () => emit("closed"),
    send: (bytes) => {
      const frame = decodeFrame(bytes);
      if (frame.kind === FRAME.stream) return;
      const message = JSON.parse(new TextDecoder().decode(frame.body));
      sent.push(message);
      if (message.method === "terminal.attach") {
        streamId++;
        // A synchronous transport produces early data/FIN before attach resolves.
        receive(streamFrame(streamId, earlyFin ? 1 : 0, new Uint8Array([65])));
        receive(jsonFrame({ type: "response", id: message.id, result: { streamId } }));
      }
    },
    onFrame: (cb) => {
      frames.add(cb);
      return () => {
        frames.delete(cb);
      };
    },
    onState: (cb) => {
      states.add(cb);
      return () => {
        states.delete(cb);
      };
    },
  };
  const client = new RpcClient(transport, { version: "1.0.0-dev", instance: "test", client: "test" });
  const welcome = () => receive(jsonFrame({ type: "welcome", version: "1.0.0-dev", protocol: 1, daemonId: "d-1" }));
  const done: (() => void)[] = [];
  const sink = {
    write: vi.fn((_data: Uint8Array, ack: () => void) => {
      done.push(ack);
    }),
    reset: vi.fn(),
  };
  const linkStates: LinkState[] = [];
  const link = new TerminalLink(client, {
    id: "t1",
    readOnly: false,
    sink,
    onState: (s) => linkStates.push(s),
    onInputDropped: vi.fn(),
  });
  return { client, link, emit, receive, welcome, sent, done, sink, linkStates };
}
test("actual RpcClient early data and reset ordering reattach safely without stale credit", async () => {
  const f = realClientFixture();
  f.client.start();
  await f.link.open(80, 24);
  f.welcome();
  await flush();
  expect(f.linkStates.at(-1)).toBe("live");
  expect(f.sink.write).toHaveBeenCalledOnce();
  expect(f.sent.filter((m) => m.type === "streamCredit")).toHaveLength(0);
  f.emit("closed");
  expect(f.linkStates.at(-1)).toBe("lost");
  f.emit("open");
  f.welcome();
  await flush();
  expect(f.linkStates.at(-1)).toBe("live");
  expect(f.sink.reset).toHaveBeenCalledOnce();
  f.done[0]!();
  expect(f.sent.filter((m) => m.type === "streamCredit")).toHaveLength(0);
  f.done[1]!();
  expect(f.sent.filter((m) => m.type === "streamCredit")).toEqual([{ type: "streamCredit", stream: 2, bytes: 1 }]);
  f.link.dispose();
  f.client.stop();
});
test("actual RpcClient buffered FIN cannot set a terminal live after onEnd", async () => {
  const f = realClientFixture(true);
  f.client.start();
  f.welcome();
  await f.link.open(80, 24);
  expect(f.sink.write).toHaveBeenCalledOnce();
  expect(f.linkStates).not.toContain("live");
  await vi.advanceTimersByTimeAsync(10000);
  expect(f.sent.filter((m) => m.method === "terminal.attach")).toHaveLength(2);
  expect(f.linkStates.at(-1)).toBe("lost");
  f.link.dispose();
  f.client.stop();
});
