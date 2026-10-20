import type { RpcClient } from "../../../rpc/client";
import type { LogLine, SearchCursor } from "../../../rpc/gen/rpc";
import { quoteFailureLogs, readFailureLogs } from "./failureLogs";

const row = (text: string, runId = "run-1"): LogLine => ({
  text,
  src: { kind: "run", runId },
  ts: "1",
  stream: "stdout",
});
const cursor = (offset: number) => ({ sources: [{ offset }] }) as unknown as SearchCursor;
test("quotes only the newest 200 server rows in chronological order", () => {
  const text = quoteFailureLogs(
    "run-1",
    Array.from({ length: 205 }, (_, i) => row(`line-${205 - i}`)),
  );
  const quoted = text.split("\n").filter((line) => /^> line-/.test(line));
  expect(quoted).toHaveLength(200);
  expect(quoted[0]).toBe("> line-6");
  expect(quoted.at(-1)).toBe("> line-205");
});
test("long UTF-8 logs include explicit omission and remain under the agent input budget", () => {
  const text = quoteFailureLogs("run-1", [row("😀한글".repeat(10000))]);
  expect(new TextEncoder().encode(text).length).toBeLessThanOrEqual(64 * 1024);
  expect(text).toContain("일부 로그 생략");
  expect(text).not.toContain("�");
  expect(new TextDecoder("utf-8", { fatal: true }).decode(new TextEncoder().encode(text))).toBe(text);
});
test("older long rows are omitted and quote/header bytes count in the limit", () => {
  const text = quoteFailureLogs("run-1", [row("newest [REDACTED]"), row("한".repeat(30000))]);
  expect(text).toContain("> newest [REDACTED]");
  expect(text).toContain("일부 로그 생략");
  expect(new TextEncoder().encode(text).length).toBeLessThanOrEqual(65536);
});
test("follows byte-limited and empty pages using one run source and no UI search filter", async () => {
  const call = vi
    .fn()
    .mockResolvedValueOnce({ items: [row("new"), row("foreign", "other")], nextCursor: cursor(1) })
    .mockResolvedValueOnce({ items: [], nextCursor: cursor(2) })
    .mockResolvedValueOnce({ items: [row("old [REDACTED]")], nextCursor: null });
  const text = await readFailureLogs({ call } as unknown as RpcClient, "run-1", new AbortController().signal);
  expect(text.indexOf("old [REDACTED]")).toBeLessThan(text.indexOf("> new"));
  expect(text).not.toContain("foreign");
  for (const [, params] of call.mock.calls)
    expect(params).toMatchObject({ sources: [{ kind: "run", runId: "run-1" }], query: "", regex: false });
  expect(call.mock.calls[1]?.[1]).toMatchObject({ before: cursor(1), limit: 199 });
  expect(call).toHaveBeenCalledTimes(3);
});
test("cancels before reading and rejects repeated cursors instead of sending partial logs", async () => {
  const call = vi.fn().mockResolvedValue({ items: [], nextCursor: cursor(1) });
  const client = { call } as unknown as RpcClient;
  const controller = new AbortController();
  controller.abort();
  await expect(readFailureLogs(client, "run-1", controller.signal)).rejects.toThrow();
  expect(call).not.toHaveBeenCalled();
  await expect(readFailureLogs(client, "run-1", new AbortController().signal)).rejects.toThrow("반복");
  expect(call).toHaveBeenCalledTimes(2);
});
test("stops fetching at 200 rows even when history continues", async () => {
  const call = vi
    .fn()
    .mockResolvedValue({ items: Array.from({ length: 200 }, (_, i) => row(String(i))), nextCursor: cursor(1) });
  const text = await readFailureLogs({ call } as unknown as RpcClient, "run-1", new AbortController().signal);
  expect(call).toHaveBeenCalledTimes(1);
  expect(text.split("\n").filter((line) => /^> \d+$/.test(line))).toHaveLength(200);
});
