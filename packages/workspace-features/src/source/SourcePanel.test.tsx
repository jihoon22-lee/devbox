import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import * as api from "./api";
import SourcePanel from "./SourcePanel";
vi.mock("./components/BranchPanel", () => ({ default: () => null }));
vi.mock("./components/StashPanel", () => ({ default: () => null }));
vi.mock("./components/GitSafetyPanel", () => ({ default: () => null }));
vi.mock("./components/HistoryDiffPanel", () => ({ default: () => null }));
vi.mock("./components/BlamePanel", () => ({ default: () => null }));
vi.mock("./components/RemoteSyncPanel", () => ({ default: () => null }));
vi.mock("./components/CleanupPanel", () => ({ default: () => null }));
vi.mock("./components/StageCommitPanel", () => ({
  default: ({ onDirtyChange }: { onDirtyChange: (dirty: boolean) => void }) => (
    <button onClick={() => onDirtyChange(true)}>커밋 메시지 편집</button>
  ),
}));
vi.mock("./api", () => ({
  repoConflicts: vi.fn(),
  repoConflictVersions: vi.fn(),
  repoConflictResolve: vi.fn(),
  repoOperationContinue: vi.fn(),
  repoOperationAbort: vi.fn(),
  repoPrStatus: vi.fn(),
  repoPrList: vi.fn(),
  repoPrCreate: vi.fn(),
  repoLastCommit: vi.fn(),
  repoBranches: vi.fn(),
  repoPush: vi.fn(),
  repoLocalCancel: vi.fn(),
  repoRemoteCancel: vi.fn(),
  repoStatus: vi.fn(),
  worktrees: vi.fn(),
}));
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.repoPrStatus).mockResolvedValue({ available: false, reason: "gh_missing", pr: null });
  vi.mocked(api.repoLocalCancel).mockResolvedValue(false);
  vi.mocked(api.repoConflicts)
    .mockResolvedValueOnce({ operation: "merge", files: [{ path: "a.txt", kind: "bothModified" }] })
    .mockResolvedValue({ operation: "merge", files: [] });
  vi.mocked(api.repoConflictVersions).mockResolvedValue({
    base: "base",
    ours: "ours",
    theirs: "theirs",
    current: "current",
    binary: false,
  });
});
afterEach(cleanup);
it("loads conflicts on entry and refreshes after resolving without dropping another panel draft", async () => {
  const dirty = vi.fn();
  render(
    <SourcePanel
      repo={{ path: "/repo", canonicalKey: "posix:/repo", hasWorktrees: false }}
      onBusyChange={vi.fn()}
      onDirtyChange={dirty}
    />,
  );
  fireEvent.click(await screen.findByRole("button", { name: "a.txt" }));
  await screen.findByRole("textbox", { name: "결과" });
  fireEvent.change(screen.getByRole("textbox", { name: "결과" }), { target: { value: "resolved" } });
  fireEvent.click(screen.getByRole("button", { name: "커밋 메시지 편집" }));
  fireEvent.click(screen.getByRole("button", { name: "편집한 결과로 해결" }));
  await waitFor(() => expect(api.repoConflicts).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(screen.getByRole("button", { name: "계속" })).not.toBeDisabled());
  expect(dirty).toHaveBeenLastCalledWith(true);
});
