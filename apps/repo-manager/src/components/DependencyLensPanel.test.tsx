import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  dependencyInventory,
  DEPENDENCY_LENS_ERROR,
  type DependencyReport,
  type RepoEntry,
} from "../api";
import DependencyLensPanel from "./DependencyLensPanel";

vi.mock("../api", () => ({
  DEPENDENCY_LENS_ERROR: "Dependency Lens 분석을 완료하지 못했습니다.",
  dependencyInventory: vi.fn(),
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

const report: DependencyReport = {
  revision: `sha256:${"a".repeat(64)}`,
  sources: [
    {
      ecosystem: "cargo",
      path: "Cargo.lock",
      status: "staleLockfile",
      manifestCount: 2,
      lockfileCount: 1,
      packageCount: 3,
      directCount: 1,
    },
    {
      ecosystem: "gradle",
      path: "android/build.gradle.kts",
      status: "unsupported",
      manifestCount: 1,
      lockfileCount: 0,
      packageCount: 0,
      directCount: 0,
    },
  ],
  packages: [
    {
      id: "cargo:serde@1.0.0",
      ecosystem: "cargo",
      name: "serde",
      version: "1.0.0",
      direct: true,
      dependencies: ["cargo:serde-core@1.0.0"],
    },
    {
      id: "cargo:serde@2.0.0",
      ecosystem: "cargo",
      name: "serde",
      version: "2.0.0",
      direct: false,
      dependencies: [],
    },
    {
      id: "cargo:serde-core@1.0.0",
      ecosystem: "cargo",
      name: "serde-core",
      version: "1.0.0",
      direct: false,
      dependencies: [],
    },
  ],
  duplicates: [{ ecosystem: "cargo", name: "serde", versions: ["1.0.0", "2.0.0"] }],
  packageCount: 3,
  directCount: 1,
  transitiveCount: 2,
  unresolvedDependencyCount: 1,
  missingLockfileCount: 0,
  staleLockfileCount: 1,
  unsupportedCount: 1,
  invalidCount: 0,
  truncated: false,
  summaryPublished: true,
};

const dependencyInventoryMock = vi.mocked(dependencyInventory);

beforeEach(() => {
  dependencyInventoryMock.mockReset().mockResolvedValue(report);
});

afterEach(() => cleanup());

describe("DependencyLensPanel", () => {
  it("analyzes the exact repository and renders sources, duplicates, and graph edges", async () => {
    render(<DependencyLensPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "의존성 분석" }));

    await waitFor(() => expect(dependencyInventoryMock).toHaveBeenCalledWith(repo.path));
    expect(await screen.findByText("manifest가 lockfile보다 최신")).toBeTruthy();
    expect(screen.getByText("현재 형식 미지원")).toBeTruthy();
    expect(screen.getAllByText("serde").length).toBeGreaterThan(0);
    expect(screen.getByText("1.0.0 · 2.0.0")).toBeTruthy();
    const inventory = screen.getByLabelText("Dependency package inventory");
    const directVersion = within(inventory).getAllByText("1.0.0", { selector: "span.mono" })[0];
    fireEvent.click(directVersion.closest("summary")!);
    expect(screen.getByText("cargo:serde-core@1.0.0")).toBeTruthy();
  });

  it("filters the bounded inventory without rerunning native analysis", async () => {
    render(<DependencyLensPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "의존성 분석" }));
    await screen.findByText("Cargo.lock");
    fireEvent.change(screen.getByPlaceholderText("이름·버전·ecosystem 필터"), {
      target: { value: "serde-core" },
    });
    const inventory = screen.getByLabelText("Dependency package inventory");
    expect(within(inventory).getByText("serde-core", { selector: "strong" })).toBeTruthy();
    expect(within(inventory).queryAllByText("serde", { selector: "strong" })).toHaveLength(0);
    expect(dependencyInventoryMock).toHaveBeenCalledTimes(1);
  });

  it("bounds duplicate and expanded edge rendering", async () => {
    const boundedReport: DependencyReport = {
      ...report,
      packages: [{
        ...report.packages[0],
        dependencies: Array.from({ length: 301 }, (_, index) => `cargo:target-${index}@1.0.0`),
      }],
      packageCount: 1,
      directCount: 1,
      transitiveCount: 0,
      duplicates: Array.from({ length: 301 }, (_, index) => ({
        ecosystem: "cargo" as const,
        name: `duplicate-${index}`,
        versions: ["1.0.0", "2.0.0"],
      })),
    };
    dependencyInventoryMock.mockResolvedValueOnce(boundedReport);
    render(<DependencyLensPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "의존성 분석" }));

    expect(await screen.findByText("duplicate-299")).toBeTruthy();
    expect(screen.queryByText("duplicate-300")).toBeNull();
    expect(screen.getByText("중복 버전은 렌더링 상한으로 300개만 표시합니다.")).toBeTruthy();

    fireEvent.click(screen.getByText("serde", { selector: "strong" }).closest("summary")!);
    expect(screen.getByText("cargo:target-299@1.0.0")).toBeTruthy();
    expect(screen.queryByText("cargo:target-300@1.0.0")).toBeNull();
    expect(screen.getByText("하위 edge는 렌더링 상한으로 300개만 표시합니다.")).toBeTruthy();
  });

  it("redacts native failures to the fixed error", async () => {
    dependencyInventoryMock.mockRejectedValueOnce(new Error("C:\\private\\registry-token"));
    render(<DependencyLensPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "의존성 분석" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe(DEPENDENCY_LENS_ERROR);
    expect(alert.textContent).not.toContain("registry-token");
  });

  it("drops a late result when the repository changes", async () => {
    let resolveOlder: ((value: DependencyReport) => void) | undefined;
    dependencyInventoryMock.mockReturnValueOnce(new Promise((resolve) => {
      resolveOlder = resolve;
    }));
    const rendered = render(<DependencyLensPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "의존성 분석" }));
    rendered.rerender(<DependencyLensPanel repo={otherRepo} />);
    resolveOlder?.(report);
    await Promise.resolve();
    expect(screen.queryByText("Cargo.lock")).toBeNull();
    expect(screen.getByRole("button", { name: "의존성 분석" })).toBeTruthy();
  });
});
