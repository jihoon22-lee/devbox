import { invoke } from "@tauri-apps/api/core";
import catalogJson from "../../catalog.json";
import { isTauri } from "./lib/isTauri";
import type { CatalogApp, Current, InstalledApp, ReleaseManifest } from "./types";

const MOCK_CATALOG: CatalogApp[] = catalogJson.apps;

const MOCK_MANIFEST: ReleaseManifest = {
  schemaVersion: 1,
  releaseTag: "v0.4.0-rc3",
  generatedAt: "2026-08-17T03:00:00Z",
  apps: [
    { id: "port-manager", version: "0.2.1", portable: { name: "port-manager.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "port-manager_0.2.1_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "developer-toolbox", version: "0.2.1", portable: { name: "developer-toolbox.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "developer-toolbox_0.2.1_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "wsl-desktop", version: "0.3.0", portable: { name: "wsl-desktop.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "wsl-desktop_0.3.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "api-playground", version: "0.3.0", portable: { name: "api-playground.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "api-playground_0.3.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "everything-plus", version: "0.3.0", portable: { name: "everything-plus.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "everything-plus_0.3.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "knowledge-base", version: "0.3.0", portable: { name: "knowledge-base.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "knowledge-base_0.3.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "life-log", version: "0.3.0", portable: { name: "life-log.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "life-log_0.3.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "devbox-manager", version: "0.3.0", portable: { name: "devbox-manager.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "devbox-manager_0.3.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "code-pad", version: "0.3.1", portable: { name: "code-pad.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "code-pad_0.3.1_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "run-manager", version: "0.3.1", portable: { name: "run-manager.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "run-manager_0.3.1_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "workbench", version: "0.1.0", portable: { name: "workbench.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "workbench_0.1.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "webhook-lab", version: "0.1.0", portable: { name: "webhook-lab.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "webhook-lab_0.1.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
    { id: "repo-manager", version: "0.1.1", portable: { name: "repo-manager.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "repo-manager_0.1.1_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
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
  if (!isTauri()) return [{ app: "port-manager", version: "0.2.1", mode: "portable" }];
  return invoke<InstalledApp[]>("installed");
}

export async function installApp(appId: string, mode: "portable" | "installer"): Promise<string> {
  if (!isTauri()) return `installed (${mode})`;
  return invoke<string>("install", { appId, mode });
}

export async function current(appId: string): Promise<Current | null> {
  if (!isTauri()) {
    return appId === "port-manager"
      ? { version: "0.2.1", installedAt: 0, previousVersion: "0.2.0" }
      : null;
  }
  return invoke<Current | null>("current", { appId });
}

export async function rollback(appId: string): Promise<string> {
  if (!isTauri()) return `rolled back (${appId})`;
  return invoke<string>("rollback", { appId });
}

export interface DiagnosisItem {
  name: string;
  ok: boolean;
  detail: string;
}

export async function runDiagnosis(): Promise<DiagnosisItem[]> {
  if (!isTauri()) {
    return [
      { name: "wsl", ok: true, detail: "WSL version 2.4.4" },
      { name: "git", ok: true, detail: "git version 2.45.0" },
      { name: "node", ok: true, detail: "v22.22.1" },
      { name: "pnpm", ok: true, detail: "9.0.0" },
      { name: "rustc", ok: true, detail: "rustc 1.97.1" },
      { name: "cargo", ok: true, detail: "cargo 1.97.1" },
      { name: "devbox-data", ok: true, detail: "카탈로그 13개 · 데이터 디렉터리 존재 10개" },
      { name: "catalog-ids", ok: true, detail: "모든 identifier가 com.devbox.*" },
      { name: "runtime-metadata", ok: true, detail: "runtime catalog와 install-root locator 정합" },
    ];
  }
  return invoke<DiagnosisItem[]>("run_diagnosis");
}

export async function launchApp(name: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("launch", { appId: name });
}

export async function openInstallFolder(appId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("open_install_folder", { appId });
}

export async function removeApp(appId: string): Promise<string> {
  if (!isTauri()) return `removed (${appId})`;
  return invoke<string>("remove_portable_app", { appId });
}
