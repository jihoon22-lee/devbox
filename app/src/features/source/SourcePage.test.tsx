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
import { SourceRoute } from "./SourcePage";
import { SourceSession } from "./session";
import { useSource } from "./store";

beforeAll(() => {
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect() },
  });
});
afterEach(cleanup);
const a = { root: "project:p", path: "a.ts" },
  b = { root: "project:p", path: "b.ts" };
function show() {
  const wire = new NotesWire();
  wire.docs.write(a, "alpha");
  wire.docs.write(b, "beta");
  wire.handlers["projects.list"] = () => ({
    rev: "1",
    items: [{ id: "p", name: "owned", path: "/owned/project", favorite: false, addedMs: 0 }],
  });
  wire.handlers["files.tree"] = () => ({
    entries: [a, b].map((key) => ({ key, name: key.path, kind: "file" })),
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
