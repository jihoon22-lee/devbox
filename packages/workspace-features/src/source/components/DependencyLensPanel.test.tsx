import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  dependencyEnrichmentExecute,
  dependencyEnrichmentPreview,
  dependencyInventory,
  DEPENDENCY_ENRICHMENT_ERROR,
  DEPENDENCY_LENS_ERROR,
  type DependencyEnrichmentPreview,
  type DependencyEnrichmentReport,
  type DependencyReport,
  type EnrichmentSelection,
  type RepoEntry,
} from "../api";
import DependencyLensPanel from "./DependencyLensPanel";

vi.mock("../api", () => ({
  DEPENDENCY_ENRICHMENT_BUSY: "다른 Dependency Lens 분석 또는 원격 조회가 진행 중입니다.",
  DEPENDENCY_ENRICHMENT_ERROR: "Dependency Lens 원격 정보를 불러오지 못했습니다.",
  DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED: "전송 내용을 다시 검토해 주세요.",
  DEPENDENCY_LENS_ERROR: "Dependency Lens 분석을 완료하지 못했습니다.",
  dependencyEnrichmentExecute: vi.fn(),
  dependencyEnrichmentPreview: vi.fn(),
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

const preview: DependencyEnrichmentPreview = {
  token: "p".repeat(64),
  revision: report.revision,
  expiresAtMs: 1_800_000_000_000,
  localPackageCount: report.packageCount,
  services: [
    {
      service: "osv",
      host: "api.osv.dev",
      transmitted: [{
        ecosystem: "crates.io",
        name: "serde",
        version: "1.0.0",
        direct: true,
        localPackageCount: 1,
      }],
      cachedCount: 2,
      staleFallbackCount: 1,
      omittedCount: 3,
      requestCount: 1,
    },
    {
      service: "depsDev",
      host: "api.deps.dev",
      transmitted: [{
        ecosystem: "CARGO",
        name: "serde",
        version: "1.0.0",
        direct: true,
        localPackageCount: 1,
      }],
      cachedCount: 1,
      staleFallbackCount: 1,
      omittedCount: 0,
      requestCount: 2,
    },
  ],
};

const partialEnrichment: DependencyEnrichmentReport = {
  revision: report.revision,
  completedAtMs: 1_700_000_000_000,
  localAuthoritative: true,
  cachePersisted: false,
  entries: [
    {
      packageIds: ["cargo:serde@1.0.0"],
      osv: {
        state: "stale",
        fetchedAtMs: 1_699_800_000_000,
        ageMs: 2 * 24 * 60 * 60 * 1_000,
        advisoryIds: ["GHSA-serde-example"],
        truncated: true,
      },
      depsDev: {
        state: "stale",
        fetchedAtMs: 1_699_800_000_000,
        ageMs: 2 * 24 * 60 * 60 * 1_000,
        licenses: ["MIT", "Apache-2.0"],
        defaultVersion: "2.0.0",
        deprecated: true,
        advisoryIds: ["CVE-2026-0001"],
        versionFound: false,
        packageFound: true,
      },
    },
    {
      packageIds: ["cargo:serde-core@1.0.0"],
      osv: {
        state: "failed",
        fetchedAtMs: null,
        ageMs: null,
        advisoryIds: [],
        truncated: false,
      },
      depsDev: {
        state: "notRequested",
        fetchedAtMs: null,
        ageMs: null,
        licenses: [],
        defaultVersion: null,
        deprecated: false,
        advisoryIds: [],
        versionFound: false,
        packageFound: false,
      },
    },
  ],
  services: [
    {
      service: "osv",
      targetCount: 3,
      transmittedCount: 2,
      cachedCount: 1,
      staleCount: 1,
      failedCount: 1,
      omittedCount: 1,
    },
    {
      service: "depsDev",
      targetCount: 2,
      transmittedCount: 1,
      cachedCount: 1,
      staleCount: 1,
      failedCount: 0,
      omittedCount: 0,
    },
  ],
};

const dependencyInventoryMock = vi.mocked(dependencyInventory);
const dependencyEnrichmentPreviewMock = vi.mocked(dependencyEnrichmentPreview);
const dependencyEnrichmentExecuteMock = vi.mocked(dependencyEnrichmentExecute);

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  dependencyInventoryMock.mockReset().mockResolvedValue(report);
  dependencyEnrichmentPreviewMock.mockReset().mockResolvedValue(preview);
  dependencyEnrichmentExecuteMock.mockReset().mockResolvedValue(partialEnrichment);
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
    const inventory = screen.getByLabelText("의존성 패키지 목록");
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
    const inventory = screen.getByLabelText("의존성 패키지 목록");
    expect(within(inventory).getByText("serde-core", { selector: "strong" })).toBeTruthy();
    expect(within(inventory).queryAllByText("serde", { selector: "strong" })).toHaveLength(0);
    expect(dependencyInventoryMock).toHaveBeenCalledTimes(1);
  });

  it("previews the exact selected services and coordinates before any explicit execute", async () => {
    const osvOnlyPreview: DependencyEnrichmentPreview = {
      ...preview,
      services: [preview.services[0]],
    };
    dependencyEnrichmentPreviewMock.mockResolvedValueOnce(osvOnlyPreview);
    render(<DependencyLensPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "의존성 분석" }));
    await screen.findByText("Cargo.lock");

    fireEvent.click(screen.getByLabelText("deps.dev 라이선스·상태"));
    fireEvent.click(screen.getByLabelText("캐시 무시하고 새로 조회"));
    fireEvent.click(screen.getByRole("button", { name: "전송 내용 검토" }));

    await screen.findByText("https://api.osv.dev");
    await waitFor(() => expect(dependencyEnrichmentPreviewMock).toHaveBeenCalledWith(
      repo.path,
      { osv: true, depsDev: false } satisfies EnrichmentSelection,
      true,
    ));
    expect(dependencyEnrichmentExecuteMock).not.toHaveBeenCalled();

    const disclosure = screen.getByLabelText("원격 전송 검토");
    expect(within(disclosure).getByText("https://api.osv.dev")).toBeTruthy();
    expect(within(disclosure).getByText("crates.io")).toBeTruthy();
    expect(within(disclosure).getByText("serde")).toBeTruthy();
    expect(within(disclosure).getByText("1.0.0")).toBeTruthy();
    expect(disclosure.textContent).toContain("캐시 2");
    expect(disclosure.textContent).toContain("오래된 캐시 대체 1");
    expect(disclosure.textContent).toContain("상한 생략 3");
    expect(disclosure.textContent).toContain("서비스별 전송 좌표 합계 1개");
    expect(disclosure.textContent).toContain("환경·사용자 식별 정보는 보내지 않습니다");
  });

  it("executes only after confirmation and maps partial remote states to the local package", async () => {
    dependencyEnrichmentExecuteMock.mockResolvedValueOnce(partialEnrichment);
    render(<DependencyLensPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "의존성 분석" }));
    await screen.findByText("Cargo.lock");
    fireEvent.click(screen.getByRole("button", { name: "전송 내용 검토" }));
    await screen.findByText("https://api.osv.dev");

    expect(dependencyEnrichmentExecuteMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "검토한 정보 보내기" }));
    await waitFor(() => expect(dependencyEnrichmentExecuteMock).toHaveBeenCalledWith(
      repo.path,
      preview.token,
    ));
    expect(await screen.findByText("원격 보강 완료")).toBeTruthy();
    const summaries = document.querySelector(".dependency-enrichment-summaries");
    expect(summaries?.textContent).toContain("OSV 대상 3 · 전송 2 · 캐시 1 · stale 1 · 실패 1 · 생략 1");
    expect(summaries?.textContent).toContain("deps.dev 대상 2 · 전송 1 · 캐시 1 · stale 1 · 실패 0 · 생략 0");
    expect(screen.getByText("이번 결과의 로컬 캐시를 저장하지 못했습니다.")).toBeTruthy();

    const inventory = screen.getByLabelText("의존성 패키지 목록");
    const serdeDetails = Array.from(inventory.querySelectorAll("details")).find((details) =>
      details.querySelector("summary")?.textContent?.includes("serde")
      && details.querySelector("summary")?.textContent?.includes("1.0.0")
      && !details.querySelector("summary")?.textContent?.includes("serde-core"),
    );
    expect(serdeDetails).toBeTruthy();
    fireEvent.click(serdeDetails!.querySelector("summary")!);
    const serdeMetadata = within(serdeDetails!).getByLabelText("원격 보강 정보");
    expect(serdeMetadata.querySelectorAll(".dependency-remote-state.state-stale")).toHaveLength(2);
    expect(serdeMetadata.textContent).toContain("권고: GHSA-serde-example");
    expect(serdeMetadata.textContent).toContain("라이선스(참고용): MIT · Apache-2.0");
    expect(within(serdeMetadata).getByText(/서비스 기본 버전:/)).toBeTruthy();
    expect(within(serdeMetadata).getByText("deps.dev에서 deprecated로 표시했습니다.")).toBeTruthy();
    expect(within(serdeMetadata).getByText(/서비스에 해당 버전/)).toBeTruthy();

    const serdeCoreDetails = Array.from(inventory.querySelectorAll("details")).find((details) =>
      details.querySelector("summary strong")?.textContent === "serde-core",
    );
    expect(serdeCoreDetails).toBeTruthy();
    expect(within(serdeCoreDetails!).getByText("조회 실패")).toBeTruthy();
  });

  it("invalidates the reviewed transmission when a service selection changes", async () => {
    render(<DependencyLensPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "의존성 분석" }));
    await screen.findByText("Cargo.lock");
    fireEvent.click(screen.getByRole("button", { name: "전송 내용 검토" }));
    await screen.findByLabelText("원격 전송 검토");

    fireEvent.click(screen.getByLabelText("OSV 취약점"));
    expect(screen.queryByLabelText("원격 전송 검토")).toBeNull();
    expect(screen.queryByRole("button", { name: "검토한 정보 보내기" })).toBeNull();
    expect(dependencyEnrichmentExecuteMock).not.toHaveBeenCalled();
  });

  it("invalidates the reviewed transmission when the offline analysis reruns", async () => {
    render(<DependencyLensPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "의존성 분석" }));
    await screen.findByText("Cargo.lock");
    fireEvent.click(screen.getByRole("button", { name: "전송 내용 검토" }));
    await screen.findByLabelText("원격 전송 검토");

    fireEvent.click(screen.getByRole("button", { name: "다시 분석" }));
    await waitFor(() => expect(dependencyInventoryMock).toHaveBeenCalledTimes(2));
    expect(screen.queryByLabelText("원격 전송 검토")).toBeNull();
    expect(screen.queryByRole("button", { name: "검토한 정보 보내기" })).toBeNull();
    expect(dependencyEnrichmentExecuteMock).not.toHaveBeenCalled();
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

  it("redacts remote preview failures and preserves the local report", async () => {
    const secret = "https://user:credential@private.example/C:/secret/path";
    dependencyEnrichmentPreviewMock.mockRejectedValueOnce(new Error(secret));
    render(<DependencyLensPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "의존성 분석" }));
    await screen.findByText("Cargo.lock");
    fireEvent.click(screen.getByRole("button", { name: "전송 내용 검토" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe(DEPENDENCY_ENRICHMENT_ERROR);
    expect(alert.textContent).not.toContain(secret);
    expect(screen.getByText("Cargo.lock")).toBeTruthy();
    expect(within(screen.getByLabelText("의존성 패키지 목록")).getAllByText("serde", { selector: "strong" }).length)
      .toBeGreaterThan(0);
  });

  it("redacts remote execute failures and preserves the local report", async () => {
    const secret = "C:\\private\\token-cache\\registry";
    dependencyEnrichmentExecuteMock.mockRejectedValueOnce(new Error(secret));
    render(<DependencyLensPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "의존성 분석" }));
    await screen.findByText("Cargo.lock");
    fireEvent.click(screen.getByRole("button", { name: "전송 내용 검토" }));
    await screen.findByLabelText("원격 전송 검토");
    fireEvent.click(screen.getByRole("button", { name: "검토한 정보 보내기" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe(DEPENDENCY_ENRICHMENT_ERROR);
    expect(alert.textContent).not.toContain(secret);
    expect(screen.getByText("Cargo.lock")).toBeTruthy();
    expect(within(screen.getByLabelText("의존성 패키지 목록")).getAllByText("serde", { selector: "strong" }).length)
      .toBeGreaterThan(0);
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

  it("drops a late preview when the repository changes", async () => {
    const pending = deferred<DependencyEnrichmentPreview>();
    dependencyEnrichmentPreviewMock.mockReturnValueOnce(pending.promise);
    const rendered = render(<DependencyLensPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "의존성 분석" }));
    await screen.findByText("Cargo.lock");
    fireEvent.click(screen.getByRole("button", { name: "전송 내용 검토" }));
    rendered.rerender(<DependencyLensPanel repo={otherRepo} />);
    pending.resolve(preview);

    await waitFor(() => expect(screen.queryByLabelText("원격 전송 검토")).toBeNull());
    expect(screen.queryByText("https://api.osv.dev")).toBeNull();
    expect(screen.queryByText("검토한 정보 보내기")).toBeNull();
  });

  it("drops a late execute result when the repository changes", async () => {
    const pending = deferred<DependencyEnrichmentReport>();
    dependencyEnrichmentExecuteMock.mockReturnValueOnce(pending.promise);
    const rendered = render(<DependencyLensPanel repo={repo} />);
    fireEvent.click(screen.getByRole("button", { name: "의존성 분석" }));
    await screen.findByText("Cargo.lock");
    fireEvent.click(screen.getByRole("button", { name: "전송 내용 검토" }));
    await screen.findByLabelText("원격 전송 검토");
    fireEvent.click(screen.getByRole("button", { name: "검토한 정보 보내기" }));
    await waitFor(() => expect(dependencyEnrichmentExecuteMock).toHaveBeenCalledWith(
      repo.path,
      preview.token,
    ));

    rendered.rerender(<DependencyLensPanel repo={otherRepo} />);
    pending.resolve(partialEnrichment);
    await waitFor(() => expect(screen.queryByText("원격 보강 완료")).toBeNull());
    expect(screen.queryByText("GHSA-serde-example")).toBeNull();
    expect(screen.getByRole("button", { name: "의존성 분석" })).toBeTruthy();
  });
});
