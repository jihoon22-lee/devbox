import { decodeFrame, FRAME, jsonFrame, streamFrame } from "./frame";
import { createOrderedSender } from "./orderedSender";

function deferred() {
  let resolve!: () => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<void>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const flush = async () => {
  await Promise.resolve();
  await Promise.resolve();
};
test("sends FIFO with only one active invocation", async () => {
  const sender = createOrderedSender(vi.fn());
  const first = deferred();
  const second = vi.fn().mockResolvedValue(undefined);
  sender.enqueue(new Uint8Array(1), () => first.promise);
  sender.enqueue(new Uint8Array(1), second);
  expect(second).not.toHaveBeenCalled();
  first.resolve();
  await flush();
  expect(second).toHaveBeenCalledOnce();
});
test("ambiguous failure drops waiting and future writes without retry", async () => {
  const failure = vi.fn();
  const sender = createOrderedSender(failure);
  const first = deferred();
  const send = vi.fn(() => first.promise);
  const later = vi.fn().mockResolvedValue(undefined);
  sender.enqueue(new Uint8Array(1), send);
  sender.enqueue(new Uint8Array(1), later);
  first.reject(new Error("unknown delivery"));
  await flush();
  sender.enqueue(new Uint8Array(1), later);
  expect(send).toHaveBeenCalledOnce();
  expect(later).not.toHaveBeenCalled();
  expect(failure).toHaveBeenCalledOnce();
});
test("bounds frames INCLUDING the active invocation and frees completed capacity", async () => {
  const failure = vi.fn();
  const sender = createOrderedSender(failure);
  const first = deferred();
  sender.enqueue(new Uint8Array(1), () => first.promise);
  for (let i = 0; i < 255; i++) sender.enqueue(new Uint8Array(1), () => new Promise(() => {}));
  expect(failure).not.toHaveBeenCalled();
  first.resolve();
  await flush();
  sender.enqueue(new Uint8Array(1), vi.fn());
  expect(failure).not.toHaveBeenCalled();
  sender.enqueue(new Uint8Array(1), vi.fn());
  expect(failure).toHaveBeenCalledOnce();
});
test("bounds bytes including active bytes at exactly 8 MiB", () => {
  const failure = vi.fn();
  const sender = createOrderedSender(failure);
  sender.enqueue(new Uint8Array(8 * 1024 * 1024), () => new Promise(() => {}));
  expect(failure).not.toHaveBeenCalled();
  sender.enqueue(new Uint8Array(1), vi.fn());
  expect(failure).toHaveBeenCalledOnce();
});
test.each([true, false])("disposal ignores late resolution/rejection (%s)", async (success) => {
  const failure = vi.fn();
  const sender = createOrderedSender(failure);
  const first = deferred();
  const later = vi.fn();
  sender.enqueue(new Uint8Array(1), () => first.promise);
  sender.enqueue(new Uint8Array(1), later);
  sender.dispose();
  if (success) first.resolve();
  else first.reject(new Error("closed"));
  await flush();
  sender.enqueue(new Uint8Array(1), later);
  expect(later).not.toHaveBeenCalled();
  expect(failure).not.toHaveBeenCalled();
});
test("synchronous invocation errors fail once", () => {
  const failure = vi.fn();
  const sender = createOrderedSender(failure);
  sender.enqueue(new Uint8Array(1), () => {
    throw new Error("bridge closed");
  });
  expect(failure).toHaveBeenCalledOnce();
});

test("tiny consumed chunks coalesce queued credits by stream without overflowing", async () => {
  const failure = vi.fn();
  const sender = createOrderedSender(failure);
  const first = deferred();
  const delivered: Uint8Array[] = [];
  const send = vi.fn(async (frame: Uint8Array) => {
    delivered.push(frame);
  });
  sender.enqueue(jsonFrame({ blocked: true }), () => first.promise);
  for (let i = 0; i < 2000; i++) {
    expect(sender.enqueue(jsonFrame({ type: "streamCredit", stream: i % 2, bytes: 1 }, FRAME.control), send)).toBe(
      true,
    );
  }
  expect(failure).not.toHaveBeenCalled();
  first.resolve();
  await flush();
  expect(delivered.map((frame) => JSON.parse(new TextDecoder().decode(decodeFrame(frame).body)))).toEqual([
    { type: "streamCredit", stream: 0, bytes: 1000 },
    { type: "streamCredit", stream: 1, bytes: 1000 },
  ]);
});

test("input, FIN, JSON and full close remain FIFO barriers between credit batches", async () => {
  const sender = createOrderedSender(vi.fn());
  const first = deferred();
  const delivered: Uint8Array[] = [];
  const send = async (frame: Uint8Array) => {
    delivered.push(frame);
  };
  const credit = (bytes: number) => jsonFrame({ type: "streamCredit", stream: 9, bytes }, FRAME.control);
  sender.enqueue(jsonFrame({ blocked: true }), () => first.promise);
  const barriers = [
    streamFrame(9, 0, Uint8Array.of(113)),
    streamFrame(9, 1, new Uint8Array()),
    jsonFrame({ request: true }),
    jsonFrame({ type: "streamClose", stream: 9 }, FRAME.control),
  ];
  for (const barrier of barriers) {
    sender.enqueue(credit(1), send);
    sender.enqueue(credit(2), send);
    sender.enqueue(barrier, send);
  }
  first.resolve();
  for (let i = 0; i < 10; i++) await flush();
  expect(delivered).toEqual(barriers.flatMap((barrier) => [credit(3), barrier]));
});

test("a merged credit never exceeds the stream window", async () => {
  const sender = createOrderedSender(vi.fn());
  const first = deferred();
  const delivered: Uint8Array[] = [];
  const send = async (frame: Uint8Array) => {
    delivered.push(frame);
  };
  sender.enqueue(jsonFrame({ blocked: true }), () => first.promise);
  for (let i = 0; i < 32; i++)
    sender.enqueue(jsonFrame({ type: "streamCredit", stream: 9, bytes: 8192 }, FRAME.control), send);
  sender.enqueue(jsonFrame({ type: "streamCredit", stream: 9, bytes: 1 }, FRAME.control), send);
  first.resolve();
  await flush();
  expect(delivered.map((frame) => JSON.parse(new TextDecoder().decode(decodeFrame(frame).body)).bytes)).toEqual([
    262144, 1,
  ]);
});
