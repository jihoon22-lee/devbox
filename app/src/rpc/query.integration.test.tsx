import { useQueryClient } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { type ReactNode, StrictMode } from "react";
import { RpcClient, type Transport, type TransportState } from "./client";
import { useConnection } from "./connection";
import { decodeFrame, jsonFrame } from "./frame";
import { confirmPolicy, useRpcMutation, useRpcQuery, useTopicInvalidation } from "./query";
import { RpcProvider } from "./RpcProvider";

class Wire implements Transport {
  sent: Record<string, unknown>[] = [];
  frames = new Set<(frame: Uint8Array) => void>();
  states = new Set<(state: TransportState) => void>();
  connect = vi.fn(() => this.emit("open"));
  close = vi.fn(() => this.emit("closed"));
  send(frame: Uint8Array) {
    this.sent.push(JSON.parse(new TextDecoder().decode(decodeFrame(frame).body)));
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
  deliver(msg: unknown) {
    for (const cb of this.frames) cb(jsonFrame(msg));
  }
  welcome(daemonId = "one") {
    this.deliver({ type: "welcome", protocol: 1, version: "1.0.0-dev", daemonId });
  }
  requests(method = "projects.list") {
    return this.sent.filter((m) => m.method === method);
  }
  request(method = "projects.list", index = 0) {
    const request = this.requests(method).at(index);
    if (!request) throw new Error(`Missing ${method} request at index ${index}`);
    return request;
  }
  ack() {
    const sub = this.sent.filter((m) => m.type === "subscribe").at(-1)!;
    this.deliver({ type: "subscribed", id: sub.id, subId: 7, rev: "1" });
  }
  reply(id: unknown, result: unknown) {
    this.deliver({ type: "response", id, result });
  }
}
function setup(enabled = true) {
  const wire = new Wire();
  const client = new RpcClient(wire, { version: "1.0.0-dev", instance: "integration", client: "test" });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <StrictMode>
      <RpcProvider client={client}>{children}</RpcProvider>
    </StrictMode>
  );
  const hook = renderHook(
    ({ enabled }) => {
      const list = useRpcQuery("projects.list", {}, { enabled });
      const ping = useRpcQuery("system.ping", {});
      const qc = useQueryClient();
      useTopicInvalidation("projects.changed", [["projects.list", {}]]);
      return { list, ping, qc, connection: useConnection() };
    },
    { wrapper, initialProps: { enabled } },
  );
  return { wire, client, ...hook };
}
async function ready(wire: Wire) {
  await act(async () => {
    wire.welcome();
    wire.ack();
  });
  await waitFor(() => expect(wire.requests()).toHaveLength(1));
}
async function respond(wire: Wire, rev: string) {
  await act(async () => wire.reply(wire.request("projects.list", -1).id, { rev, items: [] }));
}

test("ready and options gate queries; reconnect waits for ACK and refetches all active queries once", async () => {
  const { wire, result, rerender, unmount } = setup();
  expect(wire.requests()).toHaveLength(0);
  await ready(wire);
  await respond(wire, "8");
  await act(async () =>
    wire.reply(wire.request("system.ping").id, { daemonId: "one", version: "1.0.0-dev", uptimeMs: 1 }),
  );
  await waitFor(() => expect(result.current.list.data?.rev).toBe("8"));
  result.current.qc.setQueryData(["inactive"], { rev: "2" });
  await act(async () => wire.emit("closed"));
  expect(result.current.list.data?.rev).toBe("8");
  await act(async () => {
    wire.emit("open");
    wire.welcome();
  });
  expect(wire.requests()).toHaveLength(1);
  await act(async () => wire.ack());
  await waitFor(() => expect(wire.requests()).toHaveLength(2));
  expect(wire.requests("system.ping")).toHaveLength(2);
  expect(result.current.qc.getQueryState(["inactive"])?.isInvalidated).toBe(true);
  await respond(wire, "9");
  rerender({ enabled: false });
  await act(async () => wire.deliver({ type: "resync", subId: 7 }));
  expect(wire.requests()).toHaveLength(2);
  unmount();
  expect(wire.frames.size).toBe(0);
  expect(wire.states.size).toBe(0);
});

test("newer event during initial request replaces it; late response cannot undo a newer cache", async () => {
  const { wire, result } = setup();
  await ready(wire);
  const old = wire.request().id;
  await act(async () => wire.deliver({ type: "event", subId: 7, rev: "12", payload: { rev: "12" } }));
  await waitFor(() => expect(wire.requests()).toHaveLength(2));
  await act(async () => wire.reply(old, { rev: "1", items: [] }));
  await respond(wire, "12");
  await waitFor(() => expect(result.current.list.data?.rev).toBe("12"));
  await act(async () => wire.deliver({ type: "event", subId: 7, rev: "13", payload: { rev: "13" } }));
  await waitFor(() => expect(wire.requests()).toHaveLength(3));
  await act(async () => result.current.qc.setQueryData(["projects.list", {}], { rev: "15", items: [] }));
  await respond(wire, "13");
  await waitFor(() => expect(result.current.list.fetchStatus).toBe("idle"));
  expect(result.current.list.data?.rev).toBe("15");
  await act(async () => wire.deliver({ type: "event", subId: 7, rev: "15", payload: { rev: "15" } }));
  expect(wire.requests()).toHaveLength(3);
});

test("daemon reset clears inactive and mutation cache and mounted observers reattach", async () => {
  const { wire, result } = setup();
  await ready(wire);
  await respond(wire, "90");
  await waitFor(() => expect(result.current.list.data?.rev).toBe("90"));
  result.current.qc.setQueryData(["inactive"], "old");
  result.current.qc.getMutationCache().build(result.current.qc, { mutationKey: ["old"] });
  await act(async () => wire.emit("closed"));
  await act(async () => {
    wire.emit("open");
    wire.welcome("two");
  });
  expect(result.current.qc.getQueryData(["inactive"])).toBeUndefined();
  expect(result.current.qc.getMutationCache().getAll()).toHaveLength(0);
  expect(result.current.list.data).toBeUndefined();
  await act(async () => wire.ack());
  await waitFor(() => expect(wire.requests()).toHaveLength(2));
  await respond(wire, "1");
  await waitFor(() => expect(result.current.list.data?.rev).toBe("1"));
  await act(async () => wire.deliver({ type: "event", subId: 7, rev: "2", payload: { rev: "2" } }));
  await waitFor(() => expect(wire.requests()).toHaveLength(3));
  await respond(wire, "2");
  await waitFor(() => expect(result.current.list.data?.rev).toBe("2"));
});

test("mutation invocations have distinct IDs, never retry, and expose generated confirmation policy", async () => {
  const wire = new Wire();
  const client = new RpcClient(wire, { version: "1.0.0-dev", instance: "mutations", client: "test" });
  const wrapper = ({ children }: { children: ReactNode }) => <RpcProvider client={client}>{children}</RpcProvider>;
  const { result } = renderHook(() => useRpcMutation("projects.add"), { wrapper });
  await act(async () => wire.welcome());
  act(() => result.current.mutate({ path: "/owned-fixture" }));
  await waitFor(() => expect(wire.requests("projects.add")).toHaveLength(1));
  const first = wire.request("projects.add");
  await act(async () => wire.deliver({ type: "response", id: first.id, error: { code: "internal" } }));
  await waitFor(() => expect(result.current.isError).toBe(true));
  expect(wire.requests("projects.add")).toHaveLength(1);
  act(() => result.current.mutate({ path: "/owned-fixture" }));
  await waitFor(() => expect(wire.requests("projects.add")).toHaveLength(2));
  expect(first.clientRequestId).toEqual(expect.any(String));
  expect(wire.request("projects.add", 1).clientRequestId).not.toBe(first.clientRequestId);
  expect(confirmPolicy("projects.remove")).toBe("always");
  expect(confirmPolicy("projects.add")).toBe("none");
});

test("explicitly disabled queries stay disabled after initial ACK and reconnect", async () => {
  const { wire, result } = setup(false);
  await act(async () => {
    wire.welcome();
    wire.ack();
  });
  expect(wire.requests()).toHaveLength(0);
  await waitFor(() => expect(wire.requests("system.ping")).toHaveLength(1));
  await act(async () => wire.emit("closed"));
  await act(async () => {
    wire.emit("open");
    wire.welcome();
    wire.ack();
  });
  expect(wire.requests()).toHaveLength(0);
  expect(result.current.connection.state).toBe("ready");
  expect(result.current.connection.daemonVersion).toBe("1.0.0-dev");
  expect(result.current.connection.lastReadyMs).toEqual(expect.any(Number));
});

test("old pending response cannot repopulate a new daemon cache", async () => {
  const { wire, result } = setup();
  await ready(wire);
  const oldId = wire.request().id;
  await act(async () => wire.emit("closed"));
  await act(async () => {
    wire.emit("open");
    wire.welcome("two");
    wire.ack();
  });
  await waitFor(() => expect(wire.requests()).toHaveLength(2));
  await act(async () => wire.reply(oldId, { rev: "999", items: [] }));
  expect(result.current.list.data).toBeUndefined();
  await respond(wire, "1");
  await waitFor(() => expect(result.current.list.data?.rev).toBe("1"));
});

test("changing clients disposes old transport and starts a fresh query session", async () => {
  const first = new Wire(),
    second = new Wire();
  const makeClient = (wire: Wire) => new RpcClient(wire, { version: "1.0.0-dev", instance: "swap", client: "test" });
  let current = makeClient(first);
  const wrapper = ({ children }: { children: ReactNode }) => (
    <StrictMode>
      <RpcProvider client={current}>{children}</RpcProvider>
    </StrictMode>
  );
  const { result, rerender, unmount } = renderHook(() => useRpcQuery("projects.list", {}), { wrapper });
  await act(async () => first.welcome());
  await waitFor(() => expect(first.requests()).toHaveLength(1));
  await respond(first, "9");
  await waitFor(() => expect(result.current.data?.rev).toBe("9"));
  current = makeClient(second);
  rerender();
  expect(first.frames.size).toBe(0);
  expect(first.states.size).toBe(0);
  expect(second.frames.size).toBe(1);
  expect(result.current.data).toBeUndefined();
  await act(async () => second.welcome("two"));
  await waitFor(() => expect(second.requests()).toHaveLength(1));
  await respond(second, "1");
  await waitFor(() => expect(result.current.data?.rev).toBe("1"));
  unmount();
  expect(second.frames.size).toBe(0);
});

test("cancelling a refetch preserves a newer manual cache update through events and disconnect", async () => {
  const { wire, result } = setup();
  await ready(wire);
  await respond(wire, "10");
  await waitFor(() => expect(result.current.list.data?.rev).toBe("10"));
  await act(async () => wire.deliver({ type: "event", subId: 7, rev: "11", payload: { rev: "11" } }));
  await waitFor(() => expect(wire.requests()).toHaveLength(2));
  await act(async () => result.current.qc.setQueryData(["projects.list", {}], { rev: "12", items: [] }));
  await act(async () => wire.deliver({ type: "event", subId: 7, rev: "13", payload: { rev: "13" } }));
  await waitFor(() => expect(wire.requests()).toHaveLength(3));
  expect(result.current.list.data?.rev).toBe("12");
  await act(async () => wire.emit("closed"));
  await waitFor(() => expect(result.current.list.fetchStatus).toBe("idle"));
  expect(result.current.list.data?.rev).toBe("12");
  expect(result.current.list.isError).toBe(false);
});
