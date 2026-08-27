import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  GIT_SAFETY_ERROR,
  repoPreflight,
  type GitSafetySnapshot,
  type RepoEntry,
} from "../api";
import GitSafetyPanel from "./GitSafetyPanel";

vi.mock("../api", () => ({
  GIT_SAFETY_ERROR: "Git 상태를 확인하지 못했습니다.",
  repoPreflight: vi.fn(),
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

const safeSnapshot: GitSafetySnapshot = {
  branch: "main",
  upstream: "origin/main",
  ahead: 0,
  behind: 0,
  dirty: false,
  detached: false,
  noUpstream: false,
  diverged: false,
  rebaseInProgress: false,
  mergeInProgress: false,
  safe: true,
  issues: [],
};

const blockedSnapshot: GitSafetySnapshot = {
  ...safeSnapshot,
  branch: "feature/safety",
  upstream: "origin/feature/safety",
  ahead: 2,
  behind: 3,
  dirty: true,
  diverged: true,
  rebaseInProgress: true,
  mergeInProgress: true,
  safe: false,
  issues: ["dirty", "diverged", "rebaseInProgress", "mergeInProgress"],
};

const repoPreflightMock = vi.mocked(repoPreflight);

beforeEach(() => {
  repoPreflightMock.mockReset().mockResolvedValue(safeSnapshot);
});

afterEach(() => cleanup());

describe("GitSafetyPanel", () => {
  it("requests the exact selected repository and renders the complete state matrix", async () => {
    repoPreflightMock.mockResolvedValueOnce(blockedSnapshot);
    render(<GitSafetyPanel repo={repo} />);

    expect(screen.getByRole("status").textContent).toContain("상태 검사를 실행");
    fireEvent.click(screen.getByRole("button", { name: "상태 검사" }));

    await waitFor(() => expect(repoPreflightMock).toHaveBeenCalledWith(repo.path));
    expect(await screen.findByText("4개 확인이 필요합니다.")).toBeTruthy();
    expect(screen.getByText("feature/safety")).toBeTruthy();
    expect(screen.getByText("origin/feature/safety")).toBeTruthy();
    expect(screen.getByText("↑2 / ↓3")).toBeTruthy();
    expect(screen.getByText("커밋되지 않은 변경이 있습니다.")).toBeTruthy();
    expect(screen.getByText("현재 브랜치와 upstream이 서로 갈라졌습니다.")).toBeTruthy();
    expect(screen.getByText("rebase가 진행 중입니다.")).toBeTruthy();
    expect(screen.getByText("merge가 진행 중입니다.")).toBeTruthy();
  });

  it("maps raw native failures to the fixed error and exposes no recovery action", async () => {
    const raw = "C:\\secret\\credential-helper-path";
    repoPreflightMock.mockRejectedValueOnce(new Error(raw));
    render(<GitSafetyPanel repo={repo} />);

    fireEvent.click(screen.getByRole("button", { name: "상태 검사" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe(GIT_SAFETY_ERROR);
    expect(alert.textContent).not.toContain(raw);
    expect(screen.queryByRole("button", { name: /force|reset|clean/i })).toBeNull();
    expect(screen.getByText(/force push·reset·clean과 자동 복구는 제공하지 않습니다/)).toBeTruthy();
  });

  it("drops a late result from a replaced repository", async () => {
    let resolveOlder: ((value: GitSafetySnapshot) => void) | undefined;
    repoPreflightMock.mockReturnValueOnce(new Promise((resolve) => {
      resolveOlder = resolve;
    }));
    const rendered = render(<GitSafetyPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "상태 검사" }));
    rendered.rerender(<GitSafetyPanel repo={otherRepo} />);
    resolveOlder?.(blockedSnapshot);
    await Promise.resolve();

    expect(screen.getByText(otherRepo.path)).toBeTruthy();
    expect(screen.queryByText("feature/safety")).toBeNull();
    expect(screen.getByRole("status").textContent).toContain("상태 검사를 실행");
  });

  it("ignores duplicate checks and late results after unmount", async () => {
    let resolveCheck: ((value: GitSafetySnapshot) => void) | undefined;
    repoPreflightMock.mockReturnValueOnce(new Promise((resolve) => {
      resolveCheck = resolve;
    }));
    const rendered = render(<GitSafetyPanel repo={repo} />);
    const button = screen.getByRole("button", { name: "상태 검사" });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(repoPreflightMock).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("region", { name: "Git 상태 사전 검사" }).getAttribute("aria-busy"))
      .toBe("true");

    rendered.unmount();
    resolveCheck?.(blockedSnapshot);
    await Promise.resolve();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
