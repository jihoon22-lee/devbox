import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import catalogJson from "../../catalog.json";
import App from "./App";
import {
  available,
  catalog,
  current,
  installApp,
  installed,
  launchApp,
  openInstallFolder,
  removeApp,
  rollback,
  runDiagnosis,
} from "./api";
import type { CatalogApp, Current, InstalledApp, ReleaseManifest } from "./types";

vi.mock("./api", () => ({
  available: vi.fn(),
  catalog: vi.fn(),
  current: vi.fn(),
  installApp: vi.fn(),
  installed: vi.fn(),
  launchApp: vi.fn(),
  openInstallFolder: vi.fn(),
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
const installedMock = vi.mocked(installed);
const currentMock = vi.mocked(current);
const installAppMock = vi.mocked(installApp);
const launchAppMock = vi.mocked(launchApp);
const rollbackMock = vi.mocked(rollback);
const openInstallFolderMock = vi.mocked(openInstallFolder);
const removeAppMock = vi.mocked(removeApp);
const runDiagnosisMock = vi.mocked(runDiagnosis);
const confirmMock = vi.fn<(message?: string) => boolean>();

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
  launchAppMock.mockReset().mockResolvedValue(undefined);
  rollbackMock.mockReset().mockResolvedValue("rolled back");
  openInstallFolderMock.mockReset().mockResolvedValue(undefined);
  removeAppMock.mockReset().mockResolvedValue("removed");
  runDiagnosisMock.mockReset().mockResolvedValue([]);
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

    for (const label of ["설치/업데이트", "실행", "이전 버전 롤백", "설치 폴더 열기", "제거"]) {
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
