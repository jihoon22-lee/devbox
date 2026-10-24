import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesWire } from "../notes/test-wire";
import { ChangesList } from "./ChangesList";

afterEach(cleanup);
test("porcelain v2 unchanged dots do not duplicate files into the opposite group", async () => {
  const wire = new NotesWire();
  wire.handlers["git.status"] = () => ({
    files: [
      { path: "staged", oldPath: null, indexStatus: "M", worktreeStatus: ".", conflicted: false },
      { path: "working", oldPath: null, indexStatus: ".", worktreeStatus: "M", conflicted: false },
      { path: "new", oldPath: null, indexStatus: "?", worktreeStatus: "?", conflicted: false },
      { path: "conflict", oldPath: null, indexStatus: "U", worktreeStatus: "U", conflicted: true },
    ],
    warnings: [],
  });
  render(
    <RpcProvider client={wire.client}>
      <ChangesList target={{ projectId: "p" }} onSelect={vi.fn()} />
    </RpcProvider>,
  );
  await screen.findByRole("button", { name: "staged" });
  const staged = screen.getByRole("region", { name: "스테이징됨" }),
    working = screen.getByRole("region", { name: "스테이징하지 않음" });
  expect(within(staged).queryByRole("button", { name: "working" })).toBeNull();
  expect(within(working).queryByRole("button", { name: "staged" })).toBeNull();
  expect(within(staged).queryByRole("button", { name: "new" })).toBeNull();
  expect(within(working).getByRole("button", { name: "new" })).toBeVisible();
});
