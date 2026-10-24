import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, test, vi } from "vitest";
import type { StreamHandlers } from "../../rpc/client-streams";
import type { SearchQuery } from "../../rpc/gen/rpc";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesWire } from "../notes/test-wire";
import { useSearchStream } from "./useSearchStream";

const query: SearchQuery = {
  text: "a",
  scopes: ["notes"],
  projectIds: null,
  regex: false,
  caseSensitive: false,
  limit: 20,
};
afterEach(cleanup);
const data = (text: string, rev = "1") =>
  new TextEncoder().encode(
    `${JSON.stringify({ scope: "notes", rev, done: true, truncated: false, items: [{ root: "vault", path: "a.md", title: text, line: null, snippet: text, rangesUtf16: [] }] })}\n`,
  );
function setup() {
  const wire = new NotesWire();
  const streams: { handlers: StreamHandlers; end: ReturnType<typeof vi.fn> }[] = [];
  vi.spyOn(wire.client, "openStream").mockImplementation(async (_method, _params, handlers) => {
    const end = vi.fn();
    streams.push({ handlers, end });
    return { streamId: streams.length, result: { streamId: streams.length }, end, write() {} };
  });
  const hook = renderHook(({ params }) => useSearchStream(params), {
    initialProps: { params: query },
    wrapper: ({ children }: { children: ReactNode }) => <RpcProvider client={wire.client}>{children}</RpcProvider>,
  });
  return { ...hook, wire, streams };
}
test("split UTF-8 JSON frames are decoded and every frame acknowledged", async () => {
  const { result, streams } = setup();
  await waitFor(() => expect(streams).toHaveLength(1));
  const bytes = data("한글");
  const marker = bytes.indexOf(0xed) + 1;
  const ack = vi.fn();
  act(() => streams[0]!.handlers.onData(bytes.subarray(0, marker), ack));
  expect(result.current.groups.notes).toEqual([]);
  act(() => streams[0]!.handlers.onData(bytes.subarray(marker), ack));
  expect(result.current.groups.notes[0]?.snippet).toBe("한글");
  expect(ack).toHaveBeenCalledTimes(2);
});
test("new query cancels the previous stream and ignores its late data", async () => {
  const { result, rerender, streams } = setup();
  await waitFor(() => expect(streams).toHaveLength(1));
  rerender({ params: { ...query, text: "b" } });
  await waitFor(() => expect(streams).toHaveLength(2));
  expect(streams[0]!.end).toHaveBeenCalled();
  act(() => streams[0]!.handlers.onData(data("stale"), () => {}));
  expect(result.current.groups.notes).toEqual([]);
  act(() => streams[1]!.handlers.onData(data("current"), () => {}));
  expect(result.current.groups.notes[0]?.snippet).toBe("current");
});
test("duplicate newer events during refresh open one stream and failures retain the last success", async () => {
  const { result, streams, wire } = setup();
  await waitFor(() => expect(streams).toHaveLength(1));
  act(() => streams[0]!.handlers.onData(data("keep", "2"), () => {}));
  act(() => {
    wire.emit("search.changed", { rev: "3" }, "3");
    wire.emit("search.changed", { rev: "3" }, "3");
  });
  await waitFor(() => expect(streams).toHaveLength(2));
  act(() => streams[1]!.handlers.onData(new TextEncoder().encode("bad json\n"), () => {}));
  expect(result.current.error).toBeDefined();
  expect(result.current.groups.notes[0]?.snippet).toBe("keep");
  act(() => streams[1]!.handlers.onData(data("bad late", "4"), () => {}));
  expect(result.current.groups.notes[0]?.snippet).toBe("keep");
});
