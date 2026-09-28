import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import * as api from "../api";
import BlamePanel from "./BlamePanel";
vi.mock("../api", () => ({ repoBlame: vi.fn(), repoLocalCancel: vi.fn() }));
const repo = { path: "/repo", canonicalKey: "posix:/repo", hasWorktrees: false };
const id = "a".repeat(40);
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.repoLocalCancel).mockResolvedValue(false);
  vi.mocked(api.repoBlame).mockResolvedValue({
    file: "a.txt",
    lines: [
      { line: 1, commit: id, text: "one" },
      { line: 2, commit: id, text: "two" },
    ],
    commits: { [id]: { author: "Kim", authorTime: 1790000000, summary: "first" } },
    truncated: true,
  });
});
afterEach(cleanup);
it("groups consecutive commit headers, links to detail and has no a11y violations", async () => {
  const select = vi.fn();
  const { container } = render(<BlamePanel repo={repo} onShowCommit={select} />);
  fireEvent.change(screen.getByRole("textbox", { name: "파일 경로" }), { target: { value: "a.txt" } });
  fireEvent.click(screen.getByRole("button", { name: "작성 이력 불러오기" }));
  await screen.findByText("one");
  expect(screen.getAllByText(/Kim/)).toHaveLength(1);
  expect(screen.getByText("처음 10,000줄만 보여 줍니다.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: /aaaaaaaa/ }));
  expect(select).toHaveBeenCalledWith(id);
  await assertNoA11yViolations(container);
});
it("loads the exact file and commit chosen from a file row", async () => {
  render(<BlamePanel repo={repo} target={{ file: "a.txt", commitId: id, sequence: 1 }} />);
  await waitFor(() => expect(api.repoBlame).toHaveBeenCalledWith(repo.path, "a.txt", id));
  await screen.findByText("one");
});
