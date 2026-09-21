import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import Knowledge from "./Knowledge";
import { hasUnsavedNote } from "@devbox/knowledge-features/notes-lifecycle";
const failure = vi.hoisted(() => ({ editor: false }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false, invoke: vi.fn() }));
vi.mock("@devbox/knowledge-features/activity", () => ({ default: () => { throw new Error("injected activity render failure"); } }));
vi.mock("@devbox/knowledge-features/search", () => { throw new Error("injected lazy import failure"); });
vi.mock("../../../packages/knowledge-features/src/notes/components/MarkdownEditor", () => ({ default: ({ value, onChange }: { value: string; onChange: (value: string) => void }) => {
  if (failure.editor) throw new Error("injected editor failure");
  return <textarea aria-label="테스트 노트 편집" value={value} onChange={event => onChange(event.target.value)}/>;
} }));
afterEach(() => { cleanup(); failure.editor = false; vi.restoreAllMocks(); });
it("isolates actual Knowledge features and retains a dirty NoteDocument through editor recovery", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  await import("@devbox/knowledge-features/notes");
  render(<Knowledge/>);
  const nav = await screen.findByRole("navigation", { name: "제품 화면" });
  await act(async () => { await vi.dynamicImportSettled(); });
  fireEvent.click(await screen.findByText("FamilyCard.md"));
  const editor = await screen.findByLabelText("테스트 노트 편집");
  fireEvent.change(editor, { target: { value: "unsaved recovery note" } });
  expect(hasUnsavedNote()).toBe(true);
  for (const name of ["활동", "검색"]) {
    fireEvent.click(within(nav).getByRole("button", { name }));
    await act(async () => { await vi.dynamicImportSettled(); });
    expect(await screen.findByRole("alert")).toBeTruthy();
    fireEvent.click(within(nav).getByRole("button", { name: "노트" }));
    expect((screen.getByLabelText("테스트 노트 편집") as HTMLTextAreaElement).value).toBe("unsaved recovery note");
    expect(hasUnsavedNote()).toBe(true);
  }
  failure.editor = true;
  fireEvent.change(screen.getByLabelText("테스트 노트 편집"), { target: { value: "latest recoverable edit" } });
  expect((await screen.findByLabelText("보존된 노트 내용") as HTMLTextAreaElement).value).toBe("latest recoverable edit");
  expect(hasUnsavedNote()).toBe(true);
  failure.editor = false;
  fireEvent.click(screen.getByRole("button", { name: "화면 다시 시도" }));
  await waitFor(() => expect((screen.getByLabelText("테스트 노트 편집") as HTMLTextAreaElement).value).toBe("latest recoverable edit"));
}, 20_000);
