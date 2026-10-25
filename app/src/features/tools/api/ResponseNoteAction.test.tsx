import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it } from "vitest";
import type { ResponseNoteAppended, ResponseNoteSelection } from "../../../rpc/gen/rpc";
import { ApiBackendProvider, SendFailure } from "./backend";
import { ResponseNoteAction } from "./ResponseNoteAction";
import { FakeBackend } from "./test-backend";

afterEach(cleanup);
const done = (operationId = "op"): ResponseNoteAppended => ({
  path: "chosen.md",
  operationId,
  rev: { hash: "x", size: 1, mtimeNs: "1", ino: "1" },
});
function mount(backend: FakeBackend, historyId: string | null = "original-history", ownerKey = "project-a") {
  return render(
    <ApiBackendProvider backend={backend}>
      <ResponseNoteAction historyId={historyId} ownerKey={ownerKey} />
    </ApiBackendProvider>,
  );
}

it("appends the original history identity to the daily target without submitting renderer response text", async () => {
  const backend = new FakeBackend();
  const user = userEvent.setup();
  mount(backend);
  await user.click(screen.getByRole("button", { name: "노트에 붙이기" }));
  await user.click(screen.getByRole("button", { name: "붙이기" }));
  await screen.findByText("chosen.md에 붙였습니다.");
  expect(backend.selectResponseTarget).toHaveBeenCalledWith({ kind: "daily" });
  expect(backend.appendResponse).toHaveBeenCalledTimes(1);
  expect(backend.appendResponse.mock.calls[0]![0]).toEqual({
    historyId: "original-history",
    selection: "a".repeat(32),
    operationId: expect.stringMatching(/^[a-f0-9]{32}$/),
  });
  expect(backend.noteTargets).not.toHaveBeenCalled();
});

it("selects an existing note and retries the same uncertain intent without another selection or duplicate click", async () => {
  const backend = new FakeBackend();
  const user = userEvent.setup();
  let reject!: (e: Error) => void;
  backend.appendResponse.mockImplementationOnce(
    () =>
      new Promise((_resolve, no) => {
        reject = no;
      }),
  );
  mount(backend);
  await user.click(screen.getByRole("button", { name: "노트에 붙이기" }));
  await user.selectOptions(screen.getByLabelText("붙일 곳"), "existing");
  await screen.findByRole("option", { name: "고른 노트 · chosen.md" });
  await user.selectOptions(screen.getByLabelText("노트"), "chosen.md");
  await user.dblClick(screen.getByRole("button", { name: "붙이기" }));
  expect(backend.appendResponse).toHaveBeenCalledTimes(1);
  await act(async () => reject(new SendFailure("notes.append_uncertain")));
  await screen.findByRole("alert");
  await user.click(screen.getByRole("button", { name: "같은 작업 다시 확인" }));
  await screen.findByText("chosen.md에 붙였습니다.");
  expect(backend.selectResponseTarget).toHaveBeenCalledTimes(1);
  expect(backend.selectResponseTarget).toHaveBeenCalledWith({ kind: "existing", path: "chosen.md" });
  expect(backend.appendResponse.mock.calls[1]![0]).toEqual(backend.appendResponse.mock.calls[0]![0]);
  expect(backend.noteTargets).toHaveBeenCalledTimes(1);
  await user.click(screen.getByRole("button", { name: "노트에 붙이기" }));
  await user.click(screen.getByRole("button", { name: "붙이기" }));
  await waitFor(() => expect(backend.appendResponse).toHaveBeenCalledTimes(3));
  expect(backend.appendResponse.mock.calls[2]![0].operationId).not.toBe(
    backend.appendResponse.mock.calls[0]![0].operationId,
  );
});

it("a late target lookup from an old response or root cannot submit an append", async () => {
  const backend = new FakeBackend();
  let resolve!: (selection: ResponseNoteSelection) => void;
  backend.selectResponseTarget.mockImplementationOnce(
    () =>
      new Promise((yes) => {
        resolve = yes;
      }),
  );
  const user = userEvent.setup();
  const view = mount(backend);
  await user.click(screen.getByRole("button", { name: "노트에 붙이기" }));
  await user.click(screen.getByRole("button", { name: "붙이기" }));
  view.rerender(
    <ApiBackendProvider backend={backend}>
      <ResponseNoteAction historyId="new-history" ownerKey="project-b" />
    </ApiBackendProvider>,
  );
  await act(async () => resolve({ selection: "old", path: "old.md", target: { kind: "daily" } }));
  expect(backend.appendResponse).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

it("late success does not replace the new response state and a missing history disables export", async () => {
  const backend = new FakeBackend();
  let resolve!: (result: ResponseNoteAppended) => void;
  backend.appendResponse.mockImplementationOnce(
    () =>
      new Promise((yes) => {
        resolve = yes;
      }),
  );
  const user = userEvent.setup();
  const view = mount(backend);
  await user.click(screen.getByRole("button", { name: "노트에 붙이기" }));
  await user.click(screen.getByRole("button", { name: "붙이기" }));
  view.rerender(
    <ApiBackendProvider backend={backend}>
      <ResponseNoteAction historyId="new" ownerKey="project-b" />
    </ApiBackendProvider>,
  );
  await act(async () => resolve(done()));
  expect(screen.queryByText("chosen.md에 붙였습니다.")).not.toBeInTheDocument();
  view.rerender(
    <ApiBackendProvider backend={backend}>
      <ResponseNoteAction historyId={null} ownerKey="project-b" />
    </ApiBackendProvider>,
  );
  expect(screen.queryByRole("button", { name: "노트에 붙이기" })).not.toBeInTheDocument();
});
