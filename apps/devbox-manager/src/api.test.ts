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
  catalog,
  devSetupAudit,
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
});
