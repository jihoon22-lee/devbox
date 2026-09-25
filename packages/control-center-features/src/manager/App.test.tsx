import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  applyDevSetupConfiguration,
  cancelDevSetupApply,
  cancelSupportBundle,
  devSetupAudit,
  discardDevSetupConfiguration,
  exportDevSetupConfiguration,
  exportSupportBundle,
  importDevSetupConfiguration,
  installRelatedTool,
  launchRelatedTool,
  openRelatedToolUrl,
  previewSupportBundle,
  relatedTools,
  runDiagnosis,
} from "./api";
import type { DevSetupAudit, RelatedTool, RelatedToolActionResult, SupportBundlePreview } from "./types";

vi.mock("./api", () => ({
  applyDevSetupConfiguration: vi.fn(),
  cancelDevSetupApply: vi.fn(),
  cancelSupportBundle: vi.fn(),
  devSetupAudit: vi.fn(),
  discardDevSetupConfiguration: vi.fn(),
  exportDevSetupConfiguration: vi.fn(),
  exportSupportBundle: vi.fn(),
  importDevSetupConfiguration: vi.fn(),
  installRelatedTool: vi.fn(),
  launchRelatedTool: vi.fn(),
  openRelatedToolUrl: vi.fn(),
  previewSupportBundle: vi.fn(),
  relatedTools: vi.fn(),
  runDiagnosis: vi.fn(),
}));
const discardDevSetupConfigurationMock = vi.mocked(discardDevSetupConfiguration);
const importDevSetupConfigurationMock = vi.mocked(importDevSetupConfiguration);
const exportDevSetupConfigurationMock = vi.mocked(exportDevSetupConfiguration);
const applyDevSetupConfigurationMock = vi.mocked(applyDevSetupConfiguration);
const cancelDevSetupApplyMock = vi.mocked(cancelDevSetupApply);
const devSetupAuditMock = vi.mocked(devSetupAudit);
const installRelatedToolMock = vi.mocked(installRelatedTool);
const launchRelatedToolMock = vi.mocked(launchRelatedTool);
const openRelatedToolUrlMock = vi.mocked(openRelatedToolUrl);
const runDiagnosisMock = vi.mocked(runDiagnosis);
const cancelSupportBundleMock = vi.mocked(cancelSupportBundle);
const exportSupportBundleMock = vi.mocked(exportSupportBundle);
const previewSupportBundleMock = vi.mocked(previewSupportBundle);
const relatedToolsMock = vi.mocked(relatedTools);
const confirmMock = vi.fn<(message?: string) => boolean>();
const relatedTool: RelatedTool = {
  id: "vs-code",
  displayName: "Visual Studio Code",
  summary: "경량 코드 편집기",
  wingetId: "Microsoft.VisualStudioCode",
  officialUrl: "https://code.visualstudio.com/",
  licenseUrl: "https://code.visualstudio.com/License",
  license: "Microsoft 배포 약관 · 소스 MIT",
  platformSupported: true,
  installed: false,
  detection: "not-found",
  installState: "absent",
  launchState: "unavailable",
  dockerCapability: null,
};

const devSetupFixture: DevSetupAudit = {
  schemaVersion: 1,
  observedAtMs: Date.now(),
  mode: "read-only",
  capabilities: [
    {
      id: "docker-desktop-install",
      scope: "windows",
      state: "unknown",
      evidence: [{ source: "desktop-executable", result: "not-observed" }],
    },
    {
      id: "docker-desktop-launch",
      scope: "windows",
      state: "unavailable",
      evidence: [{ source: "desktop-executable", result: "not-observed" }],
    },
    {
      id: "docker-windows-cli",
      scope: "windows",
      state: "unavailable",
      evidence: [{ source: "windows-cli", result: "not-observed" }],
    },
    {
      id: "docker-wsl-backend",
      scope: "wsl",
      state: "running",
      evidence: [
        { source: "wsl-registration", result: "registered" },
        { source: "wsl-runtime", result: "running" },
      ],
    },
    {
      id: "winget",
      scope: "windows",
      state: "available",
      evidence: [{ source: "winget-executable", result: "trusted-location" }],
    },
  ],
  plan: [
    { capabilityId: "docker-desktop-install", status: "unknown", action: "verify-installation" },
    { capabilityId: "docker-desktop-launch", status: "review", action: "review-launch-path" },
    { capabilityId: "docker-windows-cli", status: "review", action: "review-cli" },
    { capabilityId: "docker-wsl-backend", status: "satisfied", action: "none" },
    { capabilityId: "winget", status: "satisfied", action: "none" },
  ],
};

type DevSetupConfigurationReviewFixture = NonNullable<Awaited<ReturnType<typeof importDevSetupConfiguration>>>;
type DevSetupConfigurationApplyFixture = Awaited<ReturnType<typeof applyDevSetupConfiguration>>;

const devSetupConfigurationReviewFixture: DevSetupConfigurationReviewFixture = {
  schemaVersion: "0.3",
  previewId: `devsetup-${"a".repeat(64)}`,
  expiresAtMs: Date.now() + 5 * 60 * 1_000,
  configurationDigest: "b".repeat(64),
  sourceTrust: "external-restricted",
  mode: "package-only",
  canApply: true,
  hasChanges: true,
  requiresAgreementConfirmation: true,
  mayRequireAdmin: true,
  mayRequireReboot: true,
  packages: [
    {
      packageId: "Git.Git",
      desired: "latest",
      version: null,
      currentState: "absent",
      action: "install",
      requestedAgreementAcceptance: false,
      declaredElevation: false,
    },
    {
      packageId: "Microsoft.VisualStudioCode",
      desired: "latest",
      version: null,
      currentState: "update-available",
      action: "update",
      requestedAgreementAcceptance: true,
      declaredElevation: false,
    },
    {
      packageId: "Microsoft.PowerShell",
      desired: "version",
      version: "7.4.6",
      currentState: "present",
      action: "reconcile-version",
      requestedAgreementAcceptance: false,
      declaredElevation: true,
    },
  ],
};

const devSetupConfigurationApplyFixture: DevSetupConfigurationApplyFixture = {
  status: "complete",
  observedAtMs: Date.now(),
  results: [
    { packageId: "Git.Git", status: "applied" },
    { packageId: "Microsoft.VisualStudioCode", status: "applied" },
    { packageId: "Microsoft.PowerShell", status: "applied" },
  ],
};

const supportPreviewFixture: SupportBundlePreview = {
  previewId: "support-preview-1",
  expiresAtMs: Date.now() + 300_000,
  estimatedBytes: 2048,
  databaseCount: 0,
  includedSections: ["diagnosis", "products", "operation-log"],
  omittedSections: ["raw-database", "raw-logs", "paths", "environment-values", "credentials", "authorization"],
  redactionVersion: "v1",
};

beforeEach(() => {
  discardDevSetupConfigurationMock.mockReset().mockResolvedValue(undefined);
  importDevSetupConfigurationMock.mockReset().mockResolvedValue(null);
  exportDevSetupConfigurationMock.mockReset().mockResolvedValue({
    filename: "devbox-packages.winget",
    mimeType: "application/yaml;charset=utf-8",
    content: "$schema: normalized",
    byteCount: 21,
    sha256: "b".repeat(64),
  });
  applyDevSetupConfigurationMock.mockReset().mockResolvedValue(devSetupConfigurationApplyFixture);
  cancelDevSetupApplyMock.mockReset().mockResolvedValue(undefined);
  devSetupAuditMock.mockReset().mockResolvedValue(devSetupFixture);
  installRelatedToolMock.mockReset().mockResolvedValue({
    toolId: relatedTool.id,
    status: "installed",
    message: "WinGet 설치가 완료되었습니다.",
  });
  launchRelatedToolMock.mockReset().mockResolvedValue({
    toolId: relatedTool.id,
    status: "launched",
    message: "관련 도구를 실행했습니다.",
  });
  openRelatedToolUrlMock.mockReset().mockResolvedValue(undefined);
  runDiagnosisMock.mockReset().mockResolvedValue([]);
  cancelSupportBundleMock.mockReset().mockResolvedValue(undefined);
  exportSupportBundleMock.mockReset().mockResolvedValue({
    filename: "devbox-support-bundle.json",
    mimeType: "application/json",
    content: '{"redactionVersion":"v1"}',
    byteCount: 26,
    redactionVersion: "v1",
  });
  previewSupportBundleMock.mockReset().mockResolvedValue(supportPreviewFixture);
  relatedToolsMock.mockReset().mockResolvedValue([relatedTool]);
  confirmMock.mockReset().mockReturnValue(false);
  Object.defineProperty(URL, "createObjectURL", {
    configurable: true,
    value: vi.fn(() => "blob:devbox-test"),
  });
  Object.defineProperty(URL, "revokeObjectURL", {
    configurable: true,
    value: vi.fn(),
  });
  Object.defineProperty(window, "confirm", {
    configurable: true,
    value: confirmMock,
  });
});

afterEach(() => cleanup());

it("초기 셸이 접근성 위반 없이 렌더링된다", async () => {
  const { container } = render(<App />);
  await screen.findByText("Control Center 도구");
  await assertNoA11yViolations(container);
});

describe("Devbox Manager diagnostics and support bundle", () => {
  it("shows support bundle inclusion and omission boundaries before one-time export", async () => {
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "환경 진단" }));
    await screen.findByRole("heading", { name: "비식별화된 지원 번들" });

    fireEvent.click(screen.getByRole("button", { name: "번들 미리 확인" }));
    await screen.findByText(/내보내기 미리 보기 · 비식별화 v1/);
    expect(previewSupportBundleMock).toHaveBeenCalledWith(expect.any(String));
    expect(exportSupportBundleMock).not.toHaveBeenCalled();
    expect(screen.getByText("raw-database")).toBeTruthy();
    expect(screen.getByText("credentials")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "확인 후 JSON 내보내기" }));
    await waitFor(() => expect(exportSupportBundleMock).toHaveBeenCalledWith("support-preview-1"));
    expect(screen.getByRole("status").textContent).toContain("redacted 지원 번들을 준비했습니다.");
  });

  it("clears a consumed support preview when native export reports a stale revision", async () => {
    exportSupportBundleMock.mockRejectedValueOnce(new Error("지원 번들이 오래되었습니다."));
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "환경 진단" }));
    await screen.findByRole("heading", { name: "비식별화된 지원 번들" });
    fireEvent.click(screen.getByRole("button", { name: "번들 미리 확인" }));
    await screen.findByText(/내보내기 미리 보기 · 비식별화 v1/);

    fireEvent.click(screen.getByRole("button", { name: "확인 후 JSON 내보내기" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("지원 번들이 오래되었습니다."));
    expect(screen.queryByText(/내보내기 미리 보기 · 비식별화 v1/)).toBeNull();
    expect(screen.getByRole("button", { name: "번들 미리 확인" })).toBeTruthy();
  });
});

describe("Devbox Manager Related Tools", () => {
  it("loads the bounded curated metadata and official links", async () => {
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "관련 도구" }));

    expect(await screen.findByText("Visual Studio Code")).toBeTruthy();
    expect(screen.getByText("표준 감지 위치에서 찾지 못했습니다.")).toBeTruthy();
    expect(screen.getByText(/감지와 이미 설치된 도구 실행은 인터넷 없이 가능/)).toBeTruthy();
    expect(screen.getByRole("link", { name: "공식 사이트" }).getAttribute("href")).toBe(
      "https://code.visualstudio.com/",
    );
    expect(screen.getByRole("link", { name: "라이선스" }).getAttribute("href")).toBe(
      "https://code.visualstudio.com/License",
    );
    expect(screen.getByText("Microsoft.VisualStudioCode")).toBeTruthy();
    expect(screen.getByRole("button", { name: "확인 후 WinGet 설치" })).toBeTruthy();
    fireEvent.click(screen.getByRole("link", { name: "공식 사이트" }));
    await waitFor(() => expect(openRelatedToolUrlMock).toHaveBeenCalledWith("https://code.visualstudio.com/"));
  });

  it("does not render non-HTTPS links returned outside the curated contract", async () => {
    relatedToolsMock.mockResolvedValueOnce([
      {
        ...relatedTool,
        officialUrl: "https://evil.example/tool.exe",
        licenseUrl: "javascript:alert(1)",
      },
    ]);
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "관련 도구" }));
    await screen.findByText("Visual Studio Code");

    expect(screen.queryByRole("link", { name: "공식 사이트" })).toBeNull();
    expect(screen.queryByRole("link", { name: "라이선스" })).toBeNull();
  });

  it("keeps official links usable while disabling WinGet on unsupported platforms", async () => {
    relatedToolsMock.mockResolvedValueOnce([
      {
        ...relatedTool,
        platformSupported: false,
        detection: "unavailable",
        installState: "unknown",
        launchState: "unknown",
      },
    ]);
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "관련 도구" }));

    const install = await screen.findByRole("button", { name: "설치 상태 확인 필요" });
    expect((install as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("link", { name: "공식 사이트" }));
    await waitFor(() => expect(openRelatedToolUrlMock).toHaveBeenCalledWith("https://code.visualstudio.com/"));
    expect(installRelatedToolMock).not.toHaveBeenCalled();
  });

  it("does not parse unbounded official-link values", async () => {
    relatedToolsMock.mockResolvedValueOnce([
      {
        ...relatedTool,
        officialUrl: `https://code.visualstudio.com/${"x".repeat(2048)}`,
      },
    ]);
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "관련 도구" }));
    await screen.findByText("Visual Studio Code");

    expect(screen.queryByRole("link", { name: "공식 사이트" })).toBeNull();
  });

  it("requires confirmation before invoking WinGet install", async () => {
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "관련 도구" }));
    await screen.findByText("Visual Studio Code");

    fireEvent.click(screen.getByRole("button", { name: "확인 후 WinGet 설치" }));
    expect(confirmMock).toHaveBeenCalledWith(
      "'Visual Studio Code'을 WinGet으로 설치할까요? WinGet이 공식 패키지 설치를 진행합니다.",
    );
    expect(installRelatedToolMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(screen.getByRole("button", { name: "확인 후 WinGet 설치" }));
    await waitFor(() => expect(installRelatedToolMock).toHaveBeenCalledWith("vs-code", true));
    await waitFor(() => expect(relatedToolsMock).toHaveBeenCalledTimes(2));
  });

  it("offers launch only for a detected installed tool", async () => {
    relatedToolsMock.mockResolvedValueOnce([
      {
        ...relatedTool,
        installed: true,
        detection: "path",
        installState: "present",
        launchState: "available",
      },
    ]);
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "관련 도구" }));

    expect(await screen.findByRole("button", { name: "실행" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "확인 후 WinGet 설치" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "실행" }));
    await waitFor(() => expect(launchRelatedToolMock).toHaveBeenCalledWith("vs-code"));
  });

  it("shows a Docker WSL backend without claiming the desktop is uninstalled", async () => {
    relatedToolsMock.mockResolvedValueOnce([
      {
        id: "docker-desktop",
        displayName: "Docker Desktop",
        summary: "Docker 컨테이너 개발 환경",
        wingetId: "Docker.DockerDesktop",
        officialUrl: "https://www.docker.com/products/docker-desktop/",
        licenseUrl: "https://www.docker.com/legal/docker-software-license/",
        license: "Docker Software License",
        platformSupported: true,
        installed: false,
        detection: "not-found",
        installState: "unknown",
        launchState: "unavailable",
        dockerCapability: {
          desktopInstall: "unknown",
          desktopLaunch: "unavailable",
          windowsCli: "available",
          wslBackend: "running",
          evidence: [
            { source: "desktop-executable", result: "not-observed" },
            { source: "windows-cli", result: "known-location" },
            { source: "wsl-registration", result: "registered" },
            { source: "wsl-runtime", result: "running" },
          ],
          observedAtMs: Date.now(),
        },
      },
    ]);
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "관련 도구" }));

    expect(await screen.findByRole("button", { name: "설치 상태 확인 필요" })).toBeTruthy();
    expect(screen.getByText("실행 중")).toBeTruthy();
    expect(screen.getByText("docker-desktop WSL 등록 확인")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "확인 후 WinGet 설치" })).toBeNull();
    expect(installRelatedToolMock).not.toHaveBeenCalled();
  });

  it("does not render raw native errors from a related-tool action", async () => {
    installRelatedToolMock.mockRejectedValueOnce(new Error("C:\\Users\\developer\\secret-token=should-not-render"));
    confirmMock.mockReturnValueOnce(true);
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "관련 도구" }));
    await screen.findByText("Visual Studio Code");

    fireEvent.click(screen.getByRole("button", { name: "확인 후 WinGet 설치" }));

    await waitFor(() => expect(screen.getByText("관련 도구 작업을 완료할 수 없습니다.")).toBeTruthy());
    expect(screen.queryByText(/secret-token/)).toBeNull();
    expect(screen.queryByText(/C:\\Users\\developer/)).toBeNull();
  });

  it("localizes an allowlisted native related-tool error without changing its contract", async () => {
    installRelatedToolMock.mockRejectedValueOnce(new Error("Related Tools는 Windows에서만 사용할 수 있습니다."));
    confirmMock.mockReturnValueOnce(true);
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "관련 도구" }));
    await screen.findByText("Visual Studio Code");

    fireEvent.click(screen.getByRole("button", { name: "확인 후 WinGet 설치" }));

    await waitFor(() => expect(screen.getByText("관련 도구는 Windows에서만 사용할 수 있습니다.")).toBeTruthy());
  });

  it("ignores a related-tool action result after unmount", async () => {
    let resolveInstall!: (result: RelatedToolActionResult) => void;
    installRelatedToolMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveInstall = resolve;
        }),
    );
    confirmMock.mockReturnValueOnce(true);
    const view = render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "관련 도구" }));
    await screen.findByText("Visual Studio Code");
    fireEvent.click(screen.getByRole("button", { name: "확인 후 WinGet 설치" }));
    await waitFor(() => expect(installRelatedToolMock).toHaveBeenCalledWith("vs-code", true));

    view.unmount();
    resolveInstall({
      toolId: "vs-code",
      status: "installed",
      message: "C:\\Users\\developer\\unexpected-output",
    });
    await Promise.resolve();

    expect(document.body.textContent).not.toContain("unexpected-output");
    expect(relatedToolsMock).toHaveBeenCalledTimes(1);
  });
});

describe("Devbox Manager Dev Setup audit", () => {
  it("renders a read-only capability inventory and review plan", async () => {
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "Dev Setup" }));

    expect(await screen.findByRole("heading", { name: "Docker Desktop 설치" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "docker-desktop WSL backend" })).toBeTruthy();
    expect(screen.getByText("Docker Desktop 실행 위치 확인")).toBeTruthy();
    expect(screen.getByText("docker-desktop WSL 실행 중")).toBeTruthy();
    expect(screen.getByText(/설치·실행·registry·PATH 변경은 수행하지 않습니다/)).toBeTruthy();
    expect(devSetupAuditMock).toHaveBeenCalledTimes(1);
    expect(installRelatedToolMock).not.toHaveBeenCalled();
  });

  it("does not render raw native errors from the audit boundary", async () => {
    devSetupAuditMock.mockRejectedValueOnce(new Error("C:\\Users\\developer\\secret-token=should-not-render"));
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "Dev Setup" }));

    expect(
      await screen.findByText("Dev Setup 감사를 완료할 수 없습니다. Windows와 WSL 환경을 확인하세요."),
    ).toBeTruthy();
    expect(screen.queryByText(/secret-token/)).toBeNull();
  });

  it("renders the normalized package-only review, diff, and fixed warnings", async () => {
    importDevSetupConfigurationMock.mockResolvedValueOnce(devSetupConfigurationReviewFixture);
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "Dev Setup" }));
    await screen.findByRole("heading", { name: "docker-desktop WSL backend" });

    fireEvent.click(screen.getByRole("button", { name: "구성 가져오기" }));

    expect(
      await screen.findByRole("heading", {
        name: "WinGet 구성 v3 · package-only",
      }),
    ).toBeTruthy();
    expect(screen.getByText(/외부 YAML은 그대로 실행하지 않습니다/)).toBeTruthy();
    expect(screen.getByText("Git.Git")).toBeTruthy();
    expect(screen.getByText("Microsoft.VisualStudioCode")).toBeTruthy();
    expect(screen.getAllByText("최신 버전").length).toBeGreaterThan(0);
    expect(screen.getByText("미설치")).toBeTruthy();
    expect(screen.getByText("업데이트 가능")).toBeTruthy();
    expect(screen.getByText("설치")).toBeTruthy();
    expect(screen.getByText("업데이트")).toBeTruthy();
    expect(screen.getByText("외부 약관 수락 요청")).toBeTruthy();
    expect(screen.getByText("외부 관리자 선언")).toBeTruthy();
    expect(screen.getByText(/외부 파일의 선언을 실행 설정에 복사하지 않습니다/)).toBeTruthy();
    expect(screen.getByText("schema")).toBeTruthy();
    expect(screen.getByText("0.3")).toBeTruthy();
    expect(screen.getByText("외부 입력 · 제한 처리(신뢰 안 함)")).toBeTruthy();
    expect(screen.getByText(/sha256:bbbbbbbbbbbb/)).toBeTruthy();
    expect(screen.getByText("5분 · 1회 적용")).toBeTruthy();
    expect(screen.getByText("네트워크 연결이 필요합니다.")).toBeTruthy();
    expect(screen.getByText(/UAC\/관리자 권한과 재부팅/)).toBeTruthy();
    expect(screen.getByText(/자동 재부팅을 예약하거나 실행하지 않습니다/)).toBeTruthy();
    expect(screen.getByText(/PATH·registry·파일을 변경/)).toBeTruthy();
    expect(screen.getByText(/상태를 알 수 없는 패키지는 적용을 차단/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "정규화된 구성 내보내기" }).hasAttribute("disabled")).toBe(false);
    expect(screen.getByRole("button", { name: "검토 버리기" }).hasAttribute("disabled")).toBe(false);
  });

  it("clears the prior preview as soon as a reimport starts and stays cleared when cancelled", async () => {
    importDevSetupConfigurationMock
      .mockResolvedValueOnce(devSetupConfigurationReviewFixture)
      .mockResolvedValueOnce(null);
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "Dev Setup" }));
    await screen.findByRole("heading", { name: "docker-desktop WSL backend" });
    fireEvent.click(screen.getByRole("button", { name: "구성 가져오기" }));
    await screen.findByText("Git.Git");

    fireEvent.click(screen.getByRole("button", { name: "다시 가져오기" }));
    await waitFor(() => expect(screen.queryByText("Git.Git")).toBeNull());
    expect(screen.queryByRole("button", { name: "정규화된 구성 내보내기" })).toBeNull();
    await waitFor(() => expect(screen.getByText(/WinGet Configuration 파일을 가져오면/)).toBeTruthy());
  });

  it("revokes the native preview before clearing a discarded review", async () => {
    importDevSetupConfigurationMock.mockResolvedValueOnce(devSetupConfigurationReviewFixture);
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "Dev Setup" }));
    await screen.findByRole("heading", { name: "docker-desktop WSL backend" });
    fireEvent.click(screen.getByRole("button", { name: "구성 가져오기" }));
    await screen.findByText("Git.Git");

    fireEvent.click(screen.getByRole("button", { name: "검토 버리기" }));
    await waitFor(() =>
      expect(discardDevSetupConfigurationMock).toHaveBeenCalledWith(devSetupConfigurationReviewFixture.previewId),
    );
    expect(screen.queryByText("Git.Git")).toBeNull();
    expect(screen.getByText(/WinGet Configuration 파일을 가져오면/)).toBeTruthy();
  });

  it("requires all three checks and the final confirmation before applying", async () => {
    importDevSetupConfigurationMock.mockResolvedValueOnce(devSetupConfigurationReviewFixture);
    confirmMock.mockReturnValueOnce(false);
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "Dev Setup" }));
    await screen.findByRole("heading", { name: "docker-desktop WSL backend" });
    fireEvent.click(screen.getByRole("button", { name: "구성 가져오기" }));

    const apply = await screen.findByRole("button", { name: "확인 후 패키지 적용" });
    expect(apply.hasAttribute("disabled")).toBe(true);
    fireEvent.click(screen.getByRole("checkbox", { name: "정규화된 package-only 검토를 확인했습니다" }));
    expect(apply.hasAttribute("disabled")).toBe(true);
    fireEvent.click(
      screen.getByRole("checkbox", { name: "로컬에 등록된 고정 이름 winget source·패키지 약관 수락을 확인했습니다" }),
    );
    expect(apply.hasAttribute("disabled")).toBe(true);
    fireEvent.click(screen.getByRole("checkbox", { name: "관리자/UAC·재부팅 위험을 확인했습니다" }));
    expect(apply.hasAttribute("disabled")).toBe(false);

    fireEvent.click(apply);
    expect(confirmMock).toHaveBeenCalledTimes(1);
    expect(applyDevSetupConfigurationMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(apply);
    await waitFor(() =>
      expect(applyDevSetupConfigurationMock).toHaveBeenCalledWith(
        devSetupConfigurationReviewFixture.previewId,
        true,
        true,
        true,
      ),
    );
    expect(screen.getByRole("button", { name: "정규화된 구성 내보내기" }).hasAttribute("disabled")).toBe(true);
  });

  it("blocks unknown package state and never suggests installing it", async () => {
    importDevSetupConfigurationMock.mockResolvedValueOnce({
      ...devSetupConfigurationReviewFixture,
      canApply: false,
      packages: [
        {
          ...devSetupConfigurationReviewFixture.packages[0],
          currentState: "unknown",
          action: "verify",
        },
      ],
    });
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "Dev Setup" }));
    await screen.findByRole("heading", { name: "docker-desktop WSL backend" });
    fireEvent.click(screen.getByRole("button", { name: "구성 가져오기" }));

    expect(await screen.findByText(/확인할 수 없는 패키지 상태가 있어 적용을 차단/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "확인 후 패키지 적용" }).hasAttribute("disabled")).toBe(true);
    expect(screen.getAllByText(/설치를 제안하지 않습니다/).length).toBeGreaterThan(0);
    expect(screen.queryByText("미설치")).toBeNull();
    expect(applyDevSetupConfigurationMock).not.toHaveBeenCalled();
  });

  it("offers cancellation while package apply is in flight and renders resource results", async () => {
    let resolveApply!: (result: DevSetupConfigurationApplyFixture) => void;
    applyDevSetupConfigurationMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveApply = resolve;
        }),
    );
    confirmMock.mockReturnValueOnce(true);
    importDevSetupConfigurationMock.mockResolvedValueOnce(devSetupConfigurationReviewFixture);
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "Dev Setup" }));
    await screen.findByRole("heading", { name: "docker-desktop WSL backend" });
    fireEvent.click(screen.getByRole("button", { name: "구성 가져오기" }));
    await screen.findByText("Git.Git");
    fireEvent.click(screen.getByRole("checkbox", { name: "정규화된 package-only 검토를 확인했습니다" }));
    fireEvent.click(
      screen.getByRole("checkbox", { name: "로컬에 등록된 고정 이름 winget source·패키지 약관 수락을 확인했습니다" }),
    );
    fireEvent.click(screen.getByRole("checkbox", { name: "관리자/UAC·재부팅 위험을 확인했습니다" }));
    fireEvent.click(await screen.findByRole("button", { name: "확인 후 패키지 적용" }));
    await waitFor(() => expect(applyDevSetupConfigurationMock).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "Dev Setup 패키지 적용 취소" }));
    await waitFor(() => expect(cancelDevSetupApplyMock).toHaveBeenCalledTimes(1));

    resolveApply(devSetupConfigurationApplyFixture);
    expect(await screen.findByText("패키지 적용 결과")).toBeTruthy();
    expect(screen.getByText("전체 적용 완료")).toBeTruthy();
    expect(screen.getAllByText("적용 완료").length).toBeGreaterThan(0);
    expect(screen.getAllByText("적용 완료").length).toBeGreaterThanOrEqual(1);
  });

  it("redacts native errors from package configuration actions", async () => {
    importDevSetupConfigurationMock.mockRejectedValueOnce(
      new Error("C:\\Users\\developer\\secret-token=should-not-render"),
    );
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "Dev Setup" }));
    await screen.findByRole("heading", { name: "docker-desktop WSL backend" });
    fireEvent.click(screen.getByRole("button", { name: "구성 가져오기" }));

    expect(await screen.findByText(/WinGet Configuration v3 파일을 불러올 수 없습니다/)).toBeTruthy();
    expect(screen.queryByText(/secret-token/)).toBeNull();
    expect(screen.queryByText(/C:\\Users\\developer/)).toBeNull();
  });

  it("downloads the sanitized export before apply", async () => {
    importDevSetupConfigurationMock.mockResolvedValueOnce(devSetupConfigurationReviewFixture);
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    render(<App />);
    await screen.findByText("Control Center 도구");
    fireEvent.click(screen.getByRole("button", { name: "Dev Setup" }));
    await screen.findByRole("heading", { name: "docker-desktop WSL backend" });
    fireEvent.click(screen.getByRole("button", { name: "구성 가져오기" }));
    fireEvent.click(await screen.findByRole("button", { name: "정규화된 구성 내보내기" }));

    await waitFor(() =>
      expect(exportDevSetupConfigurationMock).toHaveBeenCalledWith(devSetupConfigurationReviewFixture.previewId),
    );
    expect(click).toHaveBeenCalledTimes(1);
    expect(URL.createObjectURL).toHaveBeenCalledTimes(1);
  });

  it("marks a package review expired when its safety window elapses", async () => {
    vi.useFakeTimers();
    try {
      const now = Date.now();
      importDevSetupConfigurationMock.mockResolvedValueOnce({
        ...devSetupConfigurationReviewFixture,
        expiresAtMs: now + 1_000,
      });
      render(<App />);
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(screen.getByText("Control Center 도구")).toBeTruthy();
      fireEvent.click(screen.getByRole("button", { name: "Dev Setup" }));
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(screen.getByRole("heading", { name: "docker-desktop WSL backend" })).toBeTruthy();
      fireEvent.click(screen.getByRole("button", { name: "구성 가져오기" }));
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(screen.getByText("Git.Git")).toBeTruthy();
      expect(screen.getByText("적용 전 검토 가능")).toBeTruthy();

      await act(async () => {
        vi.advanceTimersByTime(1_050);
      });
      expect(screen.getByText("만료됨")).toBeTruthy();
      expect(screen.getByRole("button", { name: "확인 후 패키지 적용" }).hasAttribute("disabled")).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
});
