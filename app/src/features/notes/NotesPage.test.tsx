import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterAll, afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { buildRouter } from "../../shell/router";
import { TooltipProvider } from "../../ui";
import { memoryJournal } from "../documents/offlineJournal";
import { openPopout } from "../terminal/popout";
import { NotesPage } from "./NotesPage";
import { PopoutNote } from "./PopoutNote";
import { useNoteSelection } from "./selection";
import { NotesSession } from "./session";
import { NotesWire } from "./test-wire";

const a = { root: "vault", path: "a.md" };
const b = { root: "vault", path: "b.md" };
let wire: NotesWire;

const geometry = ["getClientRects", "getBoundingClientRect"] as const;
const saved = geometry.map((name) => Object.getOwnPropertyDescriptor(Range.prototype, name));
beforeAll(() => {
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect() },
  });
});
afterAll(() => {
  geometry.forEach((name, i) => {
    const original = saved[i];
    if (original) Object.defineProperty(Range.prototype, name, original);
    else Reflect.deleteProperty(Range.prototype, name);
  });
});
beforeEach(() => {
  // jsdom has no layout: give the virtual tree a viewport.
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(400);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(260);
  useNoteSelection.setState({ path: undefined });
  wire = new NotesWire();
  wire.docs.write(a, "alpha body");
  wire.docs.write(b, "beta body");
  wire.tree = [
    { path: "a.md", dir: false, title: "Alpha" },
    { path: "b.md", dir: false, title: "Beta" },
  ];
});
afterEach(() => cleanup());

function show() {
  return render(
    <TooltipProvider>
      <RpcProvider client={wire.client}>
        <NotesSession journal={memoryJournal()} windowId="main" />
        <NotesPage />
      </RpcProvider>
    </TooltipProvider>,
  );
}
const editor = () => screen.getByRole("textbox", { name: "노트 편집기" });

test("selecting a note in the tree opens it in the editor", async () => {
  show();
  await userEvent.click(await screen.findByRole("button", { name: "Alpha" }));
  await waitFor(() => expect(editor()).toHaveTextContent("alpha body"));
  expect(editor()).toHaveAttribute("aria-readonly", "false");
  expect(wire.docs.of("documents.claim")).toEqual([expect.objectContaining({ key: a, windowId: "main" })]);
  expect(screen.queryByRole("button", { name: "여기로 가져오기" })).toBeNull();
});

test("a note another window is editing opens read-only with [여기로 가져오기]", async () => {
  await wire.docs.call("documents.claim", { key: b, policy: { kind: "note" }, windowId: "popout-note-622e6d64" });
  show();
  await userEvent.click(await screen.findByRole("button", { name: "Beta" }));
  await waitFor(() => expect(editor()).toHaveTextContent("beta body"));
  expect(editor()).toHaveAttribute("aria-readonly", "true");
  expect(screen.getByText("다른 창에서 편집 중입니다.")).toBeVisible();
  await userEvent.click(screen.getByRole("button", { name: "여기로 가져오기" }));
  await waitFor(() => expect(editor()).toHaveAttribute("aria-readonly", "false"));
  expect(wire.docs.of("documents.take")).toEqual([expect.objectContaining({ key: b, windowId: "main" })]);
  expect(screen.queryByRole("button", { name: "여기로 가져오기" })).toBeNull();
});

test("a nested note path survives the popout URL and route param back to the same document key", async () => {
  const nested = { root: "vault", path: "work/2026 계획.md" };
  wire.docs.write(nested, "nested body");
  const open = vi.spyOn(window, "open").mockReturnValue({ closed: false, focus() {} } as unknown as Window);
  await openPopout("note", nested.path);
  const [url, label] = open.mock.calls[0] as [string, string];
  expect(label).toMatch(/^popout-note-[0-9a-f]+$/);
  const router = buildRouter(
    {
      frame: () => null,
      pages: {},
      settings: () => null,
      popouts: [{ path: "/popout/note/$key", component: PopoutNote }],
    },
    createMemoryHistory({ initialEntries: [url] }),
  );
  render(
    <TooltipProvider>
      <RpcProvider client={wire.client}>
        <NotesSession journal={memoryJournal()} windowId={label} />
        <RouterProvider router={router} />
      </RpcProvider>
    </TooltipProvider>,
  );
  await waitFor(() => expect(editor()).toHaveTextContent("nested body"));
  expect(wire.docs.of("documents.claim")).toEqual([expect.objectContaining({ key: nested, windowId: label })]);
  const to = { root: "vault", path: "renamed.md" };
  wire.docs.move(nested, to);
  wire.emit("documents.moved", { from: nested, to });
  const final = { root: "vault", path: "twice.md" };
  wire.docs.move(to, final);
  wire.emit("documents.moved", { from: to, to: final });
  await waitFor(() => expect(router.state.location.pathname).toContain("twice.md"));
  expect(editor()).toHaveTextContent("nested body");
  expect(editor()).toHaveAttribute("aria-readonly", "false");
  expect(wire.docs.count("documents.claim")).toBe(1);
});

test("a rename elsewhere re-keys the open note and the selection follows without a second claim", async () => {
  show();
  await userEvent.click(await screen.findByRole("button", { name: "Alpha" }));
  await waitFor(() => expect(editor()).toHaveTextContent("alpha body"));
  const to = { root: "vault", path: "z/a.md" };
  wire.docs.move(a, to);
  wire.emit("documents.moved", { from: a, to });
  await waitFor(() => expect(useNoteSelection.getState().path).toBe("z/a.md"));
  expect(editor()).toHaveTextContent("alpha body");
  expect(wire.docs.count("documents.claim")).toBe(1);
});

test("a note that cannot be opened says why and opens on [다시 시도]", async () => {
  wire.docs.failing.set("documents.claim", "documents.out_of_scope");
  show();
  await userEvent.click(await screen.findByRole("button", { name: "Alpha" }));
  const retry = await screen.findByRole("button", { name: "다시 시도" });
  expect(screen.getByRole("alert")).toHaveTextContent(/./);
  wire.docs.failing.delete("documents.claim");
  await userEvent.click(retry);
  await waitFor(() => expect(editor()).toHaveTextContent("alpha body"));
  expect(screen.queryByRole("button", { name: "다시 시도" })).toBeNull();
});
