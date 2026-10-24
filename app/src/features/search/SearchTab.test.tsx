import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import type { StreamHandlers } from "../../rpc/client-streams";
import type { SearchChunk } from "../../rpc/gen/rpc";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesWire } from "../notes/test-wire";
import { SearchPage } from "./SearchPage";

afterEach(cleanup);
const chunk = (rev: string): SearchChunk => ({
  scope: "notes",
  rev,
  done: true,
  truncated: false,
  items: [{ root: "vault", path: "a.md", title: "Alpha", line: null, snippet: "owned result", rangesUtf16: [] }],
});

test("keeps_results_and_selection_while_refreshing", async () => {
  const wire = new NotesWire();
  wire.handlers["search.saved"] = () => ({ rev: "1", items: [] });
  const streams: StreamHandlers[] = [];
  vi.spyOn(wire.client, "openStream").mockImplementation(async (_m, _p, handlers) => {
    streams.push(handlers);
    return { streamId: streams.length, result: { streamId: streams.length }, write() {}, end() {} };
  });
  render(
    <RpcProvider client={wire.client}>
      <SearchPage onOpenNote={() => {}} />
    </RpcProvider>,
  );
  await userEvent.type(screen.getByRole("textbox", { name: "검색어" }), "owned");
  await userEvent.click(screen.getByRole("button", { name: "검색" }));
  await waitFor(() => expect(streams).toHaveLength(1));
  act(() => streams[0]!.onData(new TextEncoder().encode(`${JSON.stringify(chunk("2"))}\n`), () => {}));
  const row = await screen.findByRole("button", { name: /Alpha.*a.md/ });
  await userEvent.click(row);
  expect(row).toHaveAttribute("aria-pressed", "true");
  act(() => {
    for (let i = 0; i < 3; i++) wire.emit("search.changed", { rev: "2" }, "2");
  });
  expect(streams).toHaveLength(1);
  act(() => wire.emit("search.changed", { rev: "3" }, "3"));
  await waitFor(() => expect(streams).toHaveLength(2));
  expect(screen.getByRole("button", { name: /Alpha.*a.md/ })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByText("owned result")).toBeVisible();
  act(() => streams[1]!.onData(new TextEncoder().encode(`${JSON.stringify(chunk("3"))}\n`), () => {}));
  expect(screen.getByRole("button", { name: /Alpha.*a.md/ })).toHaveAttribute("aria-pressed", "true");
});
