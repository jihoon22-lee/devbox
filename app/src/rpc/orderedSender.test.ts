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
