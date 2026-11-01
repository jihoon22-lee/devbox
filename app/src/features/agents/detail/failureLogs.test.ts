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
/** What the daemon refuses in a send, and CR as well: a quote only breaks lines with LF. */
const controls = (text: string) =>
  Array.from(text).filter((character) => {
    const code = character.codePointAt(0) ?? 0;
    return (code < 32 && code !== 9 && code !== 10) || (code >= 127 && code <= 159);
  });
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
test("terminal colors and keys in a row are sent as plain text", () => {
  const colored = quoteFailureLogs("run-1", [row("\x1b[31mFAIL\x1b[0m")]);
  expect(colored.split("\n").slice(1)).toEqual(["> FAIL", ""]);
  // In the agent's terminal this would end the paste, erase the prompt box, type a command and
  // press Enter.
  const keys = quoteFailureLogs("run-1", [row("\x1b[201~\x15!rm -rf ~\r")]);
  expect(keys.split("\n").slice(1)).toEqual(["> �!rm -rf ~�", ""]);
  for (const text of [colored, keys]) {
    expect(text).not.toContain("\x1b");
    expect(controls(text)).toEqual([]);
  }
});
test("a row cut to fit is plain text too and counts its replacement characters", () => {
  const colored = quoteFailureLogs("run-1", [row(`\x1b[31m${"한".repeat(30000)}\x15\x1b[0m`)]);
  expect(colored).toContain("일부 로그 생략");
  expect(colored).toMatch(/한�\n$/);
  // Each control code becomes a 3-byte "�", which must fit the budget as well.
  const replaced = quoteFailureLogs("run-1", [row("\x15".repeat(30000))]);
  expect(new TextEncoder().encode(replaced).length).toBeLessThanOrEqual(64 * 1024);
  for (const text of [colored, replaced]) {
    expect(text).not.toContain("\x1b");
    expect(controls(text)).toEqual([]);
  }
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
