import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  packageDependencySummary,
  type PackageDependencySummary,
} from "../api";
import PackageDependencySummaryPanel from "./PackageDependencySummaryPanel";

vi.mock("../api", () => ({
  packageDependencySummary: vi.fn(),
}));

const freshSummary: PackageDependencySummary = {
  profileId: "profile-1",
  source: "Repo Manager dependency-summary/v1",
  status: "fresh",
  producerVersion: "0.3.0",
  freshnessMs: 42_000,
  revision: `sha256:${"a".repeat(64)}`,
  packageCount: 12,
  directCount: 4,
  transitiveCount: 8,
  duplicateCount: 2,
  unresolvedDependencyCount: 1,
  missingLockfileCount: 0,
  staleLockfileCount: 1,
  unsupportedCount: 1,
  invalidCount: 0,
  truncated: false,
  ecosystems: [
    { ecosystem: "cargo", packageCount: 9, directCount: 3, duplicateCount: 2 },
    { ecosystem: "npm", packageCount: 3, directCount: 1, duplicateCount: 0 },
  ],
};

const packageDependencySummaryMock = vi.mocked(packageDependencySummary);

beforeEach(() => {
  packageDependencySummaryMock.mockReset().mockResolvedValue(freshSummary);
});

afterEach(() => cleanup());

describe("PackageDependencySummaryPanel", () => {
  it("renders aggregate package health without package names or paths", async () => {
    render(<PackageDependencySummaryPanel profileId="profile-1" />);

    await waitFor(() => expect(packageDependencySummaryMock).toHaveBeenCalledWith("profile-1"));
    expect(await screen.findByText("최신 요약")).toBeTruthy();
    expect(screen.getByLabelText("Package dependency 집계")).toHaveTextContent("12");
    expect(screen.getByLabelText("Ecosystem별 package 집계")).toHaveTextContent("Cargo");
    expect(screen.getByText("미해결 edge 1")).toBeTruthy();
    expect(screen.queryByText(/serde|node_modules|projects\\/i)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "패키지 요약 새로고침" }));
    await waitFor(() => expect(packageDependencySummaryMock).toHaveBeenCalledTimes(2));
  });

  it("guides the user to Repo Manager when no project summary exists", async () => {
    packageDependencySummaryMock.mockResolvedValueOnce({
      ...freshSummary,
      status: "missing",
      producerVersion: null,
      freshnessMs: null,
      revision: null,
      packageCount: 0,
      directCount: 0,
      transitiveCount: 0,
      duplicateCount: 0,
      unresolvedDependencyCount: 0,
      staleLockfileCount: 0,
      unsupportedCount: 0,
      ecosystems: [],
    });
    render(<PackageDependencySummaryPanel profileId="profile-1" />);

    expect(await screen.findByText("요약 없음")).toBeTruthy();
    expect(screen.getByText(/Repo Manager에서 이 프로젝트의 ‘의존성 분석’을 실행/)).toBeTruthy();
  });

  it("ignores a late response after switching profiles", async () => {
    let resolveFirst: ((value: PackageDependencySummary) => void) | undefined;
    packageDependencySummaryMock
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValueOnce({ ...freshSummary, profileId: "profile-2", packageCount: 7 });
    const { rerender } = render(<PackageDependencySummaryPanel profileId="profile-1" />);
    rerender(<PackageDependencySummaryPanel profileId="profile-2" />);

    expect(await screen.findByText("7", { selector: "strong" })).toBeTruthy();
    resolveFirst?.({ ...freshSummary, packageCount: 99 });
    await Promise.resolve();
    expect(screen.queryByText("99", { selector: "strong" })).toBeNull();
  });

  it("shows a fixed error without echoing native detail", async () => {
    packageDependencySummaryMock.mockRejectedValueOnce(new Error("C:\\private\\TOKEN"));
    render(<PackageDependencySummaryPanel profileId="profile-1" />);

    expect(await screen.findByRole("alert")).toHaveTextContent("패키지 의존성 요약을 불러올 수 없습니다.");
    expect(screen.queryByText(/private|TOKEN/)).toBeNull();
  });
});
