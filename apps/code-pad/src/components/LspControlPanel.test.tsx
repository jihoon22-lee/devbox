import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  importLspArchives,
  languageServerLogs,
  languageServerStatuses,
  installLsp,
  lspCatalog,
  lspInstalled,
  loadLspConfig,
  pickLspArchives,
  recoverInstalledLsp,
  restartLanguageServer,
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
import ManagedInstallerPanel from "./ManagedInstallerPanel";

vi.mock("../api", () => ({
  importLspArchives: vi.fn(),
  languageServerLogs: vi.fn(),
  languageServerStatuses: vi.fn(),
  installLsp: vi.fn(),
  lspCatalog: vi.fn(),
  lspInstalled: vi.fn(),
  loadLspConfig: vi.fn(),
  pickLspArchives: vi.fn(),
  recoverInstalledLsp: vi.fn(),
  saveLspConfig: vi.fn(),
  startLanguageServer: vi.fn(),
  stopLanguageServer: vi.fn(),
  uninstallLsp: vi.fn(),
  restartLanguageServer: vi.fn(),
}));

const loadMock = vi.mocked(loadLspConfig);
const logsMock = vi.mocked(languageServerLogs);
const statusesMock = vi.mocked(languageServerStatuses);
const catalogMock = vi.mocked(lspCatalog);
const installedMock = vi.mocked(lspInstalled);
const installMock = vi.mocked(installLsp);
const importMock = vi.mocked(importLspArchives);
const pickArchiveMock = vi.mocked(pickLspArchives);
const uninstallMock = vi.mocked(uninstallLsp);
const recoverMock = vi.mocked(recoverInstalledLsp);
const saveMock = vi.mocked(saveLspConfig);
const startMock = vi.mocked(startLanguageServer);
const stopMock = vi.mocked(stopLanguageServer);
const restartMock = vi.mocked(restartLanguageServer);

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
      install_source: "local_archive",
      last_verified_at: "2026-08-13T00:00:00Z",
    } : null,
    archive_cached: false,
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
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

beforeEach(() => {
  loadMock.mockReset().mockResolvedValue(loadedConfig());
  logsMock.mockReset().mockResolvedValue([]);
  statusesMock.mockReset().mockResolvedValue([]);
  catalogMock.mockReset().mockResolvedValue([]);
  installedMock.mockReset().mockResolvedValue([]);
  installMock.mockReset().mockResolvedValue(undefined);
  importMock.mockReset().mockResolvedValue(undefined);
  pickArchiveMock.mockReset().mockResolvedValue([]);
  uninstallMock.mockReset().mockResolvedValue(undefined);
  recoverMock.mockReset().mockResolvedValue(undefined);
  saveMock.mockReset().mockResolvedValue(undefined);
  startMock.mockReset().mockResolvedValue(undefined);
  stopMock.mockReset().mockResolvedValue(undefined);
  restartMock.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
  cleanup();
});

describe("LspControlPanel", () => {
  it("keeps WSL editing available while explicitly disabling host LSP", async () => {
    const rendered = render(
      <LspControlPanel
        workspaceRoot="\\\\wsl.localhost\\Ubuntu\\home\\jihoon\\project"
        workspaceCapabilities={{
          path: "\\\\wsl.localhost\\Ubuntu\\home\\jihoon\\project",
          sourceKind: "wsl",
          watchMode: "polling",
          editSupported: true,
          lspSupported: false,
          lspReason: "host_lsp_wsl_unsupported",
        }}
        onClose={() => undefined}
      />,
    );

    expect(await rendered.findByText(/WSL 작업 폴더의 편집과 파일 감시는 지원/u)).toBeTruthy();
    expect((rendered.getByRole("checkbox", {
      name: "이 작업 폴더에서 언어 서버 사용",
    }) as HTMLInputElement).disabled).toBe(true);
  });

  it("keeps one close action in the footer instead of a duplicate header button", async () => {
    const onClose = vi.fn();
    const rendered = render(<LspControlPanel workspaceRoot={"/work/project"} onClose={onClose} />);
    await rendered.findByText("등록된 언어 서버가 없습니다.");
    expect(rendered.getAllByRole("button", { name: "닫기" })).toHaveLength(1);
    expect(rendered.getByRole("heading", { name: "언어 서버" }).closest("header")?.querySelector("button")).toBeNull();
    fireEvent.click(rendered.getByRole("button", { name: "닫기" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("traps focus in the LSP dialog, closes with Escape, and restores its opener", async () => {
    const opener = document.createElement("button");
    opener.textContent = "LSP 열기";
    document.body.append(opener);
    opener.focus();
    const onClose = vi.fn();
    const rendered = render(<LspControlPanel workspaceRoot={"/work/project"} onClose={onClose} />);
    const dialog = rendered.getByRole("dialog", { name: "언어 서버 설정" });
    await rendered.findByText("등록된 언어 서버가 없습니다.");
    await waitFor(() => expect(dialog.contains(document.activeElement)).toBe(true));

    const focusable = Array.from(dialog.querySelectorAll<HTMLElement>(
      "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
    ));
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    expect(first).toBeTruthy();
    expect(last).toBeTruthy();
    last?.focus();
    fireEvent.keyDown(last as HTMLElement, { key: "Tab" });
    expect(document.activeElement).toBe(first);

    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    rendered.unmount();
    expect(document.activeElement).toBe(opener);
    opener.remove();
  });

  it("requires an explicit recovery save for a corrupt config", async () => {
    loadMock.mockResolvedValue(loadedConfig({ persist_allowed: false, error: "invalid JSON" }));
    const rendered = render(<LspControlPanel workspaceRoot={"/work/project"} onClose={() => undefined} />);
    expect(await rendered.findByText(/저장된 설정이 손상되었습니다/)).toBeTruthy();
    expect(rendered.queryByText("invalid JSON")).toBeNull();
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

  it("offers restart for crashed servers and stop only for live sessions", async () => {
    loadMock.mockResolvedValue(loadedConfig({
      config: {
        ...loadedConfig().config,
        enabled: true,
        workspace_root: "C:\\work",
        server_by_language: {
          rust: { kind: "local", installed_path: "C:\\rust.exe", args: [] },
          typescript: { kind: "local", installed_path: "C:\\typescript.exe", args: [] },
          python: { kind: "local", installed_path: "C:\\python.exe", args: [] },
        },
      },
    }));
    statusesMock.mockResolvedValue([
      fixtureServerStatus("rust", "crashed"),
      fixtureServerStatus("typescript", "ready"),
      fixtureServerStatus("python", "starting"),
    ]);
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    expect(await rendered.findByRole("button", { name: "다시 시도" })).toBeTruthy();
    expect(rendered.getByRole("button", { name: "시작 중…" })).toBeTruthy();
    expect(rendered.getAllByRole("button", { name: "중지" })).toHaveLength(3);
    expect(rendered.queryByRole("button", { name: "시작" })).toBeNull();
  });

  it("allows an in-progress manual start to be cancelled without waiting for start to finish", async () => {
    const pendingStart = deferred<void>();
    loadMock.mockResolvedValue(loadedConfig({
      config: {
        ...loadedConfig().config,
        enabled: true,
        workspace_root: "C:\\work",
        server_by_language: {
          rust: { kind: "local", installed_path: "C:\\rust.exe", args: [] },
        },
      },
    }));
    statusesMock.mockResolvedValueOnce([]);
    startMock.mockImplementation(() => pendingStart.promise);
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    const start = await rendered.findByRole("button", { name: "시작" });
    fireEvent.click(start);
    await waitFor(() => expect(startMock).toHaveBeenCalledWith("rust"));

    statusesMock.mockResolvedValue([fixtureServerStatus("rust", "starting")]);
    const stop = await rendered.findByRole("button", { name: "중지" }, { timeout: 3_000 });
    expect((stop as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(stop);
    await act(async () => {
      await Promise.resolve();
    });
    expect(stopMock).toHaveBeenCalledWith("rust");

    await act(async () => {
      pendingStart.resolve();
      await pendingStart.promise;
    });
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
      archive_cached: false,
    };
    catalogMock.mockResolvedValue([manifest]);
    installedMock.mockResolvedValue([notInstalled]);
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    expect(await rendered.findByText(manifest.artifact.sha256)).toBeTruthy();
    expect(rendered.getByText("17,430,385 bytes")).toBeTruthy();
    const install = rendered.getByRole("button", { name: "설치" });
    expect((install as HTMLButtonElement).disabled).toBe(false);
    install.focus();
    fireEvent.click(install);
    const confirmation = await rendered.findByRole("dialog", { name: "관리형 서버 작업 확인" });
    const cancel = rendered.getByRole("button", { name: "취소" });
    await waitFor(() => expect(document.activeElement).toBe(cancel));
    const confirm = rendered.getByRole("button", { name: "설치 확인" });
    confirm.focus();
    fireEvent.keyDown(confirm, { key: "Tab" });
    expect(document.activeElement).toBe(cancel);
    fireEvent.keyDown(confirmation, { key: "Escape" });
    await waitFor(() => expect(rendered.queryByRole("dialog", { name: "관리형 서버 작업 확인" })).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(install));

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

  it("keeps the selected local archive private and imports it only after confirmation", async () => {
    const manifest = fixtureManifest();
    const notInstalled = fixtureInstallStatus(manifest, "not_installed");
    catalogMock.mockResolvedValue([manifest]);
    installedMock.mockResolvedValue([notInstalled]);
    pickArchiveMock.mockResolvedValue(["C:\\Users\\alice\\private-server.zip"]);

    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    fireEvent.click(await rendered.findByRole("button", { name: "local archive 가져오기" }));
    expect(await rendered.findByRole("dialog", { name: "관리형 서버 작업 확인" })).toBeTruthy();
    expect(rendered.queryByText("C:\\Users\\alice\\private-server.zip")).toBeNull();
    expect(importMock).not.toHaveBeenCalled();

    fireEvent.click(rendered.getByRole("button", { name: "가져오기 확인" }));
    await waitFor(() => expect(importMock).toHaveBeenCalledWith(
      manifest.id,
      manifest.version,
      manifest.platform,
      ["C:\\Users\\alice\\private-server.zip"],
    ));
  });

  it("passes a Node archive set without exposing the selected paths", async () => {
    const manifest = fixtureManifest();
    manifest.runtime = { kind: "node", executable: "node", min_version: ">=22" };
    const notInstalled = fixtureInstallStatus(manifest, "not_installed");
    catalogMock.mockResolvedValue([manifest]);
    installedMock.mockResolvedValue([notInstalled]);
    pickArchiveMock.mockResolvedValue([
      "C:\\Users\\alice\\private-server.tgz",
      "C:\\Users\\alice\\private-dependency.tgz",
    ]);

    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    fireEvent.click(await rendered.findByRole("button", { name: "local archive 가져오기" }));
    expect(await rendered.findByRole("dialog", { name: "관리형 서버 작업 확인" })).toBeTruthy();
    expect(rendered.queryByText("C:\\Users\\alice\\private-server.tgz")).toBeNull();
    expect(rendered.queryByText("C:\\Users\\alice\\private-dependency.tgz")).toBeNull();

    fireEvent.click(rendered.getByRole("button", { name: "가져오기 확인" }));
    await waitFor(() => expect(importMock).toHaveBeenCalledWith(
      manifest.id,
      manifest.version,
      manifest.platform,
      [
        "C:\\Users\\alice\\private-server.tgz",
        "C:\\Users\\alice\\private-dependency.tgz",
      ],
    ));
  });

  it("serializes archive picking and the confirmed import mutation", async () => {
    const manifest = fixtureManifest();
    catalogMock.mockResolvedValue([manifest]);
    installedMock.mockResolvedValue([fixtureInstallStatus(manifest, "not_installed")]);
    const pendingPick = deferred<string[]>();
    const pendingImport = deferred<void>();
    pickArchiveMock.mockImplementation(() => pendingPick.promise);
    importMock.mockImplementation(() => pendingImport.promise);

    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    const choose = await rendered.findByRole("button", { name: "local archive 가져오기" });
    fireEvent.click(choose);
    fireEvent.click(choose);
    expect(pickArchiveMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      pendingPick.resolve(["C:\\Users\\alice\\private-server.zip"]);
      await pendingPick.promise;
    });
    const confirm = await rendered.findByRole("button", { name: "가져오기 확인" });
    fireEvent.click(confirm);
    fireEvent.click(confirm);
    expect(importMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      pendingImport.resolve();
      await pendingImport.promise;
    });
  });

  it("drops a managed installer refresh that completes after unmount", async () => {
    const pendingCatalog = deferred<ManagedServerManifest[]>();
    catalogMock.mockImplementation(() => pendingCatalog.promise);
    const rendered = render(<ManagedInstallerPanel />);
    await waitFor(() => expect(catalogMock).toHaveBeenCalledTimes(1));
    rendered.unmount();

    await act(async () => {
      pendingCatalog.resolve([fixtureManifest()]);
      await pendingCatalog.promise;
    });
    expect(installedMock).not.toHaveBeenCalled();
  });

  it("shows a verified archive cache as an offline install option", async () => {
    const manifest = fixtureManifest();
    const cached = fixtureInstallStatus(manifest, "not_installed");
    cached.archive_cached = true;
    catalogMock.mockResolvedValue([manifest]);
    installedMock.mockResolvedValue([cached]);

    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    expect(await rendered.findByText("검증된 archive cache를 오프라인에서 사용할 수 있습니다.")).toBeTruthy();
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
      archive_cached: false,
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
        install_source: "network",
        last_verified_at: "2026-08-13T00:00:00Z",
      },
      archive_cached: false,
    }]);
    const rendered = render(<LspControlPanel workspaceRoot={"C:\\work"} onClose={() => undefined} />);
    await rendered.findByText("재설치 필요");
    expect(rendered.queryByText("managed metadata differs")).toBeNull();
    expect(rendered.getByText(/metadata 검증에 실패했습니다/)).toBeTruthy();
    expect((rendered.getByRole("button", { name: "먼저 제거" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(rendered.getByRole("button", { name: "제거" }));
    fireEvent.click(await rendered.findByRole("button", { name: "제거 확인" }));
    await waitFor(() => expect(uninstallMock).toHaveBeenCalledWith(
      manifest.id,
      manifest.version,
      manifest.platform,
    ));
  });

  it("offers explicit index recovery from only the safe native signal", async () => {
    installedMock.mockRejectedValue(new Error("관리형 서버 설치 목록 복구가 필요합니다"));
    const rendered = render(<LspControlPanel workspaceRoot="/work" onClose={() => undefined} />);
    const recover = await rendered.findByRole("button", { name: "설치 목록 명시적 복구" });
    expect(rendered.queryByText(/installed server index is corrupt/i)).toBeNull();
    fireEvent.click(recover);
    await waitFor(() => expect(recoverMock).toHaveBeenCalledTimes(1));
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
        install_source: "network",
        last_verified_at: "2026-08-13T00:00:00Z",
      },
      archive_cached: false,
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
    await waitFor(() => expect(
      (rendered.getByLabelText("관리형 서버 버전") as HTMLSelectElement).value,
    ).toBe(`${manifest.id}\u001f${manifest.version}`));
    fireEvent.click(rendered.getByRole("button", { name: "이 언어 설정 적용" }));
    await waitFor(() => expect(rendered.getByText("managed")).toBeTruthy());
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

  it("offers restart for degraded or circuit-open servers and start only for stopped servers", async () => {
    loadMock.mockResolvedValue(loadedConfig({
      config: {
        ...loadedConfig().config,
        enabled: true,
        workspace_root: "/work",
        server_by_language: {
          rust: { kind: "local", installed_path: "/server", args: [] },
        },
      },
    }));
    statusesMock.mockResolvedValue([{
      languageId: "rust",
      status: "degraded",
      processState: "failed",
      serverInfo: null,
      capabilities: {
        positionEncoding: "utf-16",
        legacyPositionEncoding: false,
        syncKind: "full",
        openClose: true,
        save: true,
        completion: true,
        hover: true,
        definition: true,
        references: true,
        rename: true,
        formatting: true,
        diagnostics: true,
      },
      documentCount: 0,
      autoRestartDisabled: true,
    }]);
    const rendered = render(<LspControlPanel workspaceRoot="/work" onClose={() => undefined} />);
    const restart = await rendered.findByRole("button", { name: "다시 시도" });
    fireEvent.click(restart);
    await waitFor(() => expect(restartMock).toHaveBeenCalledWith("rust"));
  });

  it("shows bounded runtime logs, retry timing, and verified managed cache state", async () => {
    const manifest = fixtureManifest();
    loadMock.mockResolvedValue(loadedConfig({
      config: {
        ...loadedConfig().config,
        enabled: true,
        workspace_root: "/work",
        server_by_language: {
          rust: { kind: "managed", manifest_id: manifest.id, version: manifest.version },
        },
      },
    }));
    catalogMock.mockResolvedValue([manifest]);
    installedMock.mockResolvedValue([fixtureInstallStatus(manifest, "installed")]);
    statusesMock.mockResolvedValue([{
      ...fixtureServerStatus("rust", "degraded"),
      restartFailures: 2,
      restartDelayMs: 2_400,
      autoRestartDisabled: false,
    }]);
    logsMock.mockResolvedValue([{
      languageId: "rust",
      entries: [{
        sequence: "7",
        level: "warning",
        code: "server-stderr",
        message: "진단 출력은 native 경계에서 정리되었습니다",
      }],
      droppedEntries: 3,
      droppedStderrBytes: 128,
      stderrTruncated: true,
    }]);

    const rendered = render(<LspControlPanel workspaceRoot="/work" onClose={() => undefined} />);
    expect(await rendered.findByText("검증된 캐시 사용 가능 · native · rust-analyzer.exe")).toBeTruthy();
    expect(rendered.getByText("최근 실패 2회 · 자동 재시도까지 약 3초")).toBeTruthy();
    expect(rendered.getByText("최근 로그 1개")).toBeTruthy();
    expect(rendered.getByText("진단 출력은 native 경계에서 정리되었습니다")).toBeTruthy();
    expect(rendered.getByText("보존 상한으로 정제 로그 3개가 교체되었습니다.")).toBeTruthy();
    expect(rendered.getByText(/native 진단 원본 순환 buffer에서 오래된 128 bytes가 교체/)).toBeTruthy();
    fireEvent.click(rendered.getByRole("button", { name: "다시 시도" }));
    await waitFor(() => expect(restartMock).toHaveBeenCalledTimes(1));
  });

  it("ignores an older status and log refresh after a newer request completes", async () => {
    const oldStatuses = deferred<LanguageServerStatus[]>();
    const oldLogs = deferred<Awaited<ReturnType<typeof languageServerLogs>>>();
    loadMock.mockResolvedValue(loadedConfig({
      config: {
        ...loadedConfig().config,
        enabled: true,
        workspace_root: "/work",
        server_by_language: {
          rust: { kind: "local", installed_path: "/server", args: [] },
        },
      },
    }));
    statusesMock
      .mockImplementationOnce(() => oldStatuses.promise)
      .mockResolvedValue([fixtureServerStatus("rust", "ready")]);
    logsMock
      .mockImplementationOnce(() => oldLogs.promise)
      .mockResolvedValue([{
        languageId: "rust",
        entries: [{ sequence: "2", level: "info", code: "server-ready", message: "new-log" }],
        droppedEntries: 0,
        droppedStderrBytes: 0,
        stderrTruncated: false,
      }]);

    const rendered = render(<LspControlPanel workspaceRoot="/work" onClose={() => undefined} />);
    fireEvent.click(await rendered.findByRole("button", { name: "시작" }));
    expect(await rendered.findByText("준비됨")).toBeTruthy();
    expect(rendered.getByText("new-log")).toBeTruthy();

    await act(async () => {
      oldStatuses.resolve([fixtureServerStatus("rust", "crashed")]);
      oldLogs.resolve([{
        languageId: "rust",
        entries: [{ sequence: "1", level: "error", code: "start-failed", message: "old-log" }],
        droppedEntries: 0,
        droppedStderrBytes: 0,
        stderrTruncated: false,
      }]);
      await Promise.all([oldStatuses.promise, oldLogs.promise]);
    });
    expect(rendered.getByText("준비됨")).toBeTruthy();
    expect(rendered.queryByText("old-log")).toBeNull();
  });
});
