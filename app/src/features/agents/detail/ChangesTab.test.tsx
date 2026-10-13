import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../../rpc/RpcProvider";
import { ProjectWire } from "../../projects/test-wire";
import { ChangesTab } from "./ChangesTab";
import { useDrafts } from "./drafts";
import { useSeen } from "./seen";

const changes = {
  base: "main",
  ahead: 1,
  behind: 2,
  headOid: "h",
  baseOid: "b",
  files: [
    { path: "new.txt", oldPath: "old.txt", status: "renamed", added: 1, removed: 1 },
    { path: "next.txt", oldPath: null, status: "modified", added: 1, removed: 0 },
  ],
};
const patch = {
  patch: "diff --git a/old.txt b/new.txt\n--- a/old.txt\n+++ b/new.txt\n@@ -1 +1 @@\n-old text\n+new text\n",
  truncated: true,
};
beforeEach(() => {
  useDrafts.setState({ text: {} });
  useSeen.setState({ entries: {} });
});
test("late file patches never replace current selection, quotes append to live draft, seen moves to next file", async () => {
  const wire = new ProjectWire();
  const focus = vi.fn();
  render(
    <RpcProvider client={wire.client}>
      <ChangesTab agentId="a" onQuote={focus} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(1));
  await act(async () => wire.reply("agents.changes", changes));
  await waitFor(() => expect(wire.requests("agents.patch")).toHaveLength(1));
  fireEvent.click(screen.getByRole("button", { name: /M next.txt/ }));
  await waitFor(() => expect(wire.requests("agents.patch")).toHaveLength(2));
  await act(async () => wire.deliver({ type: "response", id: wire.request("agents.patch", 0).id, result: patch }));
  expect(screen.queryByText("old text")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /R new.txt/ }));
  await waitFor(() => expect(wire.requests("agents.patch")).toHaveLength(3));
  await act(async () => wire.reply("agents.patch", patch));
  await screen.findByText("old text");
  expect(screen.getByRole("status")).toHaveTextContent("일부만 가져왔습니다");
  fireEvent.click(screen.getByRole("button", { name: "변경 전 1줄" }));
  act(() => useDrafts.getState().set("a", "방금 쓴 지시"));
  fireEvent.click(screen.getByRole("button", { name: "지시에 인용" }));
  expect(useDrafts.getState().text.a).toBe("방금 쓴 지시\n`old.txt` L1 (변경 전):\n```\nold text\n```\n");
  expect(focus).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "봤음" }));
  expect(useSeen.getState().isSeen("a", "new.txt", "renamed:1:1")).toBe(true);
  expect(useSeen.getState().isSeen("a", "new.txt", "renamed:2:1")).toBe(false);
  expect(screen.getByRole("button", { name: /M next.txt/ })).toHaveAttribute("aria-current", "true");
  expect(screen.queryByRole("button", { name: "지시에 인용" })).toBeNull();
});
