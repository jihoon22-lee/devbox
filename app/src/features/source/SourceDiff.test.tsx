import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesWire } from "../notes/test-wire";
import { useSourceGit } from "./gitState";
import { SourceDiff } from "./SourceDiff";
import { SourceGitSession } from "./SourceGitSession";

afterEach(() => {
  cleanup();
  useSourceGit.setState({ entries: {}, confirmation: undefined });
});
const target = { projectId: "p" };
const patch = "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1 @@\n-old\n+new\n";
function show(mode: "source" | "review" = "source") {
  const wire = new NotesWire();
  wire.handlers["git.diff"] = () => ({
    path: "a.ts",
    oldPath: null,
    staged: false,
    patch,
    hunks: [{ id: "h1", patch }],
    fileOnly: false,
    expect: { fingerprint: "snapshot" },
  });
  wire.handlers["git.stage"] = () => ({});
  const select = vi.fn();
  render(
    <RpcProvider client={wire.client}>
      <SourceGitSession />
      <SourceDiff target={target} path="a.ts" staged={false} mode={mode} onSelectLines={select} />
    </RpcProvider>,
  );
  return { wire, select };
}
test("source diff stages selected hunk using the exact displayed file snapshot and preserves line ranges", async () => {
  const { wire, select } = show();
  fireEvent.click(await screen.findByRole("button", { name: "변경 후 1줄" }));
  expect(select).toHaveBeenCalledWith({ side: "new", from: 1, to: 1, text: "new" });
  fireEvent.click(screen.getByRole("button", { name: "변경 묶음 1 스테이징" }));
  await waitFor(() =>
    expect(wire.requests("git.stage")).toEqual([
      { target, path: "a.ts", hunk: "h1", expect: { fingerprint: "snapshot" } },
    ]),
  );
});
test("review mode keeps the same diff and citation range but hides every mutation", async () => {
  const { wire, select } = show("review");
  fireEvent.click(await screen.findByRole("button", { name: "변경 전 1줄" }));
  expect(select).toHaveBeenCalledWith({ side: "old", from: 1, to: 1, text: "old" });
  expect(screen.queryByRole("button", { name: /스테이징|변경 버리기/ })).toBeNull();
  expect(wire.requests("git.stage")).toHaveLength(0);
});
