import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { RpcError } from "../../rpc/client";
import type { StreamHandlers } from "../../rpc/client-streams";
import type { SearchChunk } from "../../rpc/gen/rpc";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesWire } from "../notes/test-wire";
import { useCurrentProject } from "../projects/store";
import { SearchPage } from "./SearchPage";

afterEach(() => {
  cleanup();
  useCurrentProject.setState({ id: undefined });
});
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

const project = (id: string, name: string) => ({ id, name, path: `/w/${id}`, favorite: false, addedMs: 1 });
const codeChunk = (id: string): SearchChunk => ({
  scope: "code",
  rev: "1",
  done: true,
  truncated: false,
  items: [{ root: `project:${id}`, path: "src/a.ts", title: null, line: 3, snippet: "owned code", rangesUtf16: [] }],
});
async function scoped(current: string | undefined) {
  useCurrentProject.setState({ id: current });
  const wire = new NotesWire();
  wire.handlers["search.saved"] = () => ({
    rev: "1",
    items: [{ id: "s1", name: "다른 것", query: { ...savedQuery }, createdMs: 1 }],
  });
  wire.handlers["projects.list"] = () => ({ rev: "1", items: [project("p1", "Alpha"), project("p2", "Beta")] });
  const opened: { params: unknown; handlers: StreamHandlers }[] = [];
  vi.spyOn(wire.client, "openStream").mockImplementation(async (_m, params, handlers) => {
    opened.push({ params, handlers });
    return { streamId: opened.length, result: { streamId: opened.length }, write() {}, end() {} };
  });
  render(
    <RpcProvider client={wire.client}>
      <SearchPage onOpenNote={() => {}} />
    </RpcProvider>,
  );
  await userEvent.type(screen.getByRole("textbox", { name: "검색어" }), "owned");
  return { wire, opened };
}
const savedQuery = {
  text: "saved",
  scopes: ["notes", "code"],
  projectIds: ["p2"],
  regex: false,
  caseSensitive: false,
  limit: 100,
};

test("current project scope sends projectIds [current], keeps notes, and labels code hits with the project", async () => {
  const { opened } = await scoped("p1");
  const box = await screen.findByRole("checkbox", { name: "현재 프로젝트만: Alpha" });
  expect(box).toBeChecked();
  expect(screen.getByText("노트는 프로젝트에 속하지 않아 늘 전체 노트에서 찾습니다.")).toBeVisible();
  await userEvent.click(screen.getByRole("button", { name: "검색" }));
  await waitFor(() => expect(opened).toHaveLength(1));
  expect(opened[0]!.params).toMatchObject({ projectIds: ["p1"], scopes: expect.arrayContaining(["notes"]) });
  act(() => opened[0]!.handlers.onData(new TextEncoder().encode(`${JSON.stringify(codeChunk("p1"))}\n`), () => {}));
  const row = await screen.findByRole("button", { name: /src\/a\.ts/ });
  expect(row).toHaveTextContent("Alpha");
});

test("turning the current project box off sends projectIds null", async () => {
  const { opened } = await scoped("p1");
  await userEvent.click(await screen.findByRole("checkbox", { name: "현재 프로젝트만: Alpha" }));
  expect(screen.queryByText("노트는 프로젝트에 속하지 않아 늘 전체 노트에서 찾습니다.")).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "검색" }));
  await waitFor(() => expect(opened).toHaveLength(1));
  expect(opened[0]!.params).toMatchObject({ projectIds: null });
});

test("the notes hint is the box's description", async () => {
  await scoped("p1");
  expect(await screen.findByRole("checkbox", { name: "현재 프로젝트만: Alpha" })).toHaveAccessibleDescription(
    "노트는 프로젝트에 속하지 않아 늘 전체 노트에서 찾습니다.",
  );
});

test("a scope that follows the current project moves with it", async () => {
  const { opened } = await scoped("p1");
  expect(await screen.findByRole("checkbox", { name: "현재 프로젝트만: Alpha" })).toBeChecked();
  act(() => useCurrentProject.setState({ id: "p2" }));
  expect(screen.getByRole("checkbox", { name: "현재 프로젝트만: Beta" })).toBeChecked();
  await userEvent.click(screen.getByRole("button", { name: "검색" }));
  await waitFor(() => expect(opened).toHaveLength(1));
  expect(opened[0]!.params).toMatchObject({ projectIds: ["p2"] });
});

test("choosing a current project turns the box on when there was none", async () => {
  await scoped(undefined);
  act(() => useCurrentProject.setState({ id: "p1" }));
  expect(screen.getByRole("checkbox", { name: "현재 프로젝트만: Alpha" })).toBeChecked();
});

test("an unchecked box stays unchecked when the current project changes", async () => {
  const { opened } = await scoped("p1");
  await userEvent.click(await screen.findByRole("checkbox", { name: "현재 프로젝트만: Alpha" }));
  act(() => useCurrentProject.setState({ id: "p2" }));
  expect(screen.getByRole("checkbox", { name: "현재 프로젝트만: Beta" })).not.toBeChecked();
  await userEvent.click(screen.getByRole("button", { name: "검색" }));
  await waitFor(() => expect(opened).toHaveLength(1));
  expect(opened[0]!.params).toMatchObject({ projectIds: null });
});

test("a saved search's project stays when the current project changes", async () => {
  await scoped("p1");
  await userEvent.click(await screen.findByRole("button", { name: "다른 것" }));
  expect(await screen.findByRole("checkbox", { name: "이 프로젝트만: Beta" })).toBeChecked();
  act(() => useCurrentProject.setState({ id: undefined }));
  expect(screen.getByRole("checkbox", { name: "이 프로젝트만: Beta" })).toBeChecked();
});

test("without a current project there is no box and every project is searched", async () => {
  const { opened } = await scoped(undefined);
  expect(screen.queryByRole("checkbox", { name: /프로젝트만/ })).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "검색" }));
  await waitFor(() => expect(opened).toHaveLength(1));
  expect(opened[0]!.params).toMatchObject({ projectIds: null });
});

test("a saved search keeps its own project, names it, and the box can widen then narrow to the current one", async () => {
  const { opened } = await scoped("p1");
  await userEvent.click(await screen.findByRole("button", { name: "다른 것" }));
  await waitFor(() => expect(opened).toHaveLength(1));
  expect(opened[0]!.params).toMatchObject({ projectIds: ["p2"], scopes: expect.arrayContaining(["notes"]) });
  const box = await screen.findByRole("checkbox", { name: "이 프로젝트만: Beta" });
  expect(box).toBeChecked();
  await userEvent.click(box);
  const widened = screen.getByRole("checkbox", { name: "현재 프로젝트만: Alpha" });
  expect(widened).not.toBeChecked();
  await userEvent.click(widened);
  await userEvent.click(screen.getByRole("button", { name: "검색" }));
  await waitFor(() => expect(opened).toHaveLength(2));
  expect(opened[1]!.params).toMatchObject({ projectIds: ["p1"] });
});
