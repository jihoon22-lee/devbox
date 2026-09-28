import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { WorkspaceOperationError } from "../../transport";
import * as api from "../api";
import HunkList from "./HunkList";
vi.mock("../api", () => ({ repoFileHunks: vi.fn(), repoHunksApply: vi.fn(), repoLocalCancel: vi.fn() }));
const repo = { path: "/repo", canonicalKey: "posix:/repo", hasWorktrees: false };
const view = {
  file: "a.txt",
  staged: false,
  supported: true,
  reason: null,
  revision: "r1",
  hunks: [
    {
      id: "h1",
      header: "@@ -1 +1 @@",
      oldStart: 1,
      oldCount: 1,
      newStart: 1,
      newCount: 1,
      lines: [
        { kind: "remove" as const, text: "old" },
        { kind: "add" as const, text: "new" },
      ],
    },
  ],
};
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.repoFileHunks).mockResolvedValue(view);
  vi.mocked(api.repoLocalCancel).mockResolvedValue(false);
});
afterEach(cleanup);
it("shows signed lines, stages a native hunk ID, and remains accessible", async () => {
  const changed = vi.fn();
  const { container } = render(<HunkList repo={repo} file="a.txt" staged={false} onChanged={changed} />);
  fireEvent.click(screen.getByRole("button", { name: "변경 덩어리 보기" }));
  await screen.findByText("@@ -1 +1 @@");
  expect(container.textContent).toContain("-old");
  expect(container.textContent).toContain("+new");
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("button", { name: "이 덩어리 stage" }));
  await waitFor(() =>
    expect(api.repoHunksApply).toHaveBeenCalledWith(
      repo.path,
      "a.txt",
      false,
      "stage",
      ["h1"],
      "r1",
      expect.any(String),
    ),
  );
  await waitFor(() => expect(changed).toHaveBeenCalled());
});
it("requires an inline confirmation before discarding", async () => {
  render(<HunkList repo={repo} file="a.txt" staged={false} />);
  fireEvent.click(screen.getByRole("button", { name: "변경 덩어리 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "이 덩어리 버리기" }));
  expect(api.repoHunksApply).not.toHaveBeenCalled();
  expect(screen.getByText("이 변경을 버립니다. 되돌릴 수 없습니다.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "변경 버리기" }));
  await waitFor(() =>
    expect(api.repoHunksApply).toHaveBeenCalledWith(
      repo.path,
      "a.txt",
      false,
      "discard",
      ["h1"],
      "r1",
      expect.any(String),
    ),
  );
});
it("only offers unstage on the staged side", async () => {
  render(<HunkList repo={repo} file="a.txt" staged />);
  fireEvent.click(screen.getByRole("button", { name: "변경 덩어리 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "이 덩어리 unstage" }));
  await waitFor(() =>
    expect(api.repoHunksApply).toHaveBeenCalledWith(
      repo.path,
      "a.txt",
      true,
      "unstage",
      ["h1"],
      "r1",
      expect.any(String),
    ),
  );
  expect(screen.queryByRole("button", { name: "이 덩어리 버리기" })).toBeNull();
});
it("reloads stale selections without replaying the action", async () => {
  vi.mocked(api.repoHunksApply).mockRejectedValue(new WorkspaceOperationError("파일이 바뀌었습니다.", "hunk_stale"));
  render(<HunkList repo={repo} file="a.txt" staged={false} />);
  fireEvent.click(screen.getByRole("button", { name: "변경 덩어리 보기" }));
  const apply = await screen.findByRole("button", { name: "이 덩어리 stage" });
  vi.mocked(api.repoFileHunks).mockResolvedValue({ ...view, revision: "r2", hunks: [] });
  fireEvent.click(apply);
  await screen.findByText("파일이 바뀌었습니다.");
  expect(api.repoFileHunks).toHaveBeenCalledTimes(2);
  expect(api.repoHunksApply).toHaveBeenCalledTimes(1);
  expect(screen.queryByText("@@ -1 +1 @@")).toBeNull();
});
it("uses file-level actions for unsupported binary files", async () => {
  vi.mocked(api.repoFileHunks).mockResolvedValue({ ...view, supported: false, reason: "binary", hunks: [] });
  render(<HunkList repo={repo} file="a.txt" staged={false} />);
  fireEvent.click(screen.getByRole("button", { name: "변경 덩어리 보기" }));
  await screen.findByText("이 파일은 파일 단위로만 처리할 수 있습니다.");
  expect(screen.getByText("binary 파일")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "이 덩어리 stage" })).toBeNull();
});
