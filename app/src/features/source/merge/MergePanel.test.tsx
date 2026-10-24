import { EditorView } from "@codemirror/view";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { RpcError } from "../../../rpc/client";
import type { ConflictPreview } from "../../../rpc/gen/rpc";
import { MergePanel } from "./MergePanel";
import { useMergeDrafts } from "./model";

const rpc = vi.hoisted(() => ({ call: vi.fn(), apply: vi.fn(), refetch: vi.fn() }));
vi.mock("../../../rpc/RpcProvider", () => ({ useRpc: () => rpc, useRpcSession: () => ({ state: "ready" }) }));
vi.mock("../../../rpc/query", () => ({
  useRpcQuery: () => ({ data: [{ path: "file", kind: "modifyModify" }], refetch: rpc.refetch, isLoading: false }),
  useRpcMutation: () => ({ mutateAsync: rpc.apply, isPending: false }),
}));
const target = { projectId: "p" };
function preview(text = "markers"): ConflictPreview {
  return {
    base: "base",
    ours: "ours",
    theirs: "theirs",
    merged: text,
    textSupported: true,
    labels: { base: "base", ours: "main", theirs: "side", rebase: false },
    token: { target, path: "file", worktreeHash: text, stageOids: ["a", "b", "c"], identity: "identity" },
  };
}
function edit(text: string) {
  const view = EditorView.findFromDOM(screen.getByRole("textbox", { name: "해결안" }))!;
  act(() => view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } }));
}
function resolution() {
  return EditorView.findFromDOM(screen.getByRole("textbox", { name: "해결안" }))!.state.doc.toString();
}
beforeAll(() =>
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect() },
  }),
);
beforeEach(() => {
  vi.clearAllMocks();
  useMergeDrafts.setState({ entries: {} });
  rpc.call.mockResolvedValue(preview());
  rpc.apply.mockResolvedValue({ rev: "1" });
});
afterEach(cleanup);
test("four panes keep an edited resolution through stale response and refreshed preview", async () => {
  render(<MergePanel target={target} />);
  await screen.findByRole("textbox", { name: "해결안" });
  expect(screen.getAllByRole("textbox")).toHaveLength(4);
  expect(screen.getByRole("textbox", { name: "내 것" })).toHaveAttribute("contenteditable", "false");
  edit("my typed resolution");
  rpc.apply.mockRejectedValueOnce(new RpcError("git.conflict_stale", { target, path: "file" }));
  rpc.call.mockResolvedValue(preview("external current"));
  fireEvent.click(screen.getByRole("button", { name: "해결안 적용" }));
  await screen.findByText(/파일 또는 Git 상태가 바뀌어/);
  await waitFor(() => expect(screen.getByText("external current")).toBeInTheDocument());
  expect(resolution()).toBe("my typed resolution");
  expect(rpc.apply).toHaveBeenCalledWith(
    expect.objectContaining({
      target,
      path: "file",
      resolution: "my typed resolution",
      token: expect.objectContaining({ worktreeHash: "markers" }),
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "해결안 적용" }));
  await waitFor(() => expect(rpc.apply).toHaveBeenCalledTimes(2));
  expect(rpc.apply.mock.calls[1]?.[0].token.worktreeHash).toBe("external current");
  expect(resolution()).toBe("my typed resolution");
});
test("side choices and reload preserve draft even across closing and reopening", async () => {
  const first = render(<MergePanel target={target} />);
  await screen.findByRole("textbox", { name: "해결안" });
  fireEvent.click(screen.getByRole("button", { name: "둘 다" }));
  expect(resolution()).toBe("ours\ntheirs");
  edit("saved draft");
  first.unmount();
  rpc.call.mockResolvedValue(preview("new disk"));
  render(<MergePanel target={target} />);
  await screen.findByRole("textbox", { name: "해결안" });
  await waitFor(() => expect(screen.getByText("new disk")).toBeInTheDocument());
  expect(resolution()).toBe("saved draft");
});
test("edits while apply is pending survive the successful response", async () => {
  let resolve!: (value: { rev: string }) => void;
  rpc.apply.mockImplementation(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  render(<MergePanel target={target} />);
  await screen.findByRole("textbox", { name: "해결안" });
  edit("submitted");
  fireEvent.click(screen.getByRole("button", { name: "해결안 적용" }));
  edit("newer local draft");
  await act(async () => resolve({ rev: "2" }));
  expect(resolution()).toBe("newer local draft");
  expect(rpc.apply.mock.calls[0]?.[0].resolution).toBe("submitted");
});
test("switching targets isolates the same path and disables unsupported previews", async () => {
  const view = render(<MergePanel target={target} />);
  await screen.findByRole("textbox", { name: "해결안" });
  edit("project draft");
  rpc.call.mockResolvedValue({ ...preview(), textSupported: false });
  view.rerender(<MergePanel target={{ ...target, worktreeId: "agent" }} />);
  await screen.findByText(/바이너리·큰 파일/);
  expect(screen.queryByRole("textbox", { name: "해결안" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "해결안 적용" })).toBeDisabled();
  expect(rpc.call).toHaveBeenLastCalledWith("git.conflict_preview", {
    target: { projectId: "p", worktreeId: "agent" },
    path: "file",
  });
});
test("choosing an absent side submits explicit deletion and preserves it after reload", async () => {
  rpc.call.mockResolvedValue({ ...preview(), ours: null });
  render(<MergePanel target={target} />);
  await screen.findByRole("textbox", { name: "해결안" });
  fireEvent.click(screen.getByRole("button", { name: "내 것" }));
  expect(await screen.findByText(/적용하면 이 파일을 삭제/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /현재 파일 다시 읽기/ }));
  await waitFor(() => expect(screen.getByRole("button", { name: "해결안 적용" })).toBeEnabled());
  expect(screen.getByText(/적용하면 이 파일을 삭제/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "해결안 적용" }));
  await waitFor(() => expect(rpc.apply).toHaveBeenCalledWith(expect.objectContaining({ resolution: null })));
});
