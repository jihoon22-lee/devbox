import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import * as api from "../api";
import ConflictPanel from "./ConflictPanel";
vi.mock("../api", () => ({
  repoConflictVersions: vi.fn(),
  repoConflictResolve: vi.fn(),
  repoOperationContinue: vi.fn(),
  repoOperationAbort: vi.fn(),
  repoLocalCancel: vi.fn(),
}));
const repo = { path: "/repo", canonicalKey: "posix:/repo", hasWorktrees: false };
const state = {
  operation: "merge" as const,
  files: [
    { path: "a.txt", kind: "bothModified" as const },
    { path: "gone.txt", kind: "deletedByUs" as const },
  ],
};
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.repoLocalCancel).mockResolvedValue(false);
  vi.mocked(api.repoConflictVersions).mockResolvedValue({
    base: "base",
    ours: "ours",
    theirs: "theirs",
    current: "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> other",
    binary: false,
  });
});
afterEach(cleanup);
it("shows three versions, rejects markers, and resolves reviewed content accessibly", async () => {
  const changed = vi.fn();
  const { container } = render(<ConflictPanel repo={repo} state={state} onChanged={changed} />);
  expect(screen.getByText("병합 중 충돌 2개")).toBeTruthy();
  expect(screen.getByRole("button", { name: "계속" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "a.txt" }));
  await screen.findByRole("textbox", { name: "공통 조상" });
  expect(screen.getByRole("textbox", { name: "우리 쪽" })).toHaveValue("ours");
  expect(screen.getByRole("button", { name: "편집한 결과로 해결" })).toBeDisabled();
  fireEvent.change(screen.getByRole("textbox", { name: "결과" }), { target: { value: "resolved" } });
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("button", { name: "편집한 결과로 해결" }));
  await waitFor(() =>
    expect(api.repoConflictResolve).toHaveBeenCalledWith(
      repo.path,
      "a.txt",
      { kind: "content", text: "resolved" },
      expect.any(String),
    ),
  );
  await waitFor(() => expect(changed).toHaveBeenCalled());
});
it("deletion conflicts offer only the surviving side and delete", async () => {
  render(<ConflictPanel repo={repo} state={state} />);
  fireEvent.click(screen.getByRole("button", { name: "gone.txt" }));
  await screen.findByRole("button", { name: "상대 쪽 사용" });
  expect(screen.queryByRole("button", { name: "우리 쪽 사용" })).toBeNull();
  expect(screen.queryByRole("button", { name: "편집한 결과로 해결" })).toBeNull();
  expect(screen.getByRole("button", { name: "파일 삭제로 해결" })).toBeTruthy();
});
it("requires inline abort confirmation and enables continue only after resolution", async () => {
  const { rerender } = render(<ConflictPanel repo={repo} state={state} />);
  fireEvent.click(screen.getByRole("button", { name: "중단" }));
  expect(api.repoOperationAbort).not.toHaveBeenCalled();
  expect(screen.getByText("진행 중인 해결을 모두 버리고 작업 전으로 돌아갑니다.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "중단 확인" }));
  await waitFor(() => expect(api.repoOperationAbort).toHaveBeenCalledWith(repo.path, expect.any(String)));
  rerender(<ConflictPanel repo={repo} state={{ operation: "merge", files: [] }} />);
  await waitFor(() => expect(screen.getByRole("button", { name: "계속" })).not.toBeDisabled());
  fireEvent.click(screen.getByRole("button", { name: "계속" }));
  await waitFor(() => expect(api.repoOperationContinue).toHaveBeenCalled());
});
it("does not offer text editing for binary conflicts", async () => {
  vi.mocked(api.repoConflictVersions).mockResolvedValue({
    base: null,
    ours: null,
    theirs: null,
    current: null,
    binary: true,
  });
  render(<ConflictPanel repo={repo} state={state} />);
  fireEvent.click(screen.getByRole("button", { name: "a.txt" }));
  await screen.findByText("바이너리 또는 큰 파일입니다. 사용할 쪽을 선택해 주세요.");
  expect(screen.queryByRole("textbox", { name: "결과" })).toBeNull();
  expect(screen.getByRole("button", { name: "우리 쪽 사용" })).toBeTruthy();
});
