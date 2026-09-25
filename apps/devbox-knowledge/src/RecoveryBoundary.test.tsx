import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { NoteSessionProvider, useNoteSession, hasUnsavedNote } from "@devbox/knowledge-features/notes-lifecycle";
import RecoveryBoundary from "./RecoveryBoundary";
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false }));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
it("preserves a note and its quit guard when the entire product view fails", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  let session: ReturnType<typeof useNoteSession>;
  function Capture() {
    session = useNoteSession();
    return null;
  }
  function Product({ failed }: { failed: boolean }) {
    if (failed) throw new Error("shell failed");
    return <p>product</p>;
  }
  const content = (failed: boolean) => (
    <NoteSessionProvider>
      <Capture />
      <RecoveryBoundary name="Knowledge" recoverNote>
        <Product failed={failed} />
      </RecoveryBoundary>
    </NoteSessionProvider>
  );
  const view = render(content(false));
  await act(async () => {
    await session!.openPath("note.md", () => true);
    session!.edit("recover from shell error");
  });
  view.rerender(content(true));
  expect((screen.getByLabelText("보존된 노트 내용") as HTMLTextAreaElement).value).toBe("recover from shell error");
  expect(hasUnsavedNote()).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "보존된 노트 저장" }));
  await screen.findByText("노트를 저장했습니다.");
  expect(hasUnsavedNote()).toBe(false);
});
