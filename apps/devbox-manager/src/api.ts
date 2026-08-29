import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
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
  RelatedTool,
  RelatedToolActionResult,
  ReleaseManifest,
  SupportBundleExport,
  SupportBundlePreview,
} from "./types";

const MOCK_CATALOG: CatalogApp[] = catalogJson.apps;

const MOCK_RELATED_TOOLS: RelatedTool[] = [
  {
    id: "power-toys",
    displayName: "PowerToys",
    summary: "Windows 생산성 유틸리티 모음",
    wingetId: "Microsoft.PowerToys",
    officialUrl: "https://learn.microsoft.com/windows/powertoys/",
    licenseUrl: "https://github.com/microsoft/PowerToys/blob/main/LICENSE",
    license: "MIT (소스)",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
  },
  {
    id: "windows-terminal",
    displayName: "Windows Terminal",
    summary: "탭·프로필을 지원하는 Windows 터미널",
    wingetId: "Microsoft.WindowsTerminal",
    officialUrl: "https://github.com/microsoft/terminal",
    licenseUrl: "https://github.com/microsoft/terminal/blob/main/LICENSE",
    license: "MIT",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
  },
  {
    id: "vs-code",
    displayName: "Visual Studio Code",
    summary: "경량 코드 편집기",
    wingetId: "Microsoft.VisualStudioCode",
    officialUrl: "https://code.visualstudio.com/",
    licenseUrl: "https://code.visualstudio.com/License",
    license: "Microsoft 배포 약관 · 소스 MIT",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
  },
  {
    id: "bruno",
    displayName: "Bruno",
    summary: "오프라인 우선 API 클라이언트",
    wingetId: "Bruno.Bruno",
    officialUrl: "https://www.usebruno.com/",
    licenseUrl: "https://github.com/usebruno/bruno/blob/main/LICENSE.md",
    license: "MIT",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
  },
  {
    id: "dbeaver",
    displayName: "DBeaver Community",
    summary: "관계형 데이터베이스 탐색기",
    wingetId: "DBeaver.DBeaver.Community",
    officialUrl: "https://dbeaver.io/",
    licenseUrl: "https://github.com/dbeaver/dbeaver/blob/devel/LICENSE",
    license: "Apache-2.0",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
  },
  {
    id: "db-browser",
    displayName: "DB Browser for SQLite",
    summary: "SQLite 데이터베이스 브라우저",
    wingetId: "DBBrowserForSQLite.DBBrowserForSQLite",
    officialUrl: "https://sqlitebrowser.org/",
    licenseUrl: "https://github.com/sqlitebrowser/sqlitebrowser/blob/master/LICENSE",
    license: "MPL-2.0",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
  },
  {
    id: "github-desktop",
    displayName: "GitHub Desktop",
    summary: "GitHub 저장소용 데스크톱 클라이언트",
    wingetId: "GitHub.GitHubDesktop",
    officialUrl: "https://desktop.github.com/",
    licenseUrl: "https://github.com/desktop/desktop/blob/development/LICENSE",
    license: "MIT",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
  },
  {
    id: "podman-desktop",
    displayName: "Podman Desktop",
    summary: "컨테이너와 Pod를 관리하는 데스크톱 앱",
    wingetId: "RedHat.Podman-Desktop",
    officialUrl: "https://podman-desktop.io/",
    licenseUrl: "https://github.com/containers/podman-desktop/blob/main/LICENSE",
    license: "Apache-2.0",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
  },
  {
    id: "docker-desktop",
    displayName: "Docker Desktop",
    summary: "Docker 컨테이너 개발 환경",
    wingetId: "Docker.DockerDesktop",
    officialUrl: "https://www.docker.com/products/docker-desktop/",
    licenseUrl: "https://www.docker.com/legal/docker-software-license/",
    license: "Docker Software License",
    platformSupported: false,
    installed: false,
    detection: "unavailable",
  },
];

const RELATED_TOOL_ID_SET = new Set(MOCK_RELATED_TOOLS.map((tool) => tool.id));
const RELATED_TOOL_ACTION_MESSAGES = {
  installed: "WinGet 설치가 완료되었습니다.",
  launched: "관련 도구를 실행했습니다.",
} as const;
const MAX_RELATED_TOOL_URL_LENGTH = 2048;

function isRelatedToolId(value: unknown): value is string {
  return typeof value === "string"
    && value.length <= 64
    && /^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(value)
    && RELATED_TOOL_ID_SET.has(value);
}

function isRelatedDetection(value: unknown): value is RelatedTool["detection"] {
  return value === "path"
    || value === "known-location"
    || value === "not-found"
    || value === "unavailable";
}

/**
 * Native returns a deliberately small DTO. Validate it at the API boundary
 * before React renders anything so a stale/tampered response cannot inject a
 * path, credential, arbitrary URL, or action state into the Manager screen.
 */
function validateRelatedTools(value: unknown): RelatedTool[] {
  if (!Array.isArray(value) || value.length !== MOCK_RELATED_TOOLS.length) {
    throw new Error("관련 도구 감지 응답이 올바르지 않습니다.");
  }
  const seen = new Set<string>();
  const result: RelatedTool[] = [];
  for (const candidate of value) {
    if (!candidate || typeof candidate !== "object") {
      throw new Error("관련 도구 감지 응답이 올바르지 않습니다.");
    }
    const tool = candidate as Partial<RelatedTool>;
    const expected = typeof tool.id === "string"
      ? MOCK_RELATED_TOOLS.find((item) => item.id === tool.id)
      : undefined;
    const detection = tool.detection;
    if (
      !expected
      || seen.has(expected.id)
      || !isRelatedDetection(detection)
      || tool.displayName !== expected.displayName
      || tool.summary !== expected.summary
      || tool.wingetId !== expected.wingetId
      || tool.officialUrl !== expected.officialUrl
      || tool.licenseUrl !== expected.licenseUrl
      || tool.license !== expected.license
      || typeof tool.platformSupported !== "boolean"
      || typeof tool.installed !== "boolean"
      || (!tool.platformSupported && tool.installed)
      || tool.installed !== (detection === "path" || detection === "known-location")
    ) {
      throw new Error("관련 도구 감지 응답이 올바르지 않습니다.");
    }
    seen.add(expected.id);
    result.push({
      ...expected,
      platformSupported: tool.platformSupported,
      installed: tool.installed,
      detection,
    });
  }
  return result;
}

function validateRelatedAction(
  value: unknown,
  toolId: string,
  status: RelatedToolActionResult["status"],
): RelatedToolActionResult {
  if (
    !value
    || typeof value !== "object"
    || (value as Partial<RelatedToolActionResult>).toolId !== toolId
    || (value as Partial<RelatedToolActionResult>).status !== status
  ) {
    throw new Error("관련 도구 작업 결과가 올바르지 않습니다.");
  }
  return {
    toolId,
    status,
    // Never render native-provided message text: a future process error must
    // not turn into a path, account name, credential, or package-manager log.
    message: RELATED_TOOL_ACTION_MESSAGES[status],
  };
}

export type ManagerOpenRequest = {
  target: { kind: "install"; appId: string };
  from: string | null;
};

// The source catalog has 15 apps, but this Manager-facing browser fixture
// contains only the 14 manager-visible, non-self-managed targets.
const MOCK_MANIFEST: ReleaseManifest = {
  schemaVersion: 1,
  releaseTag: "v0.5.0",
  generatedAt: "2026-08-28T23:45:52Z",
  apps: [
    { id: "port-manager", version: "0.3.0", portable: { name: "port-manager.exe", sha256: "7eabde220d9e65d47da53414f41fab7263c2f6decd26f66204854ff69f0e53f4", size: 9231872 }, installer: { name: "port-manager_0.3.0_x64-setup.exe", sha256: "d77c1e23f3790ae0f3e88f1a6680d0b47aee3ef44f6b1dd1a7ba278dc590e261", size: 2153021 } },
    { id: "developer-toolbox", version: "0.3.0", portable: { name: "developer-toolbox.exe", sha256: "5e19f7114bce15ac7d0f5ba2fc03b97e366322ed07424ee6f4cde7507dbbb56f", size: 11363328 }, installer: { name: "developer-toolbox_0.3.0_x64-setup.exe", sha256: "360fcefc0e64c5a2df77ef635838112d164185e29ab711481652113bad8e8e10", size: 2832480 } },
    { id: "wsl-desktop", version: "0.4.0", portable: { name: "wsl-desktop.exe", sha256: "99f546d55717d9be39a8160affa62b7aca27c29abe92b1b41ad456aaef4a67a2", size: 10682368 }, installer: { name: "wsl-desktop_0.4.0_x64-setup.exe", sha256: "d35ae9ac9d8f78f99d15cd0c8d5da84fdb431a768bec92ed50cbda07f55c3db9", size: 2624608 } },
    { id: "api-playground", version: "0.4.0", portable: { name: "api-playground.exe", sha256: "a75b53a4fe05ad9f77cd3e214df81f36ff665a0f2dbff16836d9dcb222128eee", size: 15140864 }, installer: { name: "api-playground_0.4.0_x64-setup.exe", sha256: "7e80dedcc5d4374149f2ce86324515c6b8d56de7918d8997ffb6265726736a3a", size: 4085331 } },
    { id: "everything-plus", version: "0.4.0", portable: { name: "everything-plus.exe", sha256: "993e9f554a6f96b2aa726ea4b4e8d04bb9f35ac1dde30c88e4113be81c2caef7", size: 13282304 }, installer: { name: "everything-plus_0.4.0_x64-setup.exe", sha256: "93e7758bb9b74f7c1fa58467f7e7e5a47b50cf1c2fa449608ac5a5054fbcba8d", size: 3710367 } },
    { id: "knowledge-base", version: "0.4.0", portable: { name: "knowledge-base.exe", sha256: "aee2044943243799808accae468c1abb21c8867b9a42337c3ecf98cab2ef4452", size: 14493184 }, installer: { name: "knowledge-base_0.4.0_x64-setup.exe", sha256: "17ae494014abb6ae74d5cf2c78875def22c2609d4e39c82daca2f02c35cc3023", size: 4702206 } },
    { id: "life-log", version: "0.4.0", portable: { name: "life-log.exe", sha256: "038f2de30e8ffabe31e502d5bec93ec711a4815d03a8972e15d29235fed2c98e", size: 13802496 }, installer: { name: "life-log_0.4.0_x64-setup.exe", sha256: "fa811672306fd9a86dc64dcb943b3120bb24cc065d28fb11a489e3f77b8a28d9", size: 3654124 } },
    { id: "code-pad", version: "0.4.0", portable: { name: "code-pad.exe", sha256: "5fa0e6cd51b1bb0149400cfa0fc8f8efb048106816895a88f55852506244f442", size: 20690432 }, installer: { name: "code-pad_0.4.0_x64-setup.exe", sha256: "aa207a3ac7999306afbec6d48d7b023c1fd768aa850721ede0f13f3d83d6df20", size: 6491410 } },
    { id: "run-manager", version: "0.4.0", portable: { name: "run-manager.exe", sha256: "19de75e10ddfa638a37e665e25f76d89ada10253239ec33a5248c6cbe376c273", size: 15579648 }, installer: { name: "run-manager_0.4.0_x64-setup.exe", sha256: "311b956e165bdb2587430879d3d7c16c6b67dc374f53791b2a3d1a036e6f36fd", size: 4080257 } },
    { id: "workbench", version: "0.2.0", portable: { name: "workbench.exe", sha256: "6af95bdc7119906ae54c61803ab5bc78daf5b0af41b95c3259a328e9b7401f18", size: 10438144 }, installer: { name: "workbench_0.2.0_x64-setup.exe", sha256: "45e79da57745fff468a661f2186a265b54e69082ee67fe7e9943ee4c30d443c8", size: 2490490 } },
    { id: "webhook-lab", version: "0.2.0", portable: { name: "webhook-lab.exe", sha256: "3b2b253557b46255cee4601dba686245b92793eb5f0384804dc0b656193f77ef", size: 9284096 }, installer: { name: "webhook-lab_0.2.0_x64-setup.exe", sha256: "914f67171d68b84e743f9e069af449e8a971e21ebdcb8d3a68648d422574d97e", size: 2211118 } },
    { id: "repo-manager", version: "0.2.0", portable: { name: "repo-manager.exe", sha256: "1e6ed9642f210fe317634fbb90a7ad9066cd3a7bfb65840fb1b56166307833f5", size: 10007040 }, installer: { name: "repo-manager_0.2.0_x64-setup.exe", sha256: "6fd55bddfe360b10f9b0640de106a30541e981541b9eac73378a0f5e481223ee", size: 2325435 } },
    { id: "devbox-launcher", version: "0.1.0", portable: { name: "devbox-launcher.exe", sha256: "547ea737cfd29c2381bc40820d94416ea8f615ffb680c0c248bfbf3bd46484c8", size: 9334784 }, installer: { name: "devbox-launcher_0.1.0_x64-setup.exe", sha256: "674be1d438d0527561058473755ce2d4c48c6f5b31cb93be72864b8fd9a9aa57", size: 2234838 } },
    { id: "log-lens", version: "0.1.0", portable: { name: "log-lens.exe", sha256: "c93a61a5b0d8ce393b4af7e6495595f4f5871865af72926fe60587f998ebfd37", size: 10528768 }, installer: { name: "log-lens_0.1.0_x64-setup.exe", sha256: "c64a02236aad6c246e9a98ba0d56be2a3bdf104e304a0f8ee378b05f0b8f0bd7", size: 2523187 } },
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

export async function relatedTools(): Promise<RelatedTool[]> {
  const result = isTauri()
    ? await invoke<unknown>("related_tools")
    : MOCK_RELATED_TOOLS.map((tool) => ({ ...tool }));
  return validateRelatedTools(result);
}

export async function installRelatedTool(
  toolId: string,
  confirmed: boolean,
): Promise<RelatedToolActionResult> {
  if (!isRelatedToolId(toolId)) throw new Error("관련 도구 식별자가 올바르지 않습니다.");
  if (typeof confirmed !== "boolean") throw new Error("관련 도구 설치 확인값이 올바르지 않습니다.");
  if (!isTauri()) {
    if (!confirmed) throw new Error("관련 도구 설치는 사용자 확인이 필요합니다.");
    return validateRelatedAction({
      toolId,
      status: "installed",
      message: "WinGet 설치가 완료되었습니다.",
    }, toolId, "installed");
  }
  const result = await invoke<unknown>("install_related_tool", {
    request: { toolId, confirmed },
  });
  return validateRelatedAction(result, toolId, "installed");
}

export async function launchRelatedTool(toolId: string): Promise<RelatedToolActionResult> {
  if (!isRelatedToolId(toolId)) throw new Error("관련 도구 식별자가 올바르지 않습니다.");
  if (!isTauri()) {
    return validateRelatedAction({
      toolId,
      status: "launched",
      message: "관련 도구를 실행했습니다.",
    }, toolId, "launched");
  }
  const result = await invoke<unknown>("launch_related_tool", { toolId });
  return validateRelatedAction(result, toolId, "launched");
}

const RELATED_TOOL_OFFICIAL_HOSTS = new Set([
  "learn.microsoft.com",
  "github.com",
  "code.visualstudio.com",
  "www.usebruno.com",
  "dbeaver.io",
  "sqlitebrowser.org",
  "desktop.github.com",
  "podman-desktop.io",
  "www.docker.com",
]);

function isSafeRelatedToolUrl(value: unknown): value is string {
  try {
    if (typeof value !== "string" || value.length > MAX_RELATED_TOOL_URL_LENGTH) return false;
    const url = new URL(value);
    return url.protocol === "https:"
      && !url.username
      && !url.password
      && !url.port
      && url.hostname.length > 0
      && RELATED_TOOL_OFFICIAL_HOSTS.has(url.hostname);
  } catch {
    return false;
  }
}

/** Open only a URL that passed the Related Tools official-host allowlist. */
export async function openRelatedToolUrl(url: string): Promise<void> {
  if (!isSafeRelatedToolUrl(url)) throw new Error("공식 링크가 올바르지 않습니다.");
  if (!isTauri()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  await openUrl(url);
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
