import { EditorView } from "@codemirror/view";
import { createMemoryHistory, Outlet, RouterProvider } from "@tanstack/react-router";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { buildRouter } from "../../shell/router";
import { DocumentCloseDialog } from "../documents/DocumentCloseDialog";
import { keyId, memoryJournal } from "../documents/offlineJournal";
import { NotesSession } from "../notes/session";
import { NotesWire } from "../notes/test-wire";
import { PopoutCode } from "./PopoutCode";

const native = vi.hoisted(() => ({
  close: undefined as undefined | ((event: { preventDefault(): void }) => Promise<void>),
  destroy: vi.fn(async () => {}),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    destroy: native.destroy,
    onCloseRequested: async (close: typeof native.close) => {
      native.close = close;
      return () => {
        native.close = undefined;
      };
    },
  }),
}));
beforeAll(() => {
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect() },
  });
});
afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.clearAllMocks();
});
test("a code popout's native X waits for the user's acknowledged draft choice", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  const key = { root: "project:p", path: "a.ts" };
  const wire = new NotesWire();
  wire.docs.write(key, "alpha");
  const journal = memoryJournal();
  const router = buildRouter(
    {
      frame: () => <Outlet />,
      pages: {},
      settings: () => null,
      popouts: [{ path: "/popout/code/$key", component: PopoutCode }],
    },
    createMemoryHistory({ initialEntries: [`/popout/code/${encodeURIComponent(keyId(key))}`] }),
  );
  render(
    <RpcProvider client={wire.client}>
      <NotesSession journal={journal} />
      <DocumentCloseDialog />
      <RouterProvider router={router} />
    </RpcProvider>,
  );
  const input = await screen.findByRole("textbox", { name: "코드 편집기" });
  await waitFor(() => expect(input).toHaveTextContent("alpha"));
  const view = EditorView.findFromDOM(input)!;
  act(() => view.dispatch({ changes: { from: 5, insert: " draft" } }));
  await waitFor(() => expect(native.close).toBeTypeOf("function"));
  const preventDefault = vi.fn();
  let close!: Promise<void>;
  act(() => {
    close = native.close!({ preventDefault });
  });
  await screen.findByRole("dialog", { name: "저장하지 않은 파일" });
  expect(preventDefault).toHaveBeenCalled();
  expect(native.destroy).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "초안 유지" }));
  await act(async () => {
    await close;
  });
  expect(wire.docs.journals.get(keyId(key))).toMatchObject({ text: "alpha draft" });
  expect(native.destroy).toHaveBeenCalledOnce();
  expect(wire.docs.count("documents.save")).toBe(0);
});
