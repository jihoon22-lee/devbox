import { type RpcClient, RpcError } from "./client";
import { readProblemPages } from "./problemPages";

const row = { file: "x.rs", line: 1, col: null, severity: "error" as const, message: "failed", source: "run:r" };
test("aggregates more than one hundred problems without publishing partial pages", async () => {
  const call = vi
    .fn()
    .mockResolvedValueOnce({
      rev: "1",
      items: Array.from({ length: 100 }, (_, line) => ({ ...row, line: line + 1 })),
      nextCursor: { rev: "1", offset: 100 },
      truncated: false,
    })
    .mockResolvedValueOnce({ rev: "1", items: [{ ...row, line: 101 }], nextCursor: null, truncated: false });
  const result = await readProblemPages(
    { call } as unknown as RpcClient,
    { projectId: "p", limit: 100 },
    new AbortController().signal,
  );
  expect(result.items).toHaveLength(101);
  expect(result.nextCursor).toBeNull();
  expect(call.mock.calls[1]?.[1].cursor).toEqual({ rev: "1", offset: 100 });
});
test("revision drift restarts and repeated invalid cursors fail after three attempts", async () => {
  const call = vi
    .fn()
    .mockResolvedValueOnce({ rev: "1", items: [row], nextCursor: { rev: "1", offset: 1 }, truncated: false })
    .mockResolvedValueOnce({ rev: "2", items: [row], nextCursor: null, truncated: false })
    .mockRejectedValue(new RpcError("observe.invalid_cursor"));
  await expect(
    readProblemPages({ call } as unknown as RpcClient, { projectId: "p" }, new AbortController().signal),
  ).rejects.toMatchObject({ code: "observe.invalid_cursor" });
  expect(call).toHaveBeenCalledTimes(4);
});
test("abort during an invalid cursor response never starts another snapshot", async () => {
  const controller = new AbortController();
  const call = vi.fn(async () => {
    controller.abort();
    throw new RpcError("observe.invalid_cursor");
  });
  await expect(
    readProblemPages({ call } as unknown as RpcClient, { projectId: "p" }, controller.signal),
  ).rejects.toMatchObject({ code: "observe.invalid_cursor" });
  expect(call).toHaveBeenCalledTimes(1);
});
