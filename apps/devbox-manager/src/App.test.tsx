import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import catalogJson from "../../catalog.json";
import App from "./App";
import {
  available,
  applyInstallRoot,
  cancelDataDiagnostics,
  cancelSupportBundle,
  catalog,
  current,
  exportDataPreview,
  exportSupportBundle,
  inspectDataDatabases,
  installApp,
  installPath,
  installMany,
  installed,
  installRelatedTool,
  launchApp,
  onPendingOpen,
  launchRelatedTool,
  openRelatedToolUrl,
  openInstallFolder,
  previewDataQuery,
  previewRemoveApp,
  previewSupportBundle,
  previewInstallRoot,
  relatedTools,
  removeApp,
  rollback,
  runDiagnosis,
  takePendingOpen,
} from "./api";
import type {
  CatalogApp,
  Current,
  DataInspectorSnapshot,
  DataQueryResult,
  InstalledApp,
  InstallPathInfo,
  InstallRootPreview,
  RemovePreview,
  RemoveResult,
  RelatedTool,
  RelatedToolActionResult,
  ReleaseManifest,
  SupportBundlePreview,
} from "./types";

vi.mock("./api", () => ({
  available: vi.fn(),
  applyInstallRoot: vi.fn(),
  cancelDataDiagnostics: vi.fn(),
  cancelSupportBundle: vi.fn(),
  catalog: vi.fn(),
  current: vi.fn(),
  exportDataPreview: vi.fn(),
  exportSupportBundle: vi.fn(),
  inspectDataDatabases: vi.fn(),
  installApp: vi.fn(),
  installPath: vi.fn(),
  installMany: vi.fn(),
  installed: vi.fn(),
  installRelatedTool: vi.fn(),
  launchApp: vi.fn(),
  launchRelatedTool: vi.fn(),
  openRelatedToolUrl: vi.fn(),
  onPendingOpen: vi.fn(async () => () => undefined),
  openInstallFolder: vi.fn(),
  previewDataQuery: vi.fn(),
  previewRemoveApp: vi.fn(),
  previewSupportBundle: vi.fn(),
  previewInstallRoot: vi.fn(),
  relatedTools: vi.fn(),
  removeApp: vi.fn(),
  rollback: vi.fn(),
  runDiagnosis: vi.fn(),
  takePendingOpen: vi.fn(async () => null),
}));

const catalogApps = catalogJson.apps as CatalogApp[];
const manifest: ReleaseManifest = {
  schemaVersion: 1,
  releaseTag: "v0.5.0-test",
  generatedAt: "2026-08-26T00:00:00Z",
  apps: [
    {
      id: "port-manager",
      version: "0.2.2",
      portable: { name: "port-manager.exe", sha256: "a".repeat(64), size: 1 },
      installer: { name: "port-manager-setup.exe", sha256: "b".repeat(64), size: 2 },
    },
    {
      id: "code-pad",
      version: "0.3.2",
      portable: { name: "code-pad.exe", sha256: "c".repeat(64), size: 3 },
      installer: { name: "code-pad-setup.exe", sha256: "d".repeat(64), size: 4 },
    },
  ],
};
const portable: InstalledApp = {
  app: "port-manager",
  version: "0.2.1",
  mode: "portable",
};
const portableCurrent: Current = {
  version: "0.2.1",
  installedAt: 1_000,
  previousVersion: "0.2.0",
};

const catalogMock = vi.mocked(catalog);
const availableMock = vi.mocked(available);
const applyInstallRootMock = vi.mocked(applyInstallRoot);
const installedMock = vi.mocked(installed);
const currentMock = vi.mocked(current);
const installAppMock = vi.mocked(installApp);
const installPathMock = vi.mocked(installPath);
const installManyMock = vi.mocked(installMany);
const installRelatedToolMock = vi.mocked(installRelatedTool);
const launchAppMock = vi.mocked(launchApp);
const launchRelatedToolMock = vi.mocked(launchRelatedTool);
const openRelatedToolUrlMock = vi.mocked(openRelatedToolUrl);
const rollbackMock = vi.mocked(rollback);
const openInstallFolderMock = vi.mocked(openInstallFolder);
const previewRemoveAppMock = vi.mocked(previewRemoveApp);
const removeAppMock = vi.mocked(removeApp);
const runDiagnosisMock = vi.mocked(runDiagnosis);
const previewInstallRootMock = vi.mocked(previewInstallRoot);
const cancelDataDiagnosticsMock = vi.mocked(cancelDataDiagnostics);
const cancelSupportBundleMock = vi.mocked(cancelSupportBundle);
const exportDataPreviewMock = vi.mocked(exportDataPreview);
const exportSupportBundleMock = vi.mocked(exportSupportBundle);
const inspectDataDatabasesMock = vi.mocked(inspectDataDatabases);
const previewDataQueryMock = vi.mocked(previewDataQuery);
const previewSupportBundleMock = vi.mocked(previewSupportBundle);
const relatedToolsMock = vi.mocked(relatedTools);
const onPendingOpenMock = vi.mocked(onPendingOpen);
const takePendingOpenMock = vi.mocked(takePendingOpen);
const confirmMock = vi.fn<(message?: string) => boolean>();
const portablePath: InstallPathInfo = {
  appId: "port-manager",
  mode: "portable",
  executable: "C:\\Devbox\\apps\\port-manager\\versions\\0.2.1\\port-manager.exe",
  installRoot: "C:\\Devbox",
  sourceManifest: "C:\\Devbox\\registry.json",
};
const relatedTool: RelatedTool = {
  id: "vs-code",
  displayName: "Visual Studio Code",
  summary: "경량 코드 편집기",
  wingetId: "Microsoft.VisualStudioCode",
  officialUrl: "https://code.visualstudio.com/",
  licenseUrl: "https://code.visualstudio.com/License",
  license: "Microsoft 배포 약관 · 소스 MIT",
  installed: false,
  detection: "not-found",
};

const inspectorSnapshot: DataInspectorSnapshot = {
  catalogRevision: 5,
  databases: [
    {
      appId: "everything-plus",
      displayName: "Everything+",
      identifier: "com.devbox.everythingplus",
      state: "available",
      revision: "database-revision-1",
      byteLength: 4096,
      schemaVersion: 1,
      tables: [{ name: "files", rowCount: 12 }],
      views: [],
      integrity: "ok",
      warning: null,
    },
    {
      appId: "life-log",
      displayName: "Life Log",
      identifier: "com.devbox.lifelog",
      state: "missing",
      revision: null,
      byteLength: null,
      schemaVersion: null,
      tables: [],
      views: [],
      integrity: "unavailable",
      warning: "데이터베이스가 없습니다.",
    },
  ],
};

const inspectorResult: DataQueryResult = {
  previewId: "query-preview-1",
  queryId: "query-1",
  appId: "everything-plus",
  databaseRevision: "database-revision-1",
  columns: ["id", "name", "status"],
  rows: [[1, "[REDACTED]", "ok"]],
  rowCount: 1,
  resultBytes: 48,
  truncated: false,
  elapsedMs: 3,
};

const supportPreviewFixture: SupportBundlePreview = {
  previewId: "support-preview-1",
  catalogRevision: 5,
  expiresAtMs: Date.now() + 300_000,
  estimatedBytes: 2048,
  databaseCount: 1,
  includedSections: ["app-metadata", "catalog-metadata", "schema-metadata", "log-metadata", "diagnosis"],
  omittedSections: ["raw-database", "raw-logs", "paths", "environment-values", "credentials", "authorization"],
  redactionVersion: "v1",
};

function appRow(name: string): HTMLTableRowElement {
  const row = screen.getByText(name).closest("tr");
  if (!(row instanceof HTMLTableRowElement)) throw new Error(`${name} row was not rendered`);
  return row;
}

beforeEach(() => {
  onPendingOpenMock.mockReset().mockResolvedValue(() => undefined);
  takePendingOpenMock.mockReset().mockResolvedValue(null);
  catalogMock.mockReset().mockResolvedValue(catalogApps);
  availableMock.mockReset().mockResolvedValue(manifest);
  installedMock.mockReset().mockResolvedValue([portable]);
  currentMock.mockReset().mockImplementation(async (appId) => (
    appId === portable.app ? portableCurrent : null
  ));
  installAppMock.mockReset().mockResolvedValue("installed");
  installPathMock.mockReset().mockResolvedValue(portablePath);
  installManyMock.mockReset().mockImplementation(async (requests) => requests.map((request) => ({
    ...request,
    ok: true,
    message: "installed",
  })));
  installRelatedToolMock.mockReset().mockResolvedValue({
    toolId: relatedTool.id,
    status: "installed",
    message: "WinGet 설치가 완료되었습니다.",
  });
  launchAppMock.mockReset().mockResolvedValue(undefined);
  launchRelatedToolMock.mockReset().mockResolvedValue({
    toolId: relatedTool.id,
    status: "launched",
    message: "관련 도구를 실행했습니다.",
  });
  openRelatedToolUrlMock.mockReset().mockResolvedValue(undefined);
  rollbackMock.mockReset().mockResolvedValue("rolled back");
  openInstallFolderMock.mockReset().mockResolvedValue(undefined);
  previewRemoveAppMock.mockReset().mockResolvedValue({
    appId: "port-manager",
    mode: "portable",
    version: "0.2.1",
    state: "ready",
    canRemove: true,
    registryRevision: 7,
    catalogRevision: 5,
    rootId: "default-root",
    manifestDigest: "a".repeat(64),
    targetPath: "C:\\Devbox\\apps\\port-manager",
    ownedEntryCount: 3,
    ownedBytes: 1,
    preservesUserData: true,
  } satisfies RemovePreview);
  removeAppMock.mockReset().mockResolvedValue({
    status: "removed",
    message: "휴대용 앱의 Manager 소유 파일을 제거했습니다. 앱 사용자 데이터는 유지됩니다.",
    removedEntryCount: 3,
    remainingEntryCount: 0,
    preservesUserData: true,
  } satisfies RemoveResult);
  runDiagnosisMock.mockReset().mockResolvedValue([]);
  previewInstallRootMock.mockReset().mockResolvedValue({
    status: "ready",
    canApply: true,
    registryRevision: 1,
    catalogRevision: 5,
    candidatePath: "C:\\Devbox-custom",
    rootId: "custom-test-root",
    freeSpaceBytes: 512 * 1024 * 1024,
    requiredFreeSpaceBytes: 128 * 1024 * 1024,
    activeInstallCount: 0,
    candidateEntryCount: 0,
    migration: "no-automatic-migration",
  });
  applyInstallRootMock.mockReset().mockResolvedValue({
    status: "applied",
    registryRevision: 2,
    rootId: "custom-test-root",
    candidatePath: "C:\\Devbox-custom",
  });
  cancelDataDiagnosticsMock.mockReset().mockResolvedValue(undefined);
  cancelSupportBundleMock.mockReset().mockResolvedValue(undefined);
  exportDataPreviewMock.mockReset().mockResolvedValue({
    filename: "devbox-data-preview.json",
    mimeType: "application/json",
    format: "json",
    content: "{\"redactionVersion\":\"v1\"}",
    byteCount: 26,
  });
  exportSupportBundleMock.mockReset().mockResolvedValue({
    filename: "devbox-support-bundle.json",
    mimeType: "application/json",
    content: "{\"redactionVersion\":\"v1\"}",
    byteCount: 26,
    redactionVersion: "v1",
  });
  inspectDataDatabasesMock.mockReset().mockResolvedValue(inspectorSnapshot);
  previewDataQueryMock.mockReset().mockResolvedValue(inspectorResult);
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

describe("Devbox Manager app row context menu", () => {
  it("renders only catalog-managed targets and selects the right-clicked row", async () => {
    render(<App />);
    await screen.findByText("Code Pad");

    expect(screen.getAllByRole("row")).toHaveLength(15);
    expect(screen.getAllByText("Devbox Manager")).toHaveLength(1);
    const target = appRow("Code Pad");
    fireEvent.contextMenu(target, { clientX: 20, clientY: 24 });

    expect(target.getAttribute("aria-current")).toBe("true");
    expect(screen.getByRole("menu", { name: "앱 메뉴" })).toBeTruthy();
  });

  it("selects the catalog-validated install target from Launcher", async () => {
    takePendingOpenMock.mockResolvedValueOnce({
      target: { kind: "install", appId: "devbox-launcher" },
      from: "devbox-launcher",
    });
    render(<App />);

    const target = await screen.findByText("Devbox Launcher");
    const row = target.closest("tr");
    if (!(row instanceof HTMLTableRowElement)) throw new Error("Launcher row was not rendered");
    await waitFor(() => expect(row.getAttribute("aria-current")).toBe("true"));
    expect(screen.getByText("Launcher 요청: 선택한 앱의 설치 방법을 고르세요.")).toBeTruthy();
  });

  it("shows the exact app actions with portable state gates", async () => {
    render(<App />);
    await screen.findByText("Port Manager");

    fireEvent.contextMenu(appRow("Port Manager"));

    for (const label of [
      "설치/업데이트",
      "실행",
      "이전 버전 롤백",
      "설치 폴더 열기",
      "설치 경로 정보",
      "제거",
    ]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeTruthy();
    }
    expect(screen.getByRole("menuitem", { name: "실행" }).getAttribute("aria-disabled")).toBeNull();
    expect(screen.getByRole("menuitem", { name: "이전 버전 롤백" }).getAttribute("aria-disabled")).toBeNull();
    expect(screen.getByRole("menuitem", { name: "제거" }).className).toContain("danger");
  });

  it("opens the install submenu from Shift+F10 and restores row focus", async () => {
    render(<App />);
    await screen.findByText("Code Pad");
    const target = appRow("Code Pad");
    target.focus();

    fireEvent.keyDown(target, { key: "F10", code: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "설치/업데이트" }));
    expect(screen.getByRole("menu", { name: "설치/업데이트" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "설치 패키지" })).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "휴대용" }));

    await waitFor(() => expect(installAppMock).toHaveBeenCalledWith("code-pad", "portable"));
    await waitFor(() => expect(document.activeElement).toBe(target));
  });

  it("routes the setup choice opened with the Menu key to the exact app", async () => {
    render(<App />);
    await screen.findByText("Code Pad");
    const target = appRow("Code Pad");
    target.focus();

    fireEvent.keyDown(target, { key: "ContextMenu", code: "ContextMenu" });
    fireEvent.click(screen.getByRole("menuitem", { name: "설치/업데이트" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "설치 패키지" }));

    await waitFor(() => expect(installAppMock).toHaveBeenCalledWith("code-pad", "installer"));
    await waitFor(() => expect(document.activeElement).toBe(target));
  });

  it("routes launch, rollback, and folder actions to the exact catalog row", async () => {
    render(<App />);
    await screen.findByText("Port Manager");
    const target = appRow("Port Manager");

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "실행" }));
    await waitFor(() => expect(launchAppMock).toHaveBeenCalledWith("port-manager"));

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "이전 버전 롤백" }));
    await waitFor(() => expect(rollbackMock).toHaveBeenCalledWith("port-manager"));

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "설치 폴더 열기" }));
    await waitFor(() => expect(openInstallFolderMock).toHaveBeenCalledWith("port-manager"));
  });

  it("previews the exact portable target before a separate confirmation", async () => {
    render(<App />);
    await screen.findByText("Port Manager");
    const target = appRow("Port Manager");

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "제거" }));
    await waitFor(() => expect(previewRemoveAppMock).toHaveBeenCalledWith("port-manager"));
    expect(screen.getByRole("region", { name: "제거 대상 미리 보기" })).toBeTruthy();
    expect(screen.getByText("C:\\Devbox\\apps\\port-manager")).toBeTruthy();
    expect(screen.getByText(/앱 사용자 데이터/)).toBeTruthy();
    expect(confirmMock).not.toHaveBeenCalled();
    expect(removeAppMock).not.toHaveBeenCalled();

    removeAppMock.mockImplementationOnce(async () => {
      installedMock.mockResolvedValue([]);
      return {
        status: "removed",
        message: "휴대용 앱의 Manager 소유 파일을 제거했습니다. 앱 사용자 데이터는 유지됩니다.",
        removedEntryCount: 3,
        remainingEntryCount: 0,
        preservesUserData: true,
      };
    });
    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(screen.getByRole("button", { name: "확인 후 제거" }));

    await waitFor(() => expect(removeAppMock).toHaveBeenCalledWith({
      appId: "port-manager",
      expectedRegistryRevision: 7,
      expectedCatalogRevision: 5,
      expectedRootId: "default-root",
      expectedManifestDigest: "a".repeat(64),
    }));
    expect(confirmMock).toHaveBeenCalledWith(
      "'Port Manager'의 Manager 소유 portable 파일을 제거할까요? 앱 사용자 데이터는 유지됩니다.",
    );
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain(
      "앱 사용자 데이터는 유지됩니다",
    ));
  });

  it("does not mutate while the removal preview is pending", async () => {
    previewRemoveAppMock.mockImplementationOnce(() => new Promise(() => {}));
    render(<App />);
    await screen.findByText("Port Manager");
    fireEvent.contextMenu(appRow("Port Manager"));
    fireEvent.click(screen.getByRole("menuitem", { name: "제거" }));
    await waitFor(() => expect(previewRemoveAppMock).toHaveBeenCalledTimes(1));
    expect((screen.getByRole("button", { name: "Refresh" }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect(removeAppMock).not.toHaveBeenCalled();
    fireEvent.contextMenu(appRow("Port Manager"));
    expect(previewRemoveAppMock).toHaveBeenCalledTimes(1);
  });

  it("clears a stale preview after the confirmed removal is rejected", async () => {
    removeAppMock.mockRejectedValueOnce(new Error("stale manifest"));
    confirmMock.mockReturnValueOnce(true);
    render(<App />);
    await screen.findByText("Port Manager");

    fireEvent.contextMenu(appRow("Port Manager"));
    fireEvent.click(screen.getByRole("menuitem", { name: "제거" }));
    await screen.findByText("C:\\Devbox\\apps\\port-manager");
    fireEvent.click(screen.getByRole("button", { name: "확인 후 제거" }));

    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain(
      "설치 상태가 바뀌었습니다. 최신 제거 미리 보기를 다시 확인하세요.",
    ));
    expect(screen.queryByText("C:\\Devbox\\apps\\port-manager")).toBeNull();
    expect(screen.queryByRole("button", { name: "확인 후 제거" })).toBeNull();
  });

  it("keeps installer lifecycle, folder, and removal actions fail-closed", async () => {
    installedMock.mockResolvedValue([
      { app: "port-manager", version: "0.2.1", mode: "installer" },
    ]);
    currentMock.mockResolvedValue(null);
    render(<App />);
    await screen.findByText("Port Manager");

    fireEvent.contextMenu(appRow("Port Manager"));

    for (const label of ["실행", "이전 버전 롤백", "설치 폴더 열기", "제거"]) {
      expect(screen.getByRole("menuitem", { name: label }).getAttribute("aria-disabled")).toBe("true");
    }
    expect(screen.getByRole("menuitem", { name: "설치 경로 정보" }).getAttribute("aria-disabled"))
      .toBeNull();
  });

  it("disables install/update when the catalog target is already current", async () => {
    availableMock.mockResolvedValue({
      ...manifest,
      apps: manifest.apps.map((app) => (
        app.id === "port-manager" ? { ...app, version: portable.version } : app
      )),
    });
    render(<App />);
    await screen.findByText("Port Manager");

    fireEvent.contextMenu(appRow("Port Manager"));

    expect(screen.getByRole("menuitem", { name: "설치/업데이트" }).getAttribute("aria-disabled"))
      .toBe("true");
  });
});

describe("Devbox Manager install path details", () => {
  it("shows only backend-verified portable executable, root, and source manifest", async () => {
    render(<App />);
    await screen.findByText("Port Manager");

    fireEvent.click(screen.getByRole("button", { name: "Port Manager 설치 경로 정보" }));

    await waitFor(() => expect(installPathMock).toHaveBeenCalledWith("port-manager"));
    expect(screen.getByRole("region", { name: "검증된 설치 경로 정보" })).toBeTruthy();
    expect(screen.getByText(portablePath.executable!)).toBeTruthy();
    expect(screen.getByText(portablePath.installRoot!)).toBeTruthy();
    expect(screen.getByText(portablePath.sourceManifest)).toBeTruthy();
    expect(screen.getByText("읽기 전용")).toBeTruthy();
    expect(installAppMock).not.toHaveBeenCalled();
    expect(openInstallFolderMock).not.toHaveBeenCalled();
    expect(removeAppMock).not.toHaveBeenCalled();
  });

  it("does not guess executable or root for installer records", async () => {
    installedMock.mockResolvedValue([
      { app: "port-manager", version: "0.2.1", mode: "installer" },
    ]);
    currentMock.mockResolvedValue(null);
    installPathMock.mockResolvedValue({
      appId: "port-manager",
      mode: "installer",
      executable: null,
      installRoot: null,
      sourceManifest: "C:\\Devbox\\registry.json",
    });
    render(<App />);
    await screen.findByText("Port Manager");

    fireEvent.click(screen.getByRole("button", { name: "Port Manager 설치 경로 정보" }));

    await waitFor(() => expect(installPathMock).toHaveBeenCalledWith("port-manager"));
    expect(screen.getAllByText("Manager가 실제 설치 위치를 추적하지 않습니다.")).toHaveLength(2);
    expect(screen.getByText(/설치 패키지는 마법사 실행 뒤의 실제 위치/)).toBeTruthy();
  });
});

describe("Devbox Manager custom install root", () => {
  it("requires an explicit preview and confirmation before applying a root", async () => {
    render(<App />);
    await screen.findByText("Port Manager");

    const input = screen.getByLabelText("설치 root 경로");
    fireEvent.change(input, { target: { value: "C:\\Devbox-custom" } });
    fireEvent.click(screen.getByRole("button", { name: "미리 확인" }));

    await waitFor(() => expect(previewInstallRootMock).toHaveBeenCalledWith("C:\\Devbox-custom"));
    expect(screen.getByRole("status")).toBeTruthy();
    expect(applyInstallRootMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(screen.getByRole("button", { name: "확인 후 이 root 적용" }));
    await waitFor(() => expect(applyInstallRootMock).toHaveBeenCalledWith("C:\\Devbox-custom", 1));
    expect(confirmMock).toHaveBeenCalledWith(
      "검증된 빈 디렉터리를 새 설치 root로 적용할까요? 기존 설치는 자동으로 이동하거나 삭제하지 않습니다.",
    );
  });

  it("invalidates a preview when the IME-safe input changes and blocks duplicate preview calls", async () => {
    render(<App />);
    await screen.findByText("Port Manager");

    const input = screen.getByLabelText("설치 root 경로");
    fireEvent.change(input, { target: { value: "C:\\Devbox-custom" } });
    fireEvent.click(screen.getByRole("button", { name: "미리 확인" }));
    await waitFor(() => expect(screen.getByRole("status")).toBeTruthy());

    fireEvent.change(input, { target: { value: "C:\\Devbox-other" } });
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByRole("button", { name: "확인 후 이 root 적용" })).toBeNull();
  });

  it("disables other Manager operations while a root preflight is pending", async () => {
    previewInstallRootMock.mockImplementationOnce(() => new Promise(() => {}));
    render(<App />);
    await screen.findByText("Port Manager");

    fireEvent.change(screen.getByLabelText("설치 root 경로"), {
      target: { value: "C:\\Devbox-pending" },
    });
    fireEvent.click(screen.getByRole("button", { name: "미리 확인" }));

    await waitFor(() => expect(previewInstallRootMock).toHaveBeenCalledTimes(1));
    expect((screen.getByRole("button", { name: "Refresh" }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect((screen.getByRole("button", { name: "환경 진단" }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect((screen.getByRole("checkbox", {
      name: "설치 및 업데이트 가능한 앱 전체 선택",
    }) as HTMLInputElement).disabled).toBe(true);
  });

  it("blocks root and app mutations while a metadata refresh is pending", async () => {
    render(<App />);
    await screen.findByText("Port Manager");
    availableMock.mockImplementationOnce(() => new Promise(() => {}));

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    await waitFor(() => expect(availableMock).toHaveBeenCalledTimes(2));
    expect((screen.getByLabelText("설치 root 경로") as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "미리 확인" }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect((screen.getByRole("button", { name: "환경 진단" }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect((screen.getByRole("button", { name: "Launch" }) as HTMLButtonElement).disabled)
      .toBe(true);
  });

  it("does not resurrect a stale preview after an in-flight input change", async () => {
    let resolvePreview!: (preview: InstallRootPreview) => void;
    previewInstallRootMock.mockImplementationOnce(() => new Promise((resolve) => {
      resolvePreview = resolve;
    }));
    render(<App />);
    await screen.findByText("Port Manager");

    const input = screen.getByLabelText("설치 root 경로");
    fireEvent.change(input, { target: { value: "C:\\Devbox-old" } });
    fireEvent.click(screen.getByRole("button", { name: "미리 확인" }));
    fireEvent.change(input, { target: { value: "C:\\Devbox-new" } });
    resolvePreview({
      status: "ready",
      canApply: true,
      registryRevision: 1,
      catalogRevision: 5,
      candidatePath: "C:\\Devbox-old",
      rootId: "stale-root",
      freeSpaceBytes: 512 * 1024 * 1024,
      requiredFreeSpaceBytes: 128 * 1024 * 1024,
      activeInstallCount: 0,
      candidateEntryCount: 0,
      migration: "no-automatic-migration",
    });

    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
    expect(screen.getByDisplayValue("C:\\Devbox-new")).toBeTruthy();
  });

  it("ignores a preview response that arrives after the component unmounts", async () => {
    let resolvePreview!: (preview: InstallRootPreview) => void;
    previewInstallRootMock.mockImplementationOnce(() => new Promise((resolve) => {
      resolvePreview = resolve;
    }));
    const view = render(<App />);
    await screen.findByText("Port Manager");

    const input = screen.getByLabelText("설치 root 경로");
    fireEvent.change(input, { target: { value: "C:\\Devbox-unmounted" } });
    fireEvent.click(screen.getByRole("button", { name: "미리 확인" }));
    view.unmount();
    resolvePreview({
      status: "ready",
      canApply: true,
      registryRevision: 1,
      catalogRevision: 5,
      candidatePath: "C:\\Devbox-unmounted",
      rootId: "unmounted-root",
      freeSpaceBytes: 512 * 1024 * 1024,
      requiredFreeSpaceBytes: 128 * 1024 * 1024,
      activeInstallCount: 0,
      candidateEntryCount: 0,
      migration: "no-automatic-migration",
    });
    await Promise.resolve();

    expect(document.querySelector(".install-root-preview")).toBeNull();
  });

  it("reports an existing-install boundary without offering migration or removal", async () => {
    previewInstallRootMock.mockResolvedValueOnce({
      status: "existing-install",
      canApply: false,
      registryRevision: 3,
      catalogRevision: 5,
      candidatePath: "C:\\Devbox-custom",
      rootId: "custom-test-root",
      freeSpaceBytes: 512 * 1024 * 1024,
      requiredFreeSpaceBytes: 128 * 1024 * 1024,
      activeInstallCount: 1,
      candidateEntryCount: 0,
      migration: "blocked-existing-install",
    });
    render(<App />);
    await screen.findByText("Port Manager");
    fireEvent.change(screen.getByLabelText("설치 root 경로"), {
      target: { value: "C:\\Devbox-custom" },
    });
    fireEvent.click(screen.getByRole("button", { name: "미리 확인" }));

    expect(await screen.findByText("기존 설치로 이동 차단")).toBeTruthy();
    expect(screen.getByText(/자동 이동하지 않습니다/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "확인 후 이 root 적용" })).toBeNull();
    expect(applyInstallRootMock).not.toHaveBeenCalled();
  });
});

describe("Devbox Manager diagnostics and support bundle", () => {
  it("keeps Data Inspector read-only and requires an explicit preview before export", async () => {
    render(<App />);
    await screen.findByText("Code Pad");
    fireEvent.click(screen.getByRole("button", { name: "환경 진단" }));
    await screen.findByRole("heading", { name: "Data Inspector" });

    expect(screen.queryByRole("button", { name: "JSON export" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "데이터 다시 확인" }));
    await screen.findByText("Everything+");
    expect(inspectDataDatabasesMock).toHaveBeenCalledWith(expect.any(String));
    expect(screen.queryByText(/C:\\Users|AppData/)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "미리 보기" }));
    await screen.findByText("조회 결과 preview");
    expect(previewDataQueryMock).toHaveBeenCalledWith(expect.objectContaining({
      appId: "everything-plus",
      sql: "SELECT name, type FROM sqlite_schema",
      expectedRevision: "database-revision-1",
    }));
    expect(exportDataPreviewMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "JSON export" }));
    await waitFor(() => expect(exportDataPreviewMock).toHaveBeenCalledWith("query-preview-1", "json"));
    expect(screen.getByRole("status").textContent).toContain("JSON 파일을 준비했습니다.");
  });

  it("offers cancellation while a native database inspection is pending", async () => {
    inspectDataDatabasesMock.mockImplementationOnce(() => new Promise(() => {}));
    render(<App />);
    await screen.findByText("Code Pad");
    fireEvent.click(screen.getByRole("button", { name: "환경 진단" }));
    await screen.findByRole("heading", { name: "Data Inspector" });
    fireEvent.click(screen.getByRole("button", { name: "데이터 다시 확인" }));

    await waitFor(() => expect(inspectDataDatabasesMock).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    expect(cancelDataDiagnosticsMock).toHaveBeenCalledWith(expect.any(String));
  });

  it("shows support bundle inclusion and omission boundaries before one-time export", async () => {
    render(<App />);
    await screen.findByText("Code Pad");
    fireEvent.click(screen.getByRole("button", { name: "환경 진단" }));
    await screen.findByRole("heading", { name: "Redacted support bundle" });

    fireEvent.click(screen.getByRole("button", { name: "번들 미리 확인" }));
    await screen.findByText(/내보내기 preview · redaction v1/);
    expect(previewSupportBundleMock).toHaveBeenCalledWith(expect.any(String));
    expect(exportSupportBundleMock).not.toHaveBeenCalled();
    expect(screen.getByText("raw-database")).toBeTruthy();
    expect(screen.getByText("credentials")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "확인 후 JSON export" }));
    await waitFor(() => expect(exportSupportBundleMock).toHaveBeenCalledWith("support-preview-1"));
    expect(screen.getByRole("status").textContent).toContain("redacted 지원 번들을 준비했습니다.");
  });

  it("clears a consumed support preview when native export reports a stale revision", async () => {
    exportSupportBundleMock.mockRejectedValueOnce(new Error("지원 번들이 오래되었습니다."));
    render(<App />);
    await screen.findByText("Code Pad");
    fireEvent.click(screen.getByRole("button", { name: "환경 진단" }));
    await screen.findByRole("heading", { name: "Redacted support bundle" });
    fireEvent.click(screen.getByRole("button", { name: "번들 미리 확인" }));
    await screen.findByText(/내보내기 preview · redaction v1/);

    fireEvent.click(screen.getByRole("button", { name: "확인 후 JSON export" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("지원 번들이 오래되었습니다."));
    expect(screen.queryByText(/내보내기 preview · redaction v1/)).toBeNull();
    expect(screen.getByRole("button", { name: "번들 미리 확인" })).toBeTruthy();
  });
});

describe("Devbox Manager batch install", () => {
  it("continues after a partial failure and retries only the failed app", async () => {
    installManyMock.mockResolvedValueOnce([
      {
        appId: "port-manager",
        mode: "portable",
        ok: false,
        message: "설치/업데이트에 실패했습니다. 앱 상태를 확인한 뒤 이 항목만 다시 시도하세요.",
      },
      {
        appId: "code-pad",
        mode: "portable",
        ok: true,
        message: "휴대용 앱을 설치했습니다.",
      },
    ]);
    render(<App />);
    await screen.findByText("Code Pad");

    fireEvent.click(screen.getByRole("checkbox", { name: "Port Manager 일괄 선택" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Code Pad 일괄 선택" }));
    fireEvent.click(screen.getByRole("button", { name: "휴대용 일괄 실행" }));

    await waitFor(() => expect(installManyMock).toHaveBeenCalledWith([
      { appId: "port-manager", mode: "portable" },
      { appId: "code-pad", mode: "portable" },
    ]));
    expect(await screen.findByText("일괄 작업 완료: 성공 1개, 실패 1개")).toBeTruthy();
    expect((screen.getByRole("checkbox", {
      name: "Port Manager 일괄 선택",
    }) as HTMLInputElement).checked).toBe(true);
    expect((screen.getByRole("checkbox", {
      name: "Code Pad 일괄 선택",
    }) as HTMLInputElement).checked).toBe(false);
    expect(installAppMock).not.toHaveBeenCalled();

    installManyMock.mockResolvedValueOnce([{
      appId: "port-manager",
      mode: "portable",
      ok: true,
      message: "휴대용 앱을 설치했습니다.",
    }]);
    const retry = screen.getByRole("button", { name: "실패 항목만 재시도 (1)" });
    await waitFor(() => expect((retry as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(retry);

    await waitFor(() => expect(installManyMock).toHaveBeenNthCalledWith(2, [
      { appId: "port-manager", mode: "portable" },
    ]));
    expect(await screen.findByText("일괄 작업 완료: 성공 1개, 실패 0개")).toBeTruthy();
  });

  it("confirms a setup batch before launching one installer per app", async () => {
    render(<App />);
    await screen.findByText("Code Pad");
    fireEvent.click(screen.getByRole("checkbox", { name: "Code Pad 일괄 선택" }));

    fireEvent.click(screen.getByRole("button", { name: "설치 패키지 일괄 실행" }));
    expect(confirmMock).toHaveBeenCalledWith(
      "1개 앱의 설치 마법사를 각각 실행할까요? 각 창에서 설치를 완료해야 합니다.",
    );
    expect(installManyMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(screen.getByRole("button", { name: "설치 패키지 일괄 실행" }));
    await waitFor(() => expect(installManyMock).toHaveBeenCalledWith([
      { appId: "code-pad", mode: "installer" },
    ]));
  });
});

describe("Devbox Manager Related Tools", () => {
  it("loads the bounded curated metadata and official links", async () => {
    render(<App />);
    await screen.findByText("Port Manager");
    fireEvent.click(screen.getByRole("button", { name: "관련 도구" }));

    expect(await screen.findByText("Visual Studio Code")).toBeTruthy();
    expect(screen.getByText("표준 감지 위치에서 찾지 못했습니다.")).toBeTruthy();
    expect(screen.getByRole("link", { name: "공식 사이트" }).getAttribute("href"))
      .toBe("https://code.visualstudio.com/");
    expect(screen.getByRole("link", { name: "라이선스" }).getAttribute("href"))
      .toBe("https://code.visualstudio.com/License");
    expect(screen.getByText("Microsoft.VisualStudioCode")).toBeTruthy();
    expect(screen.getByRole("button", { name: "확인 후 WinGet 설치" })).toBeTruthy();
    fireEvent.click(screen.getByRole("link", { name: "공식 사이트" }));
    await waitFor(() => expect(openRelatedToolUrlMock).toHaveBeenCalledWith("https://code.visualstudio.com/"));
  });

  it("does not render non-HTTPS links returned outside the curated contract", async () => {
    relatedToolsMock.mockResolvedValueOnce([{
      ...relatedTool,
      officialUrl: "https://evil.example/tool.exe",
      licenseUrl: "javascript:alert(1)",
    }]);
    render(<App />);
    await screen.findByText("Port Manager");
    fireEvent.click(screen.getByRole("button", { name: "관련 도구" }));
    await screen.findByText("Visual Studio Code");

    expect(screen.queryByRole("link", { name: "공식 사이트" })).toBeNull();
    expect(screen.queryByRole("link", { name: "라이선스" })).toBeNull();
  });

  it("requires confirmation before invoking WinGet install", async () => {
    render(<App />);
    await screen.findByText("Port Manager");
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
    relatedToolsMock.mockResolvedValueOnce([{
      ...relatedTool,
      installed: true,
      detection: "path",
    }]);
    render(<App />);
    await screen.findByText("Port Manager");
    fireEvent.click(screen.getByRole("button", { name: "관련 도구" }));

    expect(await screen.findByRole("button", { name: "실행" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "확인 후 WinGet 설치" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "실행" }));
    await waitFor(() => expect(launchRelatedToolMock).toHaveBeenCalledWith("vs-code"));
  });

  it("does not render raw native errors from a related-tool action", async () => {
    installRelatedToolMock.mockRejectedValueOnce(
      new Error("C:\\Users\\developer\\secret-token=should-not-render"),
    );
    confirmMock.mockReturnValueOnce(true);
    render(<App />);
    await screen.findByText("Port Manager");
    fireEvent.click(screen.getByRole("button", { name: "관련 도구" }));
    await screen.findByText("Visual Studio Code");

    fireEvent.click(screen.getByRole("button", { name: "확인 후 WinGet 설치" }));

    await waitFor(() => expect(screen.getByText("관련 도구 작업을 완료할 수 없습니다.")).toBeTruthy());
    expect(screen.queryByText(/secret-token/)).toBeNull();
    expect(screen.queryByText(/C:\\Users\\developer/)).toBeNull();
  });

  it("ignores a related-tool action result after unmount", async () => {
    let resolveInstall!: (result: RelatedToolActionResult) => void;
    installRelatedToolMock.mockImplementationOnce(() => new Promise((resolve) => {
      resolveInstall = resolve;
    }));
    confirmMock.mockReturnValueOnce(true);
    const view = render(<App />);
    await screen.findByText("Port Manager");
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
