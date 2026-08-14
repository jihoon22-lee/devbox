import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { CatalogApp, Current, InstalledApp, ReleaseManifest } from "./types";

const MOCK_CATALOG: CatalogApp[] = [
  { id: "port-manager", displayName: "Port Manager", productName: "PortManager", identifier: "com.devbox.portmanager", cargoPackage: "port-manager", appDir: "apps/port-manager", release: true, managerVisible: true, selfManaged: false },
  { id: "developer-toolbox", displayName: "Developer Toolbox", productName: "DevToolbox", identifier: "com.devbox.developertoolbox", cargoPackage: "developer-toolbox", appDir: "apps/developer-toolbox", release: true, managerVisible: true, selfManaged: false },
  { id: "wsl-desktop", displayName: "WSL Desktop", productName: "WSLDesktop", identifier: "com.devbox.wsldesktop", cargoPackage: "wsl-desktop", appDir: "apps/wsl-desktop", release: true, managerVisible: true, selfManaged: false },
  { id: "api-playground", displayName: "API Playground", productName: "ApiPlayground", identifier: "com.devbox.apiplayground", cargoPackage: "api-playground", appDir: "apps/api-playground", release: true, managerVisible: true, selfManaged: false },
  { id: "everything-plus", displayName: "Everything+", productName: "EverythingPlus", identifier: "com.devbox.everythingplus", cargoPackage: "everything-plus", appDir: "apps/everything-plus", release: true, managerVisible: true, selfManaged: false },
  { id: "knowledge-base", displayName: "Knowledge", productName: "Knowledge", identifier: "com.devbox.knowledgebase", cargoPackage: "knowledge-base", appDir: "apps/knowledge-base", release: true, managerVisible: true, selfManaged: false },
  { id: "life-log", displayName: "Life Log", productName: "LifeLog", identifier: "com.devbox.lifelog", cargoPackage: "life-log", appDir: "apps/life-log", release: true, managerVisible: true, selfManaged: false },
  { id: "devbox-manager", displayName: "Devbox Manager", productName: "DevboxManager", identifier: "com.devbox.devboxmanager", cargoPackage: "devbox-manager", appDir: "apps/devbox-manager", release: true, managerVisible: false, selfManaged: true },
  { id: "code-pad", displayName: "Code Pad", productName: "Code Pad", identifier: "com.devbox.codepad", cargoPackage: "code-pad", appDir: "apps/code-pad", release: true, managerVisible: true, selfManaged: false },
  { id: "run-manager", displayName: "Run Manager", productName: "Run Manager", identifier: "com.devbox.runmanager", cargoPackage: "run-manager", appDir: "apps/run-manager", release: true, managerVisible: true, selfManaged: false },
];

const MOCK_MANIFEST: ReleaseManifest = {
  schemaVersion: 1,
  releaseTag: "v0.4.0",
  generatedAt: "2026-08-14T12:00:00Z",
  apps: [
    { id: "port-manager", version: "0.2.0", portable: { name: "port-manager.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "PortManager_0.2.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "developer-toolbox", version: "0.2.0", portable: { name: "developer-toolbox.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "DevToolbox_0.2.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "wsl-desktop", version: "0.2.2", portable: { name: "wsl-desktop.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "WSLDesktop_0.2.2_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "api-playground", version: "0.2.0", portable: { name: "api-playground.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "ApiPlayground_0.2.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "everything-plus", version: "0.2.0", portable: { name: "everything-plus.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "EverythingPlus_0.2.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "knowledge-base", version: "0.2.0", portable: { name: "knowledge-base.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "Knowledge_0.2.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "life-log", version: "0.2.2", portable: { name: "life-log.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "LifeLog_0.2.2_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "devbox-manager", version: "0.2.0", portable: { name: "devbox-manager.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "DevboxManager_0.2.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "code-pad", version: "0.3.0", portable: { name: "code-pad.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "Code Pad_0.3.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "run-manager", version: "0.3.0", portable: { name: "run-manager.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "Run Manager_0.3.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
  ],
};

export async function catalog(): Promise<CatalogApp[]> {
  if (!isTauri()) return MOCK_CATALOG;
  return invoke<CatalogApp[]>("catalog");
}

export async function available(): Promise<ReleaseManifest> {
  if (!isTauri()) return MOCK_MANIFEST;
  return invoke<ReleaseManifest>("available");
}

export async function installed(): Promise<InstalledApp[]> {
  if (!isTauri()) return [{ app: "port-manager", version: "0.2.0", mode: "portable", exe_path: "" }];
  return invoke<InstalledApp[]>("installed");
}

export async function installApp(appId: string, mode: "portable" | "installer"): Promise<string> {
  if (!isTauri()) return `installed (${mode})`;
  return invoke<string>("install", { appId, mode });
}

export async function current(appId: string): Promise<Current | null> {
  if (!isTauri()) {
    return appId === "port-manager"
      ? { version: "0.2.0", exePath: "", installedAt: 0, previousVersion: "0.1.0" }
      : null;
  }
  return invoke<Current | null>("current", { appId });
}

export async function rollback(appId: string): Promise<string> {
  if (!isTauri()) return `rolled back (${appId})`;
  return invoke<string>("rollback", { appId });
}

export async function launchApp(name: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("launch", { name });
}
