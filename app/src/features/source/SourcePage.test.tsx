import { isolateHistory } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { createMemoryHistory, Outlet, RouterProvider } from "@tanstack/react-router";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { buildRouter } from "../../shell/router";
import { DocumentCloseDialog } from "../documents/DocumentCloseDialog";
import { memoryJournal } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";
import { NotesSession } from "../notes/session";
import { NotesWire } from "../notes/test-wire";
import { ProjectDataProvider } from "../projects/ProjectData";
import { useCurrentProject } from "../projects/store";
import { sourceSearch } from "./editor/location";
import { emptyTabs } from "./editor/tabs";
import { useSourceGit } from "./gitState";
import { SourceRoute } from "./SourcePage";
import { SourceSession } from "./session";
import { useSource } from "./store";

beforeAll(() => {
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect() },
  });
});
afterEach(() => {
  cleanup();
  useSourceGit.setState({ entries: {}, confirmation: undefined });
});
const a = { root: "project:p", path: "a.ts" },
  b = { root: "project:p", path: "b.ts" };
function show() {
  const wire = new NotesWire();
  wire.docs.write(a, "alpha");
  wire.docs.write(b, "beta");
  wire.docs.write({ root: "project:q", path: "q.ts" }, "other project");
  wire.handlers["projects.list"] = () => ({
    rev: "1",
    items: [
      { id: "p", name: "owned", path: "/owned/project", favorite: false, addedMs: 0 },
      { id: "q", name: "other", path: "/owned/q", favorite: false, addedMs: 0 },
    ],
  });
  wire.handlers["files.tree"] = (params) => ({
    entries: (params.projectId === "q" ? [{ root: "project:q", path: "q.ts" }] : [a, b]).map((key) => ({
      key,
      name: key.path,
      kind: "file",
    })),
    nextCursor: null,
  });
  useCurrentProject.setState({ id: "p" });
  useSource.setState({ layout: emptyTabs(), recent: [], locations: {}, quickOpen: false });
  const router = buildRouter(
    {
      frame: () => <Outlet />,
      pages: { "/source": { component: SourceRoute, validateSearch: sourceSearch } },
      settings: () => <p>설정</p>,
      popouts: [],
    },
    createMemoryHistory({ initialEntries: ["/source"] }),
  );
  const journal = memoryJournal();
  render(
    <RpcProvider client={wire.client}>
      <ProjectDataProvider>
        <NotesSession journal={journal} />
        <SourceSession />
        <DocumentCloseDialog />
        <RouterProvider router={router} />
      </ProjectDataProvider>
    </RpcProvider>,
  );
  return { wire, router, journal };
}
function type(input: HTMLElement, text: string) {
  const view = EditorView.findFromDOM(input)!;
  act(() =>
    view.dispatch({ changes: { from: view.state.doc.length, insert: text }, annotations: isolateHistory.of("full") }),
  );
}

test("real source tabs keep independent undo through group moves and section navigation", async () => {
  const { router } = show();
  fireEvent.click(await screen.findByRole("button", { name: "a.ts" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: "코드 편집기" })).toHaveTextContent("alpha"));
  type(screen.getByRole("textbox", { name: "코드 편집기" }), " one");
  fireEvent.click(screen.getByRole("button", { name: "b.ts" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: "코드 편집기" })).toHaveTextContent("beta"));
  type(screen.getByRole("textbox", { name: "코드 편집기" }), " two");
  fireEvent.click(screen.getByRole("tab", { name: "a.ts" }));
  const dragData: Record<string, string> = {};
  const dataTransfer = {
    setData: (type: string, value: string) => {
      dragData[type] = value;
    },
    getData: (type: string) => dragData[type] ?? "",
  };
  fireEvent.dragStart(screen.getByRole("tab", { name: "a.ts" }), { dataTransfer });
  fireEvent.drop(screen.getByRole("region", { name: "오른쪽으로 분할" }), { dataTransfer });
  const right = screen.getByRole("region", { name: "오른쪽 편집기" });
  expect(within(right).getByRole("textbox", { name: "코드 편집기" })).toHaveTextContent("alpha one");
  await act(async () => {
    await router.navigate({ to: "/settings" });
  });
  await act(async () => {
    await router.navigate({ to: "/source" });
  });
  const back = within(screen.getByRole("region", { name: "오른쪽 편집기" })).getByRole("textbox", {
    name: "코드 편집기",
  });
  fireEvent.keyDown(back, { key: "z", code: "KeyZ", ctrlKey: true });
  expect(useDocuments.getState().find(a)?.text).toBe("alpha");
  expect(useDocuments.getState().find(b)?.text).toBe("beta two");
});
test("the tab close dialog keeps the buffer until the explicit durable draft choice", async () => {
  const { wire } = show();
  fireEvent.click(await screen.findByRole("button", { name: "a.ts" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: "코드 편집기" })).toHaveTextContent("alpha"));
  type(screen.getByRole("textbox", { name: "코드 편집기" }), " draft");
  fireEvent.click(screen.getByRole("button", { name: "a.ts 닫기" }));
  const dialog = await screen.findByRole("dialog", { name: "저장하지 않은 파일" });
  expect(useDocuments.getState().find(a)?.text).toBe("alpha draft");
  fireEvent.click(within(dialog).getByRole("button", { name: "초안 유지" }));
  await waitFor(() => expect(screen.queryByRole("tab", { name: "a.ts" })).toBeNull());
  expect(wire.docs.journals.get(JSON.stringify([a.root, a.path]))).toMatchObject({ text: "alpha draft" });
  expect(wire.docs.count("documents.save")).toBe(0);
});

test("source navigation exposes keyboard-selectable files changes and branches without losing dirty tabs", async () => {
  show();
  fireEvent.click(await screen.findByRole("button", { name: "a.ts" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: "코드 편집기" })).toHaveTextContent("alpha"));
  type(screen.getByRole("textbox", { name: "코드 편집기" }), " retained");
  const files = screen.getByRole("tab", { name: "파일" });
  expect(files).toHaveAttribute("aria-selected", "true");
  fireEvent.keyDown(files, { key: "ArrowRight" });
  expect(screen.getByRole("tab", { name: "변경" })).toHaveAttribute("aria-selected", "true");
  fireEvent.keyDown(screen.getByRole("tab", { name: "변경" }), { key: "End" });
  expect(screen.getByRole("tab", { name: "브랜치" })).toHaveAttribute("aria-selected", "true");
  fireEvent.click(files);
  expect(useDocuments.getState().find(a)?.text).toBe("alpha retained");
  expect(screen.getByRole("tab", { name: "a.ts" })).toBeVisible();
});

test("project switching keeps each dirty editor and ignores a late previous project's Git list", async () => {
  const { wire } = show();
  fireEvent.click(await screen.findByRole("button", { name: "a.ts" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: "코드 편집기" })).toHaveTextContent("alpha"));
  type(screen.getByRole("textbox", { name: "코드 편집기" }), " dirty p");
  let finish!: (value: unknown) => void;
  wire.handlers["git.status"] = () =>
    new Promise((resolve) => {
      finish = resolve;
    });
  fireEvent.click(screen.getByRole("tab", { name: "변경" }));
  await waitFor(() => expect(finish).toBeTypeOf("function"));
  act(() => useCurrentProject.getState().set("q"));
  fireEvent.click(await screen.findByRole("button", { name: "q.ts" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: "코드 편집기" })).toHaveTextContent("other project"));
  await act(async () =>
    finish({
      files: [{ path: "late-p.ts", indexStatus: "M", worktreeStatus: ".", oldPath: null, conflicted: false }],
      warnings: [],
    }),
  );
  expect(screen.queryByText("late-p.ts")).toBeNull();
  expect(screen.queryByRole("tab", { name: "a.ts" })).toBeNull();
  expect(useDocuments.getState().find(a)?.text).toBe("alpha dirty p");
  act(() => useCurrentProject.getState().set("p"));
  await screen.findByRole("tab", { name: "a.ts" });
  expect(screen.getByRole("textbox", { name: "코드 편집기" })).toHaveTextContent("alpha dirty p");
});
test("file actions toggle hidden entries and always confirm deletion using the displayed revision", async () => {
  const { wire } = show();
  const rev = { hash: "hash", size: 5, mtimeNs: "1", ino: "2" };
  wire.handlers["files.stat"] = () => ({ key: a, kind: "file", rev });
  wire.handlers["files.delete"] = () => ({});
  await screen.findByRole("button", { name: "a.ts" });
  fireEvent.click(screen.getByRole("checkbox", { name: "숨김 파일" }));
  await waitFor(() => expect(wire.requests("files.tree").at(-1)).toMatchObject({ projectId: "p", showHidden: true }));
  fireEvent.contextMenu(await screen.findByRole("button", { name: "a.ts" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "삭제" }));
  const dialog = await screen.findByRole("dialog", { name: "파일 삭제" });
  await within(dialog).findByText("확인한 크기: 5 바이트");
  expect(wire.requests("files.delete")).toHaveLength(0);
  fireEvent.click(within(dialog).getByRole("button", { name: "삭제" }));
  await waitFor(() => expect(wire.requests("files.delete")).toEqual([{ key: a, expect: rev }]));
});
test("opening a location while viewing a diff reveals the editor even for the already active file", async () => {
  const { wire, router } = show();
  wire.handlers["git.status"] = () => ({
    files: [{ path: "a.ts", indexStatus: ".", worktreeStatus: "M", oldPath: null, conflicted: false }],
    warnings: [],
    expect: {
      state: { branch: "main", headOid: "a", upstream: null, upstreamOid: null },
      indexHash: "i",
      worktreeHash: "w",
    },
  });
  wire.handlers["git.diff"] = () => ({
    path: "a.ts",
    oldPath: null,
    staged: false,
    patch: "",
    hunks: [],
    fileOnly: true,
    expect: { fingerprint: "f" },
  });
  fireEvent.click(await screen.findByRole("button", { name: "a.ts" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: "코드 편집기" })).toHaveTextContent("alpha"));
  fireEvent.click(screen.getByRole("tab", { name: "변경" }));
  fireEvent.click(
    await within(screen.getByRole("region", { name: "스테이징하지 않음" })).findByRole("button", { name: "a.ts" }),
  );
  await screen.findByRole("region", { name: "소스 변경 내용" });
  expect(screen.queryByRole("textbox", { name: "코드 편집기" })).toBeNull();
  await act(async () => {
    await router.navigate({ to: "/source", search: { root: "project:p", path: "a.ts", line: 1, column: 1 } });
  });
  expect(await screen.findByRole("textbox", { name: "코드 편집기" })).toBeVisible();
});
test("history blame cleanup dependencies and conflict tools are reachable for the selected project", async () => {
  const { wire } = show();
  wire.handlers["git.log"] = () => [];
  wire.handlers["git.blame"] = () => [];
  wire.handlers["git.cleanup_candidates"] = () => [];
  wire.handlers["git.worktrees"] = () => [];
  wire.handlers["git.conflicts"] = () => [];
  wire.handlers["deps.list"] = () => ({ fingerprint: "locks", locks: [], dependencies: [] });
  fireEvent.click(await screen.findByRole("button", { name: "a.ts" }));
  await screen.findByRole("textbox", { name: "코드 편집기" });
  fireEvent.click(screen.getByRole("button", { name: "커밋 기록" }));
  await screen.findByRole("region", { name: "커밋 기록" });
  await waitFor(() => expect(wire.requests("git.log")).toEqual([{ target: { projectId: "p" }, limit: 50, skip: 0 }]));
  fireEvent.click(screen.getByRole("button", { name: "줄별 변경 기록" }));
  await waitFor(() => expect(wire.requests("git.blame")).toEqual([{ target: { projectId: "p" }, path: "a.ts" }]));
  fireEvent.click(screen.getByRole("button", { name: "작업 폴더 정리" }));
  await screen.findByText("정리할 수 있는 대상이 없습니다.");
  fireEvent.click(screen.getByRole("button", { name: "의존성 점검" }));
  await screen.findByRole("region", { name: "의존성" });
  await waitFor(() => expect(wire.requests("deps.list")).toEqual([{ projectId: "p" }]));
  fireEvent.click(screen.getByRole("button", { name: "충돌 해결" }));
  await screen.findByRole("dialog", { name: "충돌 해결" });
  await screen.findByText("현재 충돌 파일이 없습니다.");
});
