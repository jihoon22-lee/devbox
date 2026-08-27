import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  GIT_VIEW_ERROR,
  repoCommitDetail,
  repoDiff,
  repoHistory,
  type CommitDetail,
  type DiffResult,
  type HistoryResult,
  type RepoEntry,
} from "../api";
import HistoryDiffPanel from "./HistoryDiffPanel";

vi.mock("../api", () => ({
  GIT_VIEW_ERROR: "Git history 또는 diff를 불러올 수 없습니다.",
  repoCommitDetail: vi.fn(),
  repoDiff: vi.fn(),
  repoHistory: vi.fn(),
}));

const repo: RepoEntry = {
  path: "C:\\projects\\sample",
  canonicalKey: "win:c:/projects/sample",
  hasWorktrees: false,
};

const otherRepo: RepoEntry = {
  path: "D:\\projects\\other",
  canonicalKey: "win:d:/projects/other",
  hasWorktrees: false,
};

const entry = {
  id: "0123456789abcdef0123456789abcdef01234567",
  shortId: "0123456789ab",
  parents: [],
  authoredAt: "2026-08-27T09:00:00+09:00",
  author: "Alice",
  authorEmail: "alice@example.test",
  subject: "Add fixture",
};

const history: HistoryResult = { entries: [entry], hasMore: false };
const detail: CommitDetail = { ...entry, body: "A bounded commit body\n" };
const workingDiff: DiffResult = {
  scope: "workingTree",
  commitId: null,
  truncated: false,
  files: [{
    path: "assets/icon.bin",
    oldPath: null,
    status: "modified",
    binary: true,
    patch: "",
    truncated: false,
  }],
};

const repoHistoryMock = vi.mocked(repoHistory);
const repoCommitDetailMock = vi.mocked(repoCommitDetail);
const repoDiffMock = vi.mocked(repoDiff);

beforeEach(() => {
  repoHistoryMock.mockReset().mockResolvedValue(history);
  repoCommitDetailMock.mockReset().mockResolvedValue(detail);
  repoDiffMock.mockReset().mockResolvedValue(workingDiff);
});

afterEach(() => cleanup());

describe("HistoryDiffPanel", () => {
  it("loads an explicit bounded history request and exposes accessible status", async () => {
    render(<HistoryDiffPanel repo={repo} />);

    expect(screen.getByRole("region", { name: "Git history and diff" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "History 불러오기" }));

    await screen.findByRole("button", { name: /Add fixture/ });
    expect(repoHistoryMock).toHaveBeenCalledWith(repo.path, 50);
    expect(screen.getByText("root commit")).toBeTruthy();
    expect(screen.getByRole("status").textContent).toContain("1개 commit");
  });

  it("loads selected commit detail and commit diff without mutation actions", async () => {
    render(<HistoryDiffPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "History 불러오기" }));
    const commit = await screen.findByRole("button", { name: /Add fixture/ });

    fireEvent.click(commit);

    await screen.findByText("A bounded commit body");
    expect(repoCommitDetailMock).toHaveBeenCalledWith(repo.path, entry.id);
    expect(repoDiffMock).toHaveBeenCalledWith(repo.path, entry.id);
    expect(screen.queryByRole("button", { name: /^(stage|commit|push|reset|clean)$/i })).toBeNull();
  });

  it("handles working-tree binary and bounded diff display", async () => {
    render(<HistoryDiffPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "History 불러오기" }));
    await screen.findByRole("button", { name: /Add fixture/ });

    fireEvent.click(screen.getByRole("button", { name: "Working tree diff" }));

    await screen.findByText("Binary file — 내용은 표시하지 않습니다.");
    expect(repoDiffMock).toHaveBeenCalledWith(repo.path, null);
  });

  it("locks the refresh action and ignores duplicate requests while busy", async () => {
    let resolve: (value: HistoryResult) => void = () => undefined;
    repoHistoryMock.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
    render(<HistoryDiffPanel repo={repo} />);

    const button = screen.getByRole("button", { name: "History 불러오기" });
    fireEvent.click(button);
    fireEvent.click(button);

    expect(repoHistoryMock).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "불러오는 중…" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "Git history and diff" }).getAttribute("aria-busy"))
      .toBe("true");

    resolve(history);
    await waitFor(() => expect(screen.getByRole("button", { name: /Add fixture/ })).toBeTruthy());
  });

  it("does not submit a history request during IME composition", async () => {
    render(<HistoryDiffPanel repo={repo} />);
    const limit = screen.getByLabelText("History limit");
    fireEvent.compositionStart(limit);
    fireEvent.keyDown(limit, { key: "Enter" });
    expect(repoHistoryMock).not.toHaveBeenCalled();

    fireEvent.compositionEnd(limit);
    fireEvent.keyDown(limit, { key: "Enter" });
    await waitFor(() => expect(repoHistoryMock).toHaveBeenCalledTimes(1));
  });

  it("ignores late results after unmount and remount and redacts native errors", async () => {
    let resolve: (value: HistoryResult) => void = () => undefined;
    repoHistoryMock.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
    const rendered = render(<HistoryDiffPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "History 불러오기" }));
    rendered.unmount();
    render(<HistoryDiffPanel repo={otherRepo} />);

    resolve(history);
    await Promise.resolve();
    expect(screen.queryByText("Add fixture")).toBeNull();

    repoHistoryMock.mockRejectedValueOnce(new Error("C:\\secret\\credential.git"));
    fireEvent.click(screen.getByRole("button", { name: "History 불러오기" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe(GIT_VIEW_ERROR);
    expect(alert.textContent).not.toContain("credential.git");
  });

  it("rejects an invalid history limit with the fixed error before IPC", async () => {
    render(<HistoryDiffPanel repo={repo} />);
    fireEvent.change(screen.getByLabelText("History limit"), { target: { value: "0" } });
    fireEvent.click(screen.getByRole("button", { name: "History 불러오기" }));

    expect((await screen.findByRole("alert")).textContent).toBe(GIT_VIEW_ERROR);
    expect(repoHistoryMock).not.toHaveBeenCalled();
  });
});
