import { beforeEach, expect, it, vi } from "vitest";
import type { OutputBatch } from "./terminalReplay";
const native = vi.hoisted(() => ({
  channel: null as null | { onmessage: (value: OutputBatch) => void },
  subscribe: vi.fn(),
  call: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage = (_value: unknown) => {};
  },
}));
vi.mock("../api-transport", () => ({
  terminalCall: (...args: unknown[]) => native.call(...args),
  subscribeOutput: (...args: unknown[]) => {
    native.channel = args[2] as typeof native.channel;
    return native.subscribe(...args);
  },
}));
import { streamTerminalOutput } from "./terminalStream";
const batch = (cursor = 1, closed = false): OutputBatch => ({
  frames: [{ sequence: cursor, data: `text-${cursor}` }],
  cursor,
  closed,
  truncated: false,
  more: false,
});
const settle = async () => {
  for (let i = 0; i < 12; i++) await Promise.resolve();
};
beforeEach(() => {
  native.channel = null;
  native.call.mockReset().mockResolvedValue(null);
  native.subscribe.mockReset().mockResolvedValue({ subscriptionId: "sub-1" });
});
it("sends no idle calls, waits for xterm, acknowledges each cursor and disposes once", async () => {
  let drained!: () => void;
  const write = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        drained = resolve;
      }),
  );
  const closed = vi.fn(),
    failed = vi.fn();
  const dispose = streamTerminalOutput("s", write, closed, failed);
  await settle();
  expect(native.call).not.toHaveBeenCalled();
  native.channel!.onmessage(batch());
  await settle();
  expect(write).toHaveBeenCalledWith("text-1", false);
  expect(native.call).not.toHaveBeenCalled();
  drained();
  await settle();
  expect(native.call).toHaveBeenCalledWith("ack_terminal_output", { subscriptionId: "sub-1", cursor: 1 });
  dispose();
  dispose();
  await settle();
  expect(native.call.mock.calls.filter(([method]) => method === "unsubscribe_terminal_output")).toHaveLength(1);
  expect(closed).not.toHaveBeenCalled();
  expect(failed).not.toHaveBeenCalled();
});
it("buffers the first message before subscribe reply and closes only after the final write", async () => {
  let subscribed!: (value: { subscriptionId: string }) => void;
  native.subscribe.mockImplementation(
    () =>
      new Promise((resolve) => {
        subscribed = resolve;
      }),
  );
  let drained!: () => void;
  const write = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        drained = resolve;
      }),
  );
  const closed = vi.fn();
  const failed = vi.fn();
  streamTerminalOutput("s", write, closed, failed);
  native.channel!.onmessage(batch(1, true));
  await settle();
  expect(write).not.toHaveBeenCalled();
  subscribed({ subscriptionId: "sub-1" });
  await settle();
  expect(closed).not.toHaveBeenCalled();
  drained();
  await settle();
  expect(closed).toHaveBeenCalledTimes(1);
  expect(failed).not.toHaveBeenCalled();
  expect(native.call.mock.calls.some(([method]) => method === "ack_terminal_output")).toBe(false);
});
it("disposal before subscribe reply drops output and releases the late subscription", async () => {
  let subscribed!: (value: { subscriptionId: string }) => void;
  native.subscribe.mockImplementation(
    () =>
      new Promise((resolve) => {
        subscribed = resolve;
      }),
  );
  const write = vi.fn(),
    closed = vi.fn(),
    failed = vi.fn();
  const dispose = streamTerminalOutput("s", write, closed, failed);
  native.channel!.onmessage(batch());
  dispose();
  subscribed({ subscriptionId: "sub-1" });
  await settle();
  expect(write).not.toHaveBeenCalled();
  expect(closed).not.toHaveBeenCalled();
  expect(failed).not.toHaveBeenCalled();
  expect(native.call).toHaveBeenCalledExactlyOnceWith("unsubscribe_terminal_output", { subscriptionId: "sub-1" });
});
it("rejects invalid batches and a peer that overruns the one-batch window", async () => {
  const failed = vi.fn(),
    write = vi.fn();
  streamTerminalOutput("s", write, vi.fn(), failed);
  await settle();
  native.channel!.onmessage(batch(5));
  await settle();
  expect(failed).toHaveBeenCalledTimes(1);
  expect(write).not.toHaveBeenCalled();
  native.call.mockClear();
  const secondFailure = vi.fn();
  const dispose = streamTerminalOutput("s", () => new Promise(() => {}), vi.fn(), secondFailure);
  await settle();
  native.channel!.onmessage(batch());
  native.channel!.onmessage(batch(2));
  await settle();
  expect(secondFailure).toHaveBeenCalledTimes(1);
  expect(native.call).toHaveBeenCalledWith("unsubscribe_terminal_output", { subscriptionId: "sub-1" });
  dispose();
});
it("reports subscription, write and acknowledgement failures without unhandled rejections", async () => {
  for (const stage of ["subscribe", "write", "ack"]) {
    native.subscribe.mockResolvedValue({ subscriptionId: "sub-1" });
    native.call.mockResolvedValue(null);
    const failed = vi.fn();
    const write = vi.fn().mockResolvedValue(undefined);
    if (stage === "subscribe") native.subscribe.mockRejectedValue(new Error("offline"));
    if (stage === "write") write.mockRejectedValue(new Error("closed xterm"));
    if (stage === "ack") native.call.mockRejectedValue(new Error("expired"));
    const dispose = streamTerminalOutput("s", write, vi.fn(), failed);
    await settle();
    if (stage !== "subscribe") native.channel!.onmessage(batch());
    await settle();
    expect(failed).toHaveBeenCalledTimes(1);
    dispose();
  }
});

it("queues the next batch when it races the previous acknowledgement reply", async () => {
  let acked!: () => void;
  native.call.mockImplementation((method: string) =>
    method === "ack_terminal_output"
      ? new Promise<void>((resolve) => {
          acked = resolve;
        })
      : Promise.resolve(),
  );
  const writes = vi.fn().mockResolvedValue(undefined),
    closed = vi.fn(),
    failed = vi.fn();
  streamTerminalOutput("s", writes, closed, failed);
  await settle();
  native.channel!.onmessage(batch());
  await settle();
  native.channel!.onmessage(batch(2, true));
  await settle();
  expect(writes).toHaveBeenCalledTimes(1);
  acked();
  await settle();
  expect(writes).toHaveBeenCalledTimes(2);
  expect(closed).toHaveBeenCalledTimes(1);
  expect(failed).not.toHaveBeenCalled();
});
it("does not acknowledge or report closure after disposal during a pending write", async () => {
  let drained!: () => void;
  const closed = vi.fn(),
    failed = vi.fn();
  const dispose = streamTerminalOutput(
    "s",
    () =>
      new Promise<void>((resolve) => {
        drained = resolve;
      }),
    closed,
    failed,
  );
  await settle();
  native.channel!.onmessage(batch(1, true));
  await settle();
  dispose();
  drained();
  await settle();
  expect(closed).not.toHaveBeenCalled();
  expect(failed).not.toHaveBeenCalled();
  expect(native.call.mock.calls.some(([method]) => method === "ack_terminal_output")).toBe(false);
});
