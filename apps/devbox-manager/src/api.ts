import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import catalogJson from "../../catalog.json";
import { isTauri } from "./lib/isTauri";
import type {
  BatchInstallRequest,
  BatchInstallResult,
  CatalogApp,
  Current,
  DataDatabaseInfo,
  DataExport,
  DataInspectorSnapshot,
  DataQueryRequest,
  DataQueryResult,
  InstalledApp,
  InstallPathInfo,
  InstallRootApplyResult,
  InstallRootPreview,
  InstallMode,
  RemoveAppRequest,
  RemovePreview,
  RemoveResult,
  ReleaseManifest,
  SupportBundleExport,
  SupportBundlePreview,
} from "./types";

const MOCK_CATALOG: CatalogApp[] = catalogJson.apps;

export type ManagerOpenRequest = {
  target: { kind: "install"; appId: string };
  from: string | null;
};

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
    { id: "devbox-launcher", version: "0.1.0", portable: { name: "devbox-launcher.exe", sha256: "a".repeat(64), size: 1 }, installer: { name: "devbox-launcher_0.1.0_x64-setup.exe", sha256: "b".repeat(64), size: 2 } },
  ],
};

export async function catalog(): Promise<CatalogApp[]> {
  if (!isTauri()) return MOCK_CATALOG;
  return invoke<CatalogApp[]>("catalog");
}

export async function takePendingOpen(): Promise<ManagerOpenRequest | null> {
  if (!isTauri()) return null;
  return invoke<ManagerOpenRequest | null>("take_pending_open");
}

export function onPendingOpen(handler: (request: ManagerOpenRequest) => void): Promise<UnlistenFn> {
  if (!isTauri()) return Promise.resolve(() => undefined);
  return listen<ManagerOpenRequest>("devbox://open", (event) => handler(event.payload));
}

export async function available(): Promise<ReleaseManifest> {
  if (!isTauri()) return MOCK_MANIFEST;
  return invoke<ReleaseManifest>("available");
}

export async function installed(): Promise<InstalledApp[]> {
  if (!isTauri()) return [{ app: "port-manager", version: "0.2.1", mode: "portable" }];
  return invoke<InstalledApp[]>("installed");
}

export async function installPath(appId: string): Promise<InstallPathInfo> {
  if (!isTauri()) {
    const root = "C:\\Users\\developer\\AppData\\Local\\com.devbox.devboxmanager";
    return {
      appId,
      mode: "portable",
      executable: `${root}\\apps\\${appId}\\versions\\0.2.1\\${appId}.exe`,
      installRoot: root,
      sourceManifest: `${root}\\registry.json`,
    };
  }
  return invoke<InstallPathInfo>("install_path", { appId });
}

export async function previewInstallRoot(path: string): Promise<InstallRootPreview> {
  if (!isTauri()) {
    const candidatePath = path.trim();
    if (!candidatePath) throw new Error("install root input is invalid");
    return {
      status: "ready",
      canApply: true,
      registryRevision: 1,
      catalogRevision: Number(catalogJson.catalogRevision ?? 1),
      candidatePath,
      rootId: "custom-browser-preview",
      freeSpaceBytes: 512 * 1024 * 1024,
      requiredFreeSpaceBytes: 128 * 1024 * 1024,
      activeInstallCount: 0,
      candidateEntryCount: 0,
      migration: "no-automatic-migration",
    };
  }
  return invoke<InstallRootPreview>("preview_install_root", { request: { path } });
}

export async function applyInstallRoot(
  path: string,
  expectedRegistryRevision: number,
): Promise<InstallRootApplyResult> {
  if (!isTauri()) {
    return {
      status: "applied",
      registryRevision: expectedRegistryRevision + 1,
      rootId: "custom-browser-preview",
      candidatePath: path.trim(),
    };
  }
  return invoke<InstallRootApplyResult>("apply_install_root", {
    request: { path, expectedRegistryRevision },
  });
}

export async function installApp(appId: string, mode: InstallMode): Promise<string> {
  if (!isTauri()) return `installed (${mode})`;
  return invoke<string>("install", { appId, mode });
}

export async function installMany(
  requests: BatchInstallRequest[],
): Promise<BatchInstallResult[]> {
  if (!isTauri()) {
    return requests.map((request) => ({
      ...request,
      ok: true,
      message: request.mode === "portable"
        ? "휴대용 앱을 설치했습니다."
        : "설치 프로그램을 실행했습니다. 화면 안내에 따라 설치하세요.",
    }));
  }
  return invoke<BatchInstallResult[]>("install_many", { requests });
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
      { name: "devbox-data", ok: true, detail: "카탈로그 14개 · 데이터 디렉터리 존재 10개" },
      { name: "catalog-ids", ok: true, detail: "모든 identifier가 com.devbox.*" },
      { name: "runtime-metadata", ok: true, detail: "runtime catalog와 install-root locator 정합" },
    ];
  }
  return invoke<DiagnosisItem[]>("run_diagnosis");
}

export async function inspectDataDatabases(operationId: string): Promise<DataInspectorSnapshot> {
  // Browser values are deterministic, sanitized screen-flow fixtures only;
  // native Tauri commands own path discovery and all safety checks.
  if (!isTauri()) {
    const app: DataDatabaseInfo = {
      appId: "everything-plus",
      displayName: "Everything+",
      identifier: "com.devbox.everythingplus",
      state: "available",
      revision: "browser-preview-revision",
      byteLength: 4096,
      schemaVersion: 1,
      tables: [{ name: "files", rowCount: 12 }],
      views: [],
      integrity: "ok",
      warning: null,
    };
    return { catalogRevision: Number(catalogJson.catalogRevision ?? 1), databases: [app] };
  }
  return invoke<DataInspectorSnapshot>("inspect_data_databases", { operationId });
}

export async function previewDataQuery(request: DataQueryRequest): Promise<DataQueryResult> {
  if (!isTauri()) {
    if (!request.appId || !request.sql.trim()) throw new Error("조회 요청이 올바르지 않습니다.");
    return {
      previewId: `browser-query-${request.queryId}`,
      queryId: request.queryId,
      appId: request.appId,
      databaseRevision: "browser-preview-revision",
      columns: ["id", "name", "status"],
      rows: [[1, "browser preview", "ok"]],
      rowCount: 1,
      resultBytes: 42,
      truncated: false,
      elapsedMs: 1,
    };
  }
  return invoke<DataQueryResult>("preview_data_query", { request });
}

export async function cancelDataDiagnostics(operationId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("cancel_data_diagnostics", { request: { operationId } });
}

export async function exportDataPreview(
  previewId: string,
  format: "json" | "csv",
): Promise<DataExport> {
  if (!isTauri()) {
    const content = format === "json"
      ? JSON.stringify({ schemaVersion: 1, redactionVersion: "v1", rows: [[1, "browser preview", "ok"]] }, null, 2)
      : "id,name,status\n1,browser preview,ok\n";
    return {
      filename: `devbox-data-browser.${format}`,
      mimeType: format === "json" ? "application/json" : "text/csv;charset=utf-8",
      format,
      content,
      byteCount: content.length,
    };
  }
  return invoke<DataExport>("export_data_preview", { request: { previewId, format } });
}

export async function previewSupportBundle(operationId: string): Promise<SupportBundlePreview> {
  if (!isTauri()) {
    return {
      previewId: `browser-support-${operationId}`,
      catalogRevision: Number(catalogJson.catalogRevision ?? 1),
      expiresAtMs: Date.now() + 300_000,
      estimatedBytes: 2048,
      databaseCount: 1,
      includedSections: ["app-metadata", "catalog-metadata", "schema-metadata", "log-metadata", "diagnosis"],
      omittedSections: ["raw-database", "raw-logs", "paths", "environment-values", "credentials", "authorization"],
      redactionVersion: "v1",
    };
  }
  return invoke<SupportBundlePreview>("preview_support_bundle", { operationId });
}

export async function cancelSupportBundle(operationId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("cancel_support_bundle", { request: { operationId } });
}

export async function exportSupportBundle(previewId: string): Promise<SupportBundleExport> {
  if (!isTauri()) {
    const content = JSON.stringify({
      schemaVersion: 1,
      redaction: { version: "v1", paths: "omitted", secrets: "omitted", rawLogs: "omitted" },
      omitted: ["raw-database-bytes", "raw-log-lines", "filesystem-paths", "credentials"],
    }, null, 2);
    return {
      filename: "devbox-support-bundle.json",
      mimeType: "application/json",
      content,
      byteCount: content.length,
      redactionVersion: "v1",
    };
  }
  return invoke<SupportBundleExport>("export_support_bundle", { previewId });
}

export async function launchApp(name: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("launch", { appId: name });
}

export async function openInstallFolder(appId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("open_install_folder", { appId });
}

export async function previewRemoveApp(appId: string): Promise<RemovePreview> {
  if (!isTauri()) {
    const root = "C:\\Users\\developer\\AppData\\Local\\com.devbox.devboxmanager";
    return {
      appId,
      mode: "portable",
      version: "0.2.1",
      state: "ready",
      canRemove: true,
      registryRevision: 1,
      catalogRevision: Number(catalogJson.catalogRevision ?? 1),
      rootId: "custom-browser-root",
      manifestDigest: "0".repeat(64),
      targetPath: `${root}\\apps\\${appId}`,
      ownedEntryCount: 3,
      ownedBytes: 1,
      preservesUserData: true,
    };
  }
  return invoke<RemovePreview>("preview_remove_app", { appId });
}

export async function removeApp(request: RemoveAppRequest): Promise<RemoveResult> {
  if (!isTauri()) {
    return {
      status: "removed",
      message: "휴대용 앱의 Manager 소유 파일을 제거했습니다. 앱 사용자 데이터는 유지됩니다.",
      removedEntryCount: 3,
      remainingEntryCount: 0,
      preservesUserData: true,
    };
  }
  return invoke<RemoveResult>("remove_portable_app", { request });
}
