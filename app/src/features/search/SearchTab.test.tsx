import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { RpcError } from "../../rpc/client";
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

async function searchFailing(code: ConstructorParameters<typeof RpcError>[0]) {
  const wire = new NotesWire();
  wire.handlers["search.saved"] = () => {
    throw new RpcError(code);
  };
  vi.spyOn(wire.client, "openStream").mockRejectedValue(new RpcError(code));
  render(
    <RpcProvider client={wire.client}>
      <SearchPage onOpenNote={() => {}} />
    </RpcProvider>,
  );
  await userEvent.type(screen.getByRole("textbox", { name: "검색어" }), "owned");
  await userEvent.click(screen.getByRole("button", { name: "검색" }));
}

test("search turned off by a config error says how to turn it back on, with no retry that cannot succeed", async () => {
  await searchFailing("search.disabled");
  // The search itself and the saved searches both fail with it, and both say what to do.
  await waitFor(() => expect(screen.getAllByRole("alert")).toHaveLength(2));
  for (const alert of screen.getAllByRole("alert")) {
    expect(alert).toHaveTextContent("검색 설정을 읽지 못해 검색을 껐습니다.");
    expect(alert).toHaveTextContent("설정 → 앱 진단 → [다시 설정]을 눌러 주세요.");
  }
  expect(screen.queryByRole("button", { name: "다시 시도" })).toBeNull();
});

test("any other search failure shows the message of its code with its action and still offers a retry", async () => {
  await searchFailing("search.invalid_regex");
  await waitFor(() => expect(screen.getAllByRole("alert")).toHaveLength(2));
  for (const alert of screen.getAllByRole("alert")) {
    expect(alert).toHaveTextContent("정규식이 올바르지 않습니다.");
    expect(alert).toHaveTextContent("괄호와 특수 문자를 확인해 주세요.");
  }
  expect(screen.getByRole("button", { name: "다시 시도" })).toBeEnabled();
});
