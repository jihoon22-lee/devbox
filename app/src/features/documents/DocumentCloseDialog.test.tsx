import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { chooseDocumentClose, useClosePrompt } from "./closePrompt";
import { DocumentCloseDialog } from "./DocumentCloseDialog";
import { closedDoc } from "./machine";

afterEach(() => {
  useClosePrompt.getState().request?.answer();
  cleanup();
});
test("code close choice is explicit and cancellation leaves no pending prompt", async () => {
  render(<DocumentCloseDialog />);
  const doc = {
    ...closedDoc("d", { root: "project:p", path: "file.ts" }, { kind: "code" }),
    state: "Dirty" as const,
    leaseId: "lease",
    text: "draft",
    editSeq: 1,
  };
  let choice!: ReturnType<typeof chooseDocumentClose>;
  act(() => {
    choice = chooseDocumentClose([doc], new AbortController().signal);
  });
  expect(screen.getByRole("dialog")).toHaveTextContent("file.ts");
  fireEvent.click(screen.getByRole("button", { name: "초안 유지" }));
  expect(await choice).toEqual({ d: "keep" });
  const aborted = new AbortController();
  act(() => {
    choice = chooseDocumentClose([doc], aborted.signal);
  });
  act(() => aborted.abort());
  expect(await choice).toBeUndefined();
  expect(screen.queryByRole("dialog")).toBeNull();
});
