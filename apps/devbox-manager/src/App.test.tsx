import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import catalogJson from "../../catalog.json";
import App from "./App";
import {
  available,
  applyInstallRoot,
  catalog,
  current,
  installApp,
  installPath,
  installMany,
  installed,
  launchApp,
  openInstallFolder,
  previewInstallRoot,
  removeApp,
  rollback,
  runDiagnosis,
} from "./api";
import type {
  CatalogApp,
  Current,
  InstalledApp,
  InstallPathInfo,
  InstallRootPreview,
  ReleaseManifest,
} from "./types";

vi.mock("./api", () => ({
  available: vi.fn(),
  applyInstallRoot: vi.fn(),
  catalog: vi.fn(),
  current: vi.fn(),
  installApp: vi.fn(),
  installPath: vi.fn(),
  installMany: vi.fn(),
  installed: vi.fn(),
  launchApp: vi.fn(),
  openInstallFolder: vi.fn(),
  previewInstallRoot: vi.fn(),
  removeApp: vi.fn(),
  rollback: vi.fn(),
  runDiagnosis: vi.fn(),
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
const launchAppMock = vi.mocked(launchApp);
const rollbackMock = vi.mocked(rollback);
const openInstallFolderMock = vi.mocked(openInstallFolder);
const removeAppMock = vi.mocked(removeApp);
const runDiagnosisMock = vi.mocked(runDiagnosis);
const previewInstallRootMock = vi.mocked(previewInstallRoot);
const confirmMock = vi.fn<(message?: string) => boolean>();
const portablePath: InstallPathInfo = {
  appId: "port-manager",
  mode: "portable",
  executable: "C:\\Devbox\\apps\\port-manager\\versions\\0.2.1\\port-manager.exe",
  installRoot: "C:\\Devbox",
  sourceManifest: "C:\\Devbox\\registry.json",
};

function appRow(name: string): HTMLTableRowElement {
  const row = screen.getByText(name).closest("tr");
  if (!(row instanceof HTMLTableRowElement)) throw new Error(`${name} row was not rendered`);
  return row;
}

beforeEach(() => {
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
  launchAppMock.mockReset().mockResolvedValue(undefined);
  rollbackMock.mockReset().mockResolvedValue("rolled back");
  openInstallFolderMock.mockReset().mockResolvedValue(undefined);
  removeAppMock.mockReset().mockResolvedValue("removed");
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
  confirmMock.mockReset().mockReturnValue(false);
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

    expect(screen.getAllByRole("row")).toHaveLength(13);
    expect(screen.getAllByText("Devbox Manager")).toHaveLength(1);
    const target = appRow("Code Pad");
    fireEvent.contextMenu(target, { clientX: 20, clientY: 24 });

    expect(target.getAttribute("aria-current")).toBe("true");
    expect(screen.getByRole("menu", { name: "앱 메뉴" })).toBeTruthy();
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

  it("requires explicit confirmation before removing only the selected portable app", async () => {
    render(<App />);
    await screen.findByText("Port Manager");
    const target = appRow("Port Manager");

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "제거" }));
    expect(confirmMock).toHaveBeenCalledWith(
      "'Port Manager' 휴대용 앱을 제거할까요? Manager가 관리하는 실행 파일과 보존 버전만 삭제하며 앱 사용자 데이터는 유지됩니다.",
    );
    expect(removeAppMock).not.toHaveBeenCalled();

    removeAppMock.mockImplementationOnce(async () => {
      installedMock.mockResolvedValue([]);
      return "휴대용 앱을 제거했습니다. 앱 사용자 데이터는 유지됩니다.";
    });
    confirmMock.mockReturnValueOnce(true);
    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "제거" }));

    await waitFor(() => expect(removeAppMock).toHaveBeenCalledWith("port-manager"));
    await waitFor(() => expect(screen.getByText(/앱 사용자 데이터는 유지됩니다/)).toBeTruthy());
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
