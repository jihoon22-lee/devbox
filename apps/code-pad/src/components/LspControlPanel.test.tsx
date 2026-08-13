import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  languageServerStatuses,
  installLsp,
  lspCatalog,
  lspInstalled,
  loadLspConfig,
  recoverInstalledLsp,
  saveLspConfig,
  startLanguageServer,
  stopLanguageServer,
  uninstallLsp,
} from "../api";
import type {
  LanguageServerStatus,
  LoadedLspConfig,
  ManagedInstallStatus,
  ManagedServerManifest,
} from "../types";
import LspControlPanel from "./LspControlPanel";

vi.mock("../api", () => ({
  languageServerStatuses: vi.fn(),
  installLsp: vi.fn(),
  lspCatalog: vi.fn(),
  lspInstalled: vi.fn(),
  loadLspConfig: vi.fn(),
  recoverInstalledLsp: vi.fn(),
  saveLspConfig: vi.fn(),
  startLanguageServer: vi.fn(),
  stopLanguageServer: vi.fn(),
  uninstallLsp: vi.fn(),
}));

const loadMock = vi.mocked(loadLspConfig);
const statusesMock = vi.mocked(languageServerStatuses);
const catalogMock = vi.mocked(lspCatalog);
const installedMock = vi.mocked(lspInstalled);
const installMock = vi.mocked(installLsp);
const uninstallMock = vi.mocked(uninstallLsp);
const recoverMock = vi.mocked(recoverInstalledLsp);
const saveMock = vi.mocked(saveLspConfig);
const startMock = vi.mocked(startLanguageServer);
const stopMock = vi.mocked(stopLanguageServer);

function loadedConfig(overrides: Partial<LoadedLspConfig> = {}): LoadedLspConfig {
  return {
    config: {
      version: 1,
      enabled: false,
      workspace_root: "",
      server_by_language: {},
      custom_servers: [],
      update_policy: "manual",
    },
    persist_allowed: true,
    error: null,
    ...overrides,
  };
}

function fixtureManifest(): ManagedServerManifest {
  return {
    id: "rust-analyzer",
    version: "2026-08-10.1",
    platform: "windows-x86_64",
    languages: [{ language_id: "rust", extensions: [".rs"] }],
    source_url: "https://example.com/rust-analyzer",
    license: "MIT",
    artifact: {
      kind: "zip",
      url: "https://example.com/rust-analyzer.zip",
      sha256: "11".repeat(32),
      size_bytes: 42,
      allowed_redirect_hosts: [],
      archive_root: "",
    },
    runtime: { kind: "native", executable: "rust-analyzer.exe", min_version: null },
    command: { executable: "rust-analyzer.exe", args: [] },
    files: { entrypoint: "rust-analyzer.exe", package_lock_sha256: null },
    capabilities_hint: null,
    generated_at: "2026-08-13T00:00:00Z",
  };
}

function fixtureInstallStatus(
  manifest: ManagedServerManifest,
  state: ManagedInstallStatus["state"],
): ManagedInstallStatus {
  return {
    manifest_id: manifest.id,
    version: manifest.version,
    platform: manifest.platform,
    state,
    reason: null,
    installed: state === "installed" ? {
      manifest_id: manifest.id,
      version: manifest.version,
      platform: manifest.platform,
      sha256: manifest.artifact.sha256,
      source_url: manifest.source_url,
      license: manifest.license,
      artifact_url: manifest.artifact.url,
      entrypoint: manifest.files.entrypoint,
      runtime: manifest.runtime,
      installed_at: "2026-08-13T00:00:00Z",
      package_lock_sha256: null,
    } : null,
  };
}

function fixtureServerStatus(
  languageId: string,
  status: LanguageServerStatus["status"],
): LanguageServerStatus {
  return {
    languageId,
    status,
    processState: status,
    serverInfo: null,
    capabilities: {
      positionEncoding: "utf-16",
      legacyPositionEncoding: false,
      syncKind: null,
      openClose: false,
      save: false,
      completion: false,
      hover: false,
      definition: false,
      references: false,
      rename: false,
      formatting: false,
      diagnostics: false,
    },
    documentCount: 0,
    stderr: "",
    stderrTruncated: false,
    stderrDroppedBytes: 0,
  };
}

beforeEach(() => {
  loadMock.mockReset().mockResolvedValue(loadedConfig());
  statusesMock.mockReset().mockResolvedValue([]);
  catalogMock.mockReset().mockResolvedValue([]);
  installedMock.mockReset().mockResolvedValue([]);
  installMock.mockReset().mockResolvedValue(undefined);
  uninstallMock.mockReset().mockResolvedValue(undefined);
  recoverMock.mockReset().mockResolvedValue(undefined);
  saveMock.mockReset().mockResolvedValue(undefined);
  startMock.mockReset().mockResolvedValue(undefined);
  stopMock.mockReset().mockResolvedValue(undefined);
});

afterEach(() => cleanup());

describe("LspControlPanel", () => {
  it("keeps one close action in the footer instead of a duplicate header button", async () => {
    const onClose = vi.fn();
    const rendered = render(<LspControlPanel workspaceRoot={"/work/project"} onClose={onClose} />);
    await rendered.findByText("등록된 언어 서버가 없습니다.");
    expect(rendered.getAllByRole("button", { name: "닫기" })).toHaveLength(1);
    expect(rendered.getByRole("heading", { name: "언어 서버" }).closest("header")?.querySelector("button")).toBeNull();
    fireEvent.click(rendered.getByRole("button", { name: "닫기" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("requires an explicit recovery save for a corrupt config", async () => {
    loadMock.mockResolvedValue(loadedConfig({ persist_allowed: false, error: "invalid JSON" }));
    const rendered = render(<LspControlPanel workspaceRoot={"/work/project"} onClose={() => undefined} />);
    expect(await rendered.findByText(/저장된 설정이 손상되었습니다/)).toBeTruthy();
    fireEvent.click(rendered.getByRole("button", { name: "설정 저장" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledWith(
      expect.objectContaining({ workspace_root: "/work/project" }),
      true,
    ));
  });

  it("stores local server arguments as argv lines without shell parsing", async () => {
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    await rendered.findByText("등록된 언어 서버가 없습니다.");
    await waitFor(() => expect((rendered.getByRole("button", { name: "설정 저장" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.change(rendered.getByLabelText("실행 파일 절대 경로"), {
      target: { value: "C:\\Tools\\server.exe" },
    });
    fireEvent.change(rendered.getByLabelText(/인자 \(한 줄에 하나/), {
      target: { value: "--stdio\n--log file=C:\\my logs\\server.log" },
    });
    fireEvent.click(rendered.getByRole("button", { name: "이 언어 설정 적용" }));
    fireEvent.click(rendered.getByRole("checkbox", { name: "이 작업 폴더에서 언어 서버 사용" }));
    fireEvent.click(rendered.getByRole("button", { name: "설정 저장" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledWith(
      expect.objectContaining({
        enabled: true,
        workspace_root: "C:\\work",
        server_by_language: {
          rust: {
            kind: "local",
            installed_path: "C:\\Tools\\server.exe",
            executable: null,
            args: ["--stdio", "--log file=C:\\my logs\\server.log"],
          },
        },
      }),
      false,
    ));
  });

  it("does not start a server until edited settings are saved", async () => {
    loadMock.mockResolvedValue(loadedConfig({
      config: {
        ...loadedConfig().config,
        enabled: true,
        workspace_root: "C:\\work",
        server_by_language: {
          rust: { kind: "local", installed_path: "C:\\server.exe", args: [] },
        },
      },
    }));
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    const start = await rendered.findByRole("button", { name: "시작" });
    await waitFor(() => expect((start as HTMLButtonElement).disabled).toBe(false));
    fireEvent.change(rendered.getByLabelText("실행 파일 절대 경로"), {
      target: { value: "C:\\new-server.exe" },
    });
    fireEvent.click(rendered.getByRole("button", { name: "이 언어 설정 적용" }));
    expect((start as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(start);
    expect(startMock).not.toHaveBeenCalled();
  });

  it("offers start for stopped or crashed servers and stop only for live sessions", async () => {
    loadMock.mockResolvedValue(loadedConfig({
      config: {
        ...loadedConfig().config,
        enabled: true,
        workspace_root: "C:\\work",
        server_by_language: {
          rust: { kind: "local", installed_path: "C:\\rust.exe", args: [] },
          typescript: { kind: "local", installed_path: "C:\\typescript.exe", args: [] },
        },
      },
    }));
    statusesMock.mockResolvedValue([
      fixtureServerStatus("rust", "crashed"),
      fixtureServerStatus("typescript", "ready"),
    ]);
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    expect(await rendered.findByRole("button", { name: "시작" })).toBeTruthy();
    expect(rendered.getByRole("button", { name: "중지" })).toBeTruthy();
    expect(rendered.getAllByRole("button", { name: "시작" })).toHaveLength(1);
    expect(rendered.getAllByRole("button", { name: "중지" })).toHaveLength(1);
  });

  it("shows reviewed metadata and requires an explicit install confirmation", async () => {
    const manifest: ManagedServerManifest = {
      id: "rust-analyzer",
      version: "2026-08-10.1",
      platform: "windows-x86_64",
      languages: [{ language_id: "rust", extensions: [".rs"] }],
      source_url: "https://github.com/rust-lang/rust-analyzer",
      license: "MIT OR Apache-2.0",
      artifact: {
        kind: "zip",
        url: "https://github.com/rust-lang/rust-analyzer/releases/download/2026-08-10.1/rust-analyzer-x86_64-pc-windows-msvc.zip",
        sha256: "f667620d3af202f480faf9e407374509ebddef3b8611922e463aeaa7e6985fc8",
        size_bytes: 17_430_385,
        allowed_redirect_hosts: ["release-assets.githubusercontent.com"],
        archive_root: "",
      },
      runtime: { kind: "native", executable: "rust-analyzer.exe", min_version: null },
      command: { executable: "rust-analyzer.exe", args: [] },
      files: { entrypoint: "rust-analyzer.exe", package_lock_sha256: null },
      capabilities_hint: null,
      generated_at: "2026-08-12T00:00:00Z",
    };
    const notInstalled: ManagedInstallStatus = {
      manifest_id: manifest.id,
      version: manifest.version,
      platform: manifest.platform,
      state: "not_installed",
      reason: null,
      installed: null,
    };
    catalogMock.mockResolvedValue([manifest]);
    installedMock.mockResolvedValue([notInstalled]);
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    expect(await rendered.findByText(manifest.artifact.sha256)).toBeTruthy();
    expect(rendered.getByText("17,430,385 bytes")).toBeTruthy();
    const install = rendered.getByRole("button", { name: "설치" });
    expect((install as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(install);
    expect(await rendered.findByRole("dialog", { name: "관리형 서버 작업 확인" })).toBeTruthy();
    expect(rendered.getAllByText(manifest.artifact.url).length).toBeGreaterThanOrEqual(2);
    expect(rendered.getByRole("button", { name: "설치 확인" })).toBeTruthy();
    fireEvent.click(rendered.getByRole("button", { name: "설치 확인" }));
    await waitFor(() => expect(installMock).toHaveBeenCalledWith(
      manifest.id,
      manifest.version,
      manifest.platform,
    ));
  });

  it("disables mutation controls for installed entries and exposes reinstall state", async () => {
    const manifest: ManagedServerManifest = {
      id: "fixture-server",
      version: "1.2.3",
      platform: "windows-x86_64",
      languages: [{ language_id: "fixture", extensions: [".fixture"] }],
      source_url: "https://example.com/source",
      license: "MIT",
      artifact: {
        kind: "zip",
        url: "https://example.com/server.zip",
        sha256: "11".repeat(32),
        size_bytes: 42,
        allowed_redirect_hosts: [],
        archive_root: "",
      },
      runtime: { kind: "native", executable: "server.exe", min_version: null },
      command: { executable: "server.exe", args: [] },
      files: { entrypoint: "server.exe", package_lock_sha256: null },
      capabilities_hint: null,
      generated_at: "2026-08-13T00:00:00Z",
    };
    catalogMock.mockResolvedValue([manifest]);
    installedMock.mockResolvedValue([{
      manifest_id: manifest.id,
      version: manifest.version,
      platform: manifest.platform,
      state: "installed",
      reason: null,
      installed: null,
    }]);
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    await rendered.findByText("설치됨");
    expect((rendered.getByRole("button", { name: "최신 버전" }) as HTMLButtonElement).disabled).toBe(true);
    expect((rendered.getByRole("button", { name: "제거" }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(rendered.getByRole("button", { name: "제거" }));
    expect(await rendered.findByRole("button", { name: "제거 확인" })).toBeTruthy();
    expect(rendered.queryByRole("button", { name: "설치 확인" })).toBeNull();
  });

  it("requires removal before repairing a needs-reinstall destination", async () => {
    const manifest: ManagedServerManifest = {
      id: "fixture-server",
      version: "1.2.3",
      platform: "windows-x86_64",
      languages: [{ language_id: "fixture", extensions: [".fixture"] }],
      source_url: "https://example.com/source",
      license: "MIT",
      artifact: {
        kind: "zip",
        url: "https://example.com/server.zip",
        sha256: "11".repeat(32),
        size_bytes: 42,
        allowed_redirect_hosts: [],
        archive_root: "",
      },
      runtime: { kind: "native", executable: "server.exe", min_version: null },
      command: { executable: "server.exe", args: [] },
      files: { entrypoint: "server.exe", package_lock_sha256: null },
      capabilities_hint: null,
      generated_at: "2026-08-13T00:00:00Z",
    };
    catalogMock.mockResolvedValue([manifest]);
    installedMock.mockResolvedValue([{
      manifest_id: manifest.id,
      version: manifest.version,
      platform: manifest.platform,
      state: "needs_reinstall",
      reason: "managed metadata differs",
      installed: {
        manifest_id: manifest.id,
        version: manifest.version,
        platform: manifest.platform,
        sha256: "00".repeat(32),
        source_url: manifest.source_url,
        license: manifest.license,
        artifact_url: manifest.artifact.url,
        entrypoint: manifest.files.entrypoint,
        runtime: manifest.runtime,
        installed_at: "2026-08-13T00:00:00Z",
        package_lock_sha256: null,
      },
    }]);
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    await rendered.findByText("재설치 필요");
    expect((rendered.getByRole("button", { name: "먼저 제거" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(rendered.getByRole("button", { name: "제거" }));
    fireEvent.click(await rendered.findByRole("button", { name: "제거 확인" }));
    await waitFor(() => expect(uninstallMock).toHaveBeenCalledWith(
      manifest.id,
      manifest.version,
      manifest.platform,
    ));
  });

  it("renders catalog-orphaned indexed versions with explicit removal", async () => {
    catalogMock.mockResolvedValue([]);
    installedMock.mockResolvedValue([{
      manifest_id: "old-server",
      version: "0.9.0",
      platform: "windows-x86_64",
      state: "needs_reinstall",
      reason: "installed entry is not present in the reviewed catalog",
      installed: {
        manifest_id: "old-server",
        version: "0.9.0",
        platform: "windows-x86_64",
        sha256: "22".repeat(32),
        source_url: "https://example.com/old-source",
        license: "Apache-2.0",
        artifact_url: "https://example.com/old-server.zip",
        entrypoint: "old-server.exe",
        runtime: { kind: "native", executable: "old-server.exe", min_version: null },
        installed_at: "2026-08-13T00:00:00Z",
        package_lock_sha256: null,
      },
    }]);
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    expect(await rendered.findByText("0.9.0 · windows-x86_64")).toBeTruthy();
    expect(rendered.getByText("https://example.com/old-source")).toBeTruthy();
    expect(rendered.getByText("카탈로그 없음")).toBeTruthy();
    fireEvent.click(rendered.getByRole("button", { name: "제거" }));
    fireEvent.click(await rendered.findByRole("button", { name: "제거 확인" }));
    await waitFor(() => expect(uninstallMock).toHaveBeenCalledWith(
      "old-server",
      "0.9.0",
      "windows-x86_64",
    ));
  });

  it("refreshes managed choices after an explicit install in the same dialog", async () => {
    const manifest = fixtureManifest();
    let installed = false;
    catalogMock.mockResolvedValue([manifest]);
    installedMock.mockImplementation(async () => [fixtureInstallStatus(
      manifest,
      installed ? "installed" : "not_installed",
    )]);
    installMock.mockImplementation(async () => {
      installed = true;
    });
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    fireEvent.click(await rendered.findByRole("button", { name: "설치" }));
    fireEvent.click(await rendered.findByRole("button", { name: "설치 확인" }));
    await waitFor(() => expect(installMock).toHaveBeenCalledWith(
      manifest.id,
      manifest.version,
      manifest.platform,
    ));
    await waitFor(() => expect(
      (rendered.getByRole("option", { name: "설치된 관리형 서버" }) as HTMLOptionElement).disabled,
    ).toBe(false));
    fireEvent.change(rendered.getByLabelText("서버 종류"), { target: { value: "managed" } });
    expect(await rendered.findByRole("option", { name: /rust-analyzer@2026-08-10\.1/ })).toBeTruthy();
  });

  it("persists only the managed id and version, never an install path", async () => {
    const manifest = fixtureManifest();
    catalogMock.mockResolvedValue([manifest]);
    installedMock.mockResolvedValue([fixtureInstallStatus(manifest, "installed")]);
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    await waitFor(() => expect(
      (rendered.getByRole("option", { name: "설치된 관리형 서버" }) as HTMLOptionElement).disabled,
    ).toBe(false));
    fireEvent.change(rendered.getByLabelText("서버 종류"), { target: { value: "managed" } });
    fireEvent.click(rendered.getByRole("button", { name: "이 언어 설정 적용" }));
    fireEvent.click(rendered.getByRole("button", { name: "설정 저장" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledWith(
      expect.objectContaining({
        server_by_language: {
          rust: {
            kind: "managed",
            manifest_id: manifest.id,
            version: manifest.version,
          },
        },
      }),
      false,
    ));
  });

  it("removes a managed choice after an explicit uninstall in the same dialog", async () => {
    const manifest = fixtureManifest();
    let installed = true;
    catalogMock.mockResolvedValue([manifest]);
    installedMock.mockImplementation(async () => [fixtureInstallStatus(
      manifest,
      installed ? "installed" : "not_installed",
    )]);
    uninstallMock.mockImplementation(async () => {
      installed = false;
    });
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    await waitFor(() => expect(
      (rendered.getByRole("option", { name: "설치된 관리형 서버" }) as HTMLOptionElement).disabled,
    ).toBe(false));
    fireEvent.change(await rendered.findByLabelText("서버 종류"), { target: { value: "managed" } });
    expect(await rendered.findByRole("option", { name: /rust-analyzer@2026-08-10\.1/ })).toBeTruthy();
    fireEvent.click(rendered.getByRole("button", { name: "제거" }));
    fireEvent.click(await rendered.findByRole("button", { name: "제거 확인" }));
    await waitFor(() => expect(uninstallMock).toHaveBeenCalledWith(
      manifest.id,
      manifest.version,
      manifest.platform,
    ));
    await waitFor(() => expect(
      rendered.queryByRole("option", { name: /rust-analyzer@2026-08-10\.1/ }),
    ).toBeNull());
  });

  it("does not overwrite an unsaved local form when managed status refreshes", async () => {
    const manifest = fixtureManifest();
    let installed = true;
    catalogMock.mockResolvedValue([manifest]);
    installedMock.mockImplementation(async () => [fixtureInstallStatus(
      manifest,
      installed ? "installed" : "not_installed",
    )]);
    uninstallMock.mockImplementation(async () => {
      installed = false;
    });
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    await waitFor(() => expect(
      (rendered.getByRole("option", { name: "설치된 관리형 서버" }) as HTMLOptionElement).disabled,
    ).toBe(false));
    const executable = rendered.getByLabelText("실행 파일 절대 경로") as HTMLInputElement;
    await waitFor(() => expect(executable.disabled).toBe(false));
    fireEvent.change(executable, { target: { value: "C:\\local\\server.exe" } });
    fireEvent.click(rendered.getByRole("button", { name: "제거" }));
    fireEvent.click(await rendered.findByRole("button", { name: "제거 확인" }));
    await waitFor(() => expect(uninstallMock).toHaveBeenCalled());
    expect((rendered.getByLabelText("실행 파일 절대 경로") as HTMLInputElement).value)
      .toBe("C:\\local\\server.exe");
  });
});
