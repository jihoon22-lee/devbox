import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesCommands } from "./NotesCommands";
import { useNoteSelection } from "./selection";
import { NotesWire } from "./test-wire";

let wire: NotesWire;
beforeEach(() => {
  useNoteSelection.setState({ path: undefined });
  window.history.replaceState(null, "", "/notes");
  wire = new NotesWire();
  wire.handlers["notes.daily"] = () => ({ path: "daily/2026-10-24.md", created: true });
  wire.handlers["notes.create"] = () => ({ path: "새 노트.md", rev: { hash: "h", size: 1, mtimeNs: "1", ino: "1" } });
});
afterEach(() => {
  cleanup();
  window.name = "";
  window.history.replaceState(null, "", "/");
});

function show(navigate = vi.fn()) {
  render(
    <RpcProvider client={wire.client}>
      <NotesCommands navigate={navigate} />
    </RpcProvider>,
  );
  return navigate;
}
const ready = () => vi.waitFor(() => expect(wire.client.state).toBe("ready"));

test("Ctrl+Alt+D opens today's daily note in the main window", async () => {
  const navigate = show();
  await ready();
  fireEvent.keyDown(window, { code: "KeyD", ctrlKey: true, altKey: true });
  await waitFor(() => expect(useNoteSelection.getState().path).toBe("daily/2026-10-24.md"));
  expect(wire.requests("notes.daily")).toEqual([{ date: null }]);
  expect(navigate).toHaveBeenCalledTimes(1);
});

test("a popout window leaves the daily and new-note shortcuts to the main window", async () => {
  window.name = "popout-note-61";
  window.history.replaceState(null, "", "/popout/note/a.md");
  const navigate = show();
  await ready();
  fireEvent.keyDown(window, { code: "KeyD", ctrlKey: true, altKey: true });
  fireEvent.keyDown(window, { code: "KeyN", ctrlKey: true });
  await new Promise((done) => setTimeout(done, 20));
  expect(wire.requests("notes.daily")).toEqual([]);
  expect(wire.requests("notes.create")).toEqual([]);
  expect(navigate).not.toHaveBeenCalled();
});

test("Ctrl+N makes a new note only in the notes section", async () => {
  show();
  await ready();
  window.history.replaceState(null, "", "/agents");
  fireEvent.keyDown(window, { code: "KeyN", ctrlKey: true });
  await new Promise((done) => setTimeout(done, 20));
  expect(wire.requests("notes.create")).toEqual([]);
  window.history.replaceState(null, "", "/notes");
  fireEvent.keyDown(window, { code: "KeyN", ctrlKey: true });
  await waitFor(() => expect(useNoteSelection.getState().path).toBe("새 노트.md"));
  expect(wire.requests("notes.create")).toEqual([{ folder: "", title: "새 노트", template: null }]);
});
