import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, isTauriMock, openUrlMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  isTauriMock: vi.fn(),
  openUrlMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: openUrlMock }));
vi.mock("./lib/isTauri", () => ({ isTauri: isTauriMock }));

import {
  available,
  applyDevSetupConfiguration,
  catalog,
  cancelDevSetupApply,
  devSetupAudit,
  discardDevSetupConfiguration,
  exportDevSetupConfiguration,
  importDevSetupConfiguration,
  inspectLocalQuality,
  installRelatedTool,
  launchRelatedTool,
  openRelatedToolUrl,
  relatedTools,
} from "./api";

beforeEach(() => {
  invokeMock.mockReset();
  openUrlMock.mockReset();
  isTauriMock.mockReset().mockReturnValue(false);
});

describe("Browser release fallback", () => {
  it("tracks the stable v0.5.0 manifest for the 14 managed apps", async () => {
    const catalogApps = await catalog();
    const manifest = await available();
    const managedIds = catalogApps
      .filter((app) => app.managerVisible && !app.selfManaged)
      .map((app) => app.id);

    expect(catalogApps).toHaveLength(15);
    expect(manifest.releaseTag).toBe("v0.5.0");
    expect(manifest.generatedAt).toBe("2026-08-28T23:45:52Z");
    expect(manifest.apps).toHaveLength(14);
    expect(manifest.apps.map((app) => app.id)).toEqual(managedIds);
    expect(manifest.apps.some((app) => app.id === "devbox-manager")).toBe(false);
    expect(manifest.apps.some((app) => app.id === "devbox-launcher")).toBe(true);
    expect(manifest.apps.some((app) => app.id === "log-lens")).toBe(true);

    const expectedVersions: Record<string, string> = {
      "port-manager": "0.3.0",
      "developer-toolbox": "0.3.0",
      "wsl-desktop": "0.4.0",
      "api-playground": "0.4.0",
      "everything-plus": "0.4.0",
      "knowledge-base": "0.4.0",
      "life-log": "0.4.0",
      "code-pad": "0.4.0",
      "run-manager": "0.4.0",
      workbench: "0.2.0",
      "webhook-lab": "0.2.0",
      "repo-manager": "0.2.0",
      "devbox-launcher": "0.1.0",
      "log-lens": "0.1.0",
    };
    for (const [id, version] of Object.entries(expectedVersions)) {
      const app = manifest.apps.find((candidate) => candidate.id === id);
      expect(app).toMatchObject({
        id,
        version,
        portable: { name: `${id}.exe` },
        installer: { name: `${id}_${version}_x64-setup.exe` },
      });
    }
  });
});

describe("Local quality API boundary", () => {
  it("returns a bounded path-free local-only browser snapshot", async () => {
    const snapshot = await inspectLocalQuality();

    expect(snapshot).toMatchObject({
      schemaVersion: 1,
      mode: "local-only",
      status: "healthy",
      installation: { catalogState: "ready", registryState: "ready" },
      integration: { rootState: "ready", issueCount: 0 },
    });
    expect(snapshot.installation.apps).toHaveLength(14);
    expect(snapshot.integration.snapshots).toHaveLength(1);
    expect(JSON.stringify(snapshot)).not.toMatch(/[A-Z]:\\|\/home\/|\\Users\\/);
  });

  it("rejects path-bearing fields and contradictory registry counts", async () => {
    const valid = await inspectLocalQuality();
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce({
      ...valid,
      integration: {
        ...valid.integration,
        snapshots: valid.integration.snapshots.map((snapshot) => ({
          ...snapshot,
          path: "C:\\Users\\private\\summary.json",
        })),
      },
    });
    await expect(inspectLocalQuality()).rejects.toThrow("로컬 품질 응답이 올바르지 않습니다.");

    invokeMock.mockResolvedValueOnce({
      ...valid,
      installation: {
        ...valid.installation,
        installedAppCount: 2,
      },
    });
    await expect(inspectLocalQuality()).rejects.toThrow("로컬 품질 응답이 올바르지 않습니다.");

    invokeMock.mockResolvedValueOnce({
      ...valid,
      installation: {
        ...valid.installation,
        managedAppCount: 0,
        installedAppCount: 0,
        apps: [],
      },
    });
    await expect(inspectLocalQuality()).rejects.toThrow("로컬 품질 응답이 올바르지 않습니다.");

    invokeMock.mockResolvedValueOnce({
      ...valid,
      installation: {
        ...valid.installation,
        apps: valid.installation.apps.map((app) => app.state === "installed"
          ? { ...app, version: "1.2.3-01" }
          : app),
      },
    });
    await expect(inspectLocalQuality()).rejects.toThrow("로컬 품질 응답이 올바르지 않습니다.");
  });

  it("keeps an unavailable registry unknown instead of claiming apps are absent", async () => {
    const valid = await inspectLocalQuality();
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce({
      ...valid,
      status: "attention",
      installation: {
        ...valid.installation,
        registryState: "unavailable",
        registryRevision: null,
        installedAppCount: null,
        apps: valid.installation.apps.map((app) => ({
          ...app,
          state: "unknown",
          version: null,
          mode: null,
        })),
      },
    });

    const snapshot = await inspectLocalQuality();
    expect(snapshot.installation.apps.every((app) => app.state === "unknown")).toBe(true);
    expect(snapshot.installation.apps.some((app) => app.state === "not-installed")).toBe(false);
  });

  it("rejects partial results when the integration root itself is unavailable", async () => {
    const valid = await inspectLocalQuality();
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce({
      ...valid,
      status: "attention",
      integration: {
        ...valid.integration,
        rootState: "unavailable",
        rootIssue: "unreadable",
      },
    });

    await expect(inspectLocalQuality()).rejects.toThrow("로컬 품질 응답이 올바르지 않습니다.");

    invokeMock.mockResolvedValueOnce({
      ...valid,
      status: "attention",
      integration: {
        rootState: "unavailable",
        rootIssue: "unsafe",
        snapshotCount: 0,
        issueCount: 0,
        snapshots: [],
        issues: [],
        snapshotsTruncated: false,
        issuesTruncated: false,
      },
    });
    await expect(inspectLocalQuality()).resolves.toMatchObject({
      status: "attention",
      integration: { rootState: "unavailable", rootIssue: "unsafe" },
    });
  });
});

describe("Related Tools API boundary", () => {
  it("rejects tampered catalog metadata and impossible platform state", async () => {
    const valid = await relatedTools();
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce(valid.map((tool, index) => index === 0
      ? { ...tool, officialUrl: "https://evil.example/download" }
      : tool));
    await expect(relatedTools()).rejects.toThrow("관련 도구 감지 응답이 올바르지 않습니다.");

    invokeMock.mockResolvedValueOnce(valid.map((tool, index) => index === 0
      ? { ...tool, platformSupported: false, installed: true, detection: "path" }
      : tool));
    await expect(relatedTools()).rejects.toThrow("관련 도구 감지 응답이 올바르지 않습니다.");
  });

  it("accepts backend-only Docker evidence without converting unknown to absent", async () => {
    const valid = await relatedTools();
    const docker = valid.find((tool) => tool.id === "docker-desktop");
    if (!docker?.dockerCapability) throw new Error("Docker fixture missing");
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce(valid.map((tool) => tool.id === "docker-desktop" ? {
      ...tool,
      platformSupported: true,
      detection: "not-found",
      installState: "unknown",
      launchState: "unavailable",
      dockerCapability: {
        ...docker.dockerCapability,
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
      },
    } : tool));

    const result = await relatedTools();
    const capability = result.find((tool) => tool.id === "docker-desktop");
    expect(capability?.installState).toBe("unknown");
    expect(capability?.dockerCapability?.wslBackend).toBe("running");
    expect(capability?.installed).toBe(false);
  });

  it("rejects contradictory Docker evidence", async () => {
    const valid = await relatedTools();
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce(valid.map((tool) => tool.id === "docker-desktop" ? {
      ...tool,
      dockerCapability: tool.dockerCapability && {
        ...tool.dockerCapability,
        desktopInstall: "present",
      },
      installState: "present",
      installed: true,
    } : tool));
    await expect(relatedTools()).rejects.toThrow("Docker capability 응답이 올바르지 않습니다.");
  });

  it("rejects contradictory WSL registration and runtime evidence", async () => {
    const valid = await relatedTools();
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce(valid.map((tool) => tool.id === "docker-desktop" ? {
      ...tool,
      dockerCapability: tool.dockerCapability && {
        ...tool.dockerCapability,
        wslBackend: "absent",
        evidence: tool.dockerCapability.evidence.map((evidence) => (
          evidence.source === "wsl-runtime" ? { ...evidence, result: "running" } : evidence
        )),
      },
    } : tool));
    await expect(relatedTools()).rejects.toThrow("Docker capability 응답이 올바르지 않습니다.");
  });

  it("accepts only the exact requested action and replaces native message text", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce({
      toolId: "vs-code",
      status: "installed",
      message: "C:\\Users\\developer\\secret-token",
    });
    await expect(installRelatedTool("vs-code", true)).resolves.toEqual({
      toolId: "vs-code",
      status: "installed",
      message: "WinGet 설치가 완료되었습니다.",
    });
    expect(invokeMock).toHaveBeenCalledWith("install_related_tool", {
      request: { toolId: "vs-code", confirmed: true },
    });

    invokeMock.mockResolvedValueOnce({ toolId: "another-tool", status: "launched" });
    await expect(launchRelatedTool("vs-code"))
      .rejects.toThrow("관련 도구 작업 결과가 올바르지 않습니다.");
  });

  it("rejects arbitrary ids and external URLs before a native call", async () => {
    isTauriMock.mockReturnValue(true);
    await expect(installRelatedTool("vs-code --silent", true))
      .rejects.toThrow("관련 도구 식별자가 올바르지 않습니다.");
    await expect(openRelatedToolUrl("https://evil.example/tool"))
      .rejects.toThrow("공식 링크가 올바르지 않습니다.");
    expect(invokeMock).not.toHaveBeenCalled();
    expect(openUrlMock).not.toHaveBeenCalled();
  });
});

describe("Dev Setup API boundary", () => {
  it("returns only the fixed read-only audit contract", async () => {
    const audit = await devSetupAudit();
    expect(audit.mode).toBe("read-only");
    expect(audit.capabilities.map((capability) => capability.id)).toEqual([
      "docker-desktop-install",
      "docker-desktop-launch",
      "docker-windows-cli",
      "docker-wsl-backend",
      "winget",
    ]);
    expect(audit.plan.every((item) => item.action === "verify-installation")).toBe(true);
  });

  it("rejects native plan text and state contradictions", async () => {
    const valid = await devSetupAudit();
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce({
      ...valid,
      plan: valid.plan.map((item, index) => index === 0
        ? { ...item, action: "run-arbitrary-command" }
        : item),
    });
    await expect(devSetupAudit()).rejects.toThrow("Dev Setup 감사 응답이 올바르지 않습니다.");

    invokeMock.mockResolvedValueOnce({
      ...valid,
      capabilities: valid.capabilities.map((capability, index) => index === 0
        ? { ...capability, evidence: [{ source: "path", result: "C:\\Users\\secret" }] }
        : capability),
    });
    await expect(devSetupAudit()).rejects.toThrow("환경 capability 응답이 올바르지 않습니다.");
  });

  it("provides a safe two-package browser review, export, and apply flow", async () => {
    const review = await importDevSetupConfiguration();
    expect(review).not.toBeNull();
    expect(review?.schemaVersion).toBe("0.3");
    expect(review?.previewId).toMatch(/^devsetup-[0-9a-f]{64}$/);
    expect(review?.packages.map((pkg) => pkg.packageId)).toEqual([
      "Git.Git",
      "Microsoft.VisualStudioCode",
    ]);
    expect(review?.canApply).toBe(true);
    expect(review?.hasChanges).toBe(true);

    const exported = await exportDevSetupConfiguration(review!.previewId);
    expect(exported.filename).toBe("devbox-packages.winget");
    expect(exported.mimeType).toBe("application/yaml;charset=utf-8");
    expect(exported.content).toContain("Microsoft.WinGet/Package");
    expect(exported.content).toContain('id: "Git.Git"');
    expect(exported.content).toContain('id: "Microsoft.VisualStudioCode"');
    expect(exported.byteCount).toBe(new TextEncoder().encode(exported.content).byteLength);
    expect(exported.sha256).toMatch(/^[0-9a-f]{64}$/);

    const applied = await applyDevSetupConfiguration(
      review!.previewId,
      true,
      true,
      true,
    );
    expect(applied).toMatchObject({
      status: "complete",
      results: [
        { packageId: "Git.Git", status: "applied" },
        { packageId: "Microsoft.VisualStudioCode", status: "unchanged" },
      ],
    });
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("invokes the native configuration commands with the exact request shape", async () => {
    const browserReview = await importDevSetupConfiguration();
    expect(browserReview).not.toBeNull();
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce(browserReview);
    await expect(importDevSetupConfiguration()).resolves.toEqual(browserReview);
    expect(invokeMock).toHaveBeenLastCalledWith("import_dev_setup_configuration");

    isTauriMock.mockReturnValue(false);
    const browserExport = await exportDevSetupConfiguration(browserReview!.previewId);
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce(browserExport);
    await expect(exportDevSetupConfiguration(browserReview!.previewId)).resolves.toEqual(browserExport);
    expect(invokeMock).toHaveBeenLastCalledWith("export_dev_setup_configuration", {
      request: { previewId: browserReview!.previewId },
    });

    const nativeApply = {
      status: "complete",
      observedAtMs: Date.now(),
      results: browserReview!.packages.map((pkg) => ({
        packageId: pkg.packageId,
        status: pkg.action === "none" ? "unchanged" : "applied",
      })),
    };
    invokeMock.mockResolvedValueOnce(nativeApply);
    await expect(applyDevSetupConfiguration(
      browserReview!.previewId,
      true,
      true,
      true,
    )).resolves.toEqual(nativeApply);
    expect(invokeMock).toHaveBeenLastCalledWith("apply_dev_setup_configuration", {
      request: {
        previewId: browserReview!.previewId,
        confirmed: true,
        acceptPackageAgreements: true,
        acknowledgeAdminAndReboot: true,
      },
    });

    invokeMock.mockResolvedValueOnce(undefined);
    await expect(cancelDevSetupApply()).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenLastCalledWith("cancel_dev_setup_apply");

    isTauriMock.mockReturnValue(false);
    const nextReview = await importDevSetupConfiguration();
    expect(nextReview).not.toBeNull();
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce(undefined);
    await expect(discardDevSetupConfiguration(nextReview!.previewId)).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenLastCalledWith("discard_dev_setup_configuration", {
      request: { previewId: nextReview!.previewId },
    });
    await expect(exportDevSetupConfiguration(nextReview!.previewId))
      .rejects.toThrow("Dev Setup 구성 요청이 올바르지 않습니다.");
  });

  it("rejects review schema, package coherence, and unsafe preview fields", async () => {
    const valid = await importDevSetupConfiguration();
    expect(valid).not.toBeNull();
    isTauriMock.mockReturnValue(true);

    invokeMock.mockResolvedValueOnce({ ...valid, schemaVersion: "0.2" });
    await expect(importDevSetupConfiguration()).rejects.toThrow("Dev Setup 구성 검토 응답이 올바르지 않습니다.");

    invokeMock.mockResolvedValueOnce({
      ...valid,
      previewId: "devsetup-" + "A".repeat(64),
    });
    await expect(importDevSetupConfiguration()).rejects.toThrow("Dev Setup 구성 검토 응답이 올바르지 않습니다.");

    invokeMock.mockResolvedValueOnce({
      ...valid,
      packages: valid!.packages.map((pkg, index) => index === 0
        ? { ...pkg, currentState: "unknown", action: "install" }
        : pkg),
    });
    await expect(importDevSetupConfiguration()).rejects.toThrow("Dev Setup 구성 검토 응답이 올바르지 않습니다.");

    invokeMock.mockResolvedValueOnce({
      ...valid,
      packages: valid!.packages.map((pkg) => ({
        ...pkg,
        packageId: "Git.Git",
      })),
    });
    await expect(importDevSetupConfiguration()).rejects.toThrow("Dev Setup 구성 검토 응답이 올바르지 않습니다.");

    invokeMock.mockResolvedValueOnce({
      ...valid,
      packages: valid!.packages.map((pkg, index) => index === 0
        ? { ...pkg, packageId: "--help.Package" }
        : pkg),
    });
    await expect(importDevSetupConfiguration()).rejects.toThrow("Dev Setup 구성 검토 응답이 올바르지 않습니다.");

    invokeMock.mockResolvedValueOnce({
      ...valid,
      packages: valid!.packages.map((pkg, index) => index === 1
        ? { ...pkg, currentState: "update-available", action: "update" }
        : pkg),
    });
    await expect(importDevSetupConfiguration()).rejects.toThrow("Dev Setup 구성 검토 응답이 올바르지 않습니다.");
  });

  it("requires an exact reviewed package list and coherent apply results", async () => {
    const review = await importDevSetupConfiguration();
    const validExport = await exportDevSetupConfiguration(review!.previewId);
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce({
      ...validExport,
      filename: "C:\\Users\\developer\\secret.winget",
    });
    await expect(exportDevSetupConfiguration(review!.previewId)).rejects.toThrow(
      "Dev Setup 구성 내보내기 응답이 올바르지 않습니다.",
    );

    invokeMock.mockResolvedValueOnce({
      status: "complete",
      observedAtMs: Date.now(),
      results: [
        { packageId: "Microsoft.VisualStudioCode", status: "applied" },
        { packageId: "Git.Git", status: "unchanged" },
      ],
    });
    await expect(applyDevSetupConfiguration(review!.previewId, true, true, true)).rejects.toThrow(
      "Dev Setup 구성 적용 결과가 올바르지 않습니다.",
    );
  });

  it("validates confirmations before native invocation and consumes failed applies", async () => {
    const review = await importDevSetupConfiguration();
    isTauriMock.mockReturnValue(true);
    await expect(applyDevSetupConfiguration(review!.previewId, true, false, true))
      .rejects.toThrow("Dev Setup 적용에는 세 가지 확인이 모두 필요합니다.");
    expect(invokeMock).not.toHaveBeenCalled();

    invokeMock.mockRejectedValueOnce(new Error("C:\\Users\\developer\\secret-token"));
    await expect(applyDevSetupConfiguration(review!.previewId, true, true, true))
      .rejects.toThrow("Dev Setup 구성 작업을 완료할 수 없습니다.");
    await expect(exportDevSetupConfiguration(review!.previewId))
      .rejects.toThrow("Dev Setup 구성 요청이 올바르지 않습니다.");
  });
});
