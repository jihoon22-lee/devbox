import { Suspense, useState, useSyncExternalStore } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { NoteSessionProvider, useNoteSession, hasUnsavedNote } from "@devbox/knowledge-features/notes-lifecycle";
import RecoveryBoundary from "./RecoveryBoundary";
import { recoveryLazy } from "./recoveryLazy";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
it.each([false, true])("reinvokes only the failed boundary's loader and retains the dirty session (repeated failure: %s)", async repeated => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  const load = vi.fn(async () => ({ default: () => <p>recovered feature</p> }));
  load.mockRejectedValueOnce(new Error("chunk unavailable"));
  if (repeated) load.mockRejectedValueOnce(new Error("still unavailable"));
  const Feature = recoveryLazy(load);
  const siblingLoad = vi.fn(async () => ({ default: () => <p>unaffected feature</p> }));
  const Sibling = recoveryLazy(siblingLoad);
  function Editor() {
    const document = useNoteSession()!;
    const note = useSyncExternalStore(document.subscribe, document.snapshot);
    const [show, setShow] = useState(false);
    return <>
      <button onClick={() => { void document.open(async () => ({ path: "draft.md", content: "saved", revision: "r1" }), () => true).then(() => { document.edit("unsaved before failure"); setShow(true); }); }}>open failing feature</button>
      <textarea aria-label="live draft" value={note.content} onChange={event => document.edit(event.target.value)}/>
      <RecoveryBoundary name="sibling"><Suspense><Sibling/></Suspense></RecoveryBoundary>
      {show && <RecoveryBoundary name="failed" recoverNote><Suspense><Feature/></Suspense></RecoveryBoundary>}
    </>;
  }
  render(<NoteSessionProvider><Editor/></NoteSessionProvider>);
  await screen.findByText("unaffected feature");
  fireEvent.click(screen.getByText("open failing feature"));
  await screen.findByRole("alert");
  expect(load).toHaveBeenCalledTimes(1);
  expect((screen.getByLabelText("보존된 노트 내용") as HTMLTextAreaElement).value).toBe("unsaved before failure");
  fireEvent.change(screen.getByLabelText("live draft"), { target: { value: "edited during recovery" } });
  fireEvent.click(screen.getByText("화면 다시 시도"));
  await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
  if (repeated) {
    await screen.findByRole("alert");
    expect((screen.getByLabelText("보존된 노트 내용") as HTMLTextAreaElement).value).toBe("edited during recovery");
    fireEvent.click(screen.getByText("화면 다시 시도"));
  }
  await screen.findByText("recovered feature");
  expect(load).toHaveBeenCalledTimes(repeated ? 3 : 2);
  expect(siblingLoad).toHaveBeenCalledTimes(1);
  expect((screen.getByLabelText("live draft") as HTMLTextAreaElement).value).toBe("edited during recovery");
  expect(hasUnsavedNote()).toBe(true);
});
