export interface CatalogAction {
  actionId: string;
  actionVersion: number;
  label: string;
  target: string;
  payloadKind: string;
}

export interface CatalogApp {
  id: string;
  displayName: string;
  productName: string;
  identifier: string;
  cargoPackage: string;
  appDir: string;
  release: boolean;
  managerVisible: boolean;
  selfManaged: boolean;
  accepts: string[];
  produces: string[];
  actions: CatalogAction[];
}

export interface AssetRef {
  name: string;
  sha256: string;
  size: number;
}

export interface AppManifest {
  id: string;
  version: string;
  portable: AssetRef;
  installer: AssetRef;
}

export interface ReleaseManifest {
  schemaVersion: number;
  releaseTag: string;
  generatedAt: string;
  apps: AppManifest[];
}

export interface InstalledApp {
  app: string;
  version: string;
  mode: InstallMode;
}

export interface InstallPathInfo {
  appId: string;
  mode: InstallMode;
  executable: string | null;
  installRoot: string | null;
  sourceManifest: string;
}

export type InstallRootPreviewStatus =
  | "ready"
  | "already-active"
  | "existing-install"
  | "candidate-conflict"
  | "permission-denied"
  | "insufficient-free-space"
  | "free-space-unavailable";

export interface InstallRootPreview {
  status: InstallRootPreviewStatus;
  canApply: boolean;
  registryRevision: number;
  catalogRevision: number;
  candidatePath: string;
  rootId: string;
  freeSpaceBytes: number | null;
  requiredFreeSpaceBytes: number;
  activeInstallCount: number;
  candidateEntryCount: number;
  migration: "blocked-existing-install" | "no-automatic-migration";
}

export interface InstallRootApplyResult {
  status: "applied" | "already-active";
  registryRevision: number;
  rootId: string;
  candidatePath: string;
}

export type RemovePreviewState = "ready" | "partial" | "missing" | "unsupported-installer";

export interface RemovePreview {
  appId: string;
  mode: InstallMode;
  version: string;
  state: RemovePreviewState;
  canRemove: boolean;
  registryRevision: number;
  catalogRevision: number;
  rootId: string;
  manifestDigest: string;
  targetPath: string | null;
  ownedEntryCount: number;
  ownedBytes: number;
  preservesUserData: boolean;
}

export interface RemoveAppRequest {
  appId: string;
  expectedRegistryRevision: number;
  expectedCatalogRevision: number;
  expectedRootId: string;
  expectedManifestDigest: string;
}

export interface RemoveResult {
  status: "removed" | "partial";
  message: string;
  removedEntryCount: number;
  remainingEntryCount: number;
  preservesUserData: boolean;
}

export type InstallMode = "portable" | "installer";

export interface BatchInstallRequest {
  appId: string;
  mode: InstallMode;
}

export interface BatchInstallResult extends BatchInstallRequest {
  ok: boolean;
  message: string;
}

export interface Current {
  version: string;
  installedAt: number;
  previousVersion: string | null;
}

export type DataDatabaseState = "available" | "missing" | "unsafe-path" | "unreadable";
export type DataIntegrity = "ok" | "failed" | "timed-out" | "unavailable";

export interface DataSchemaObject {
  name: string;
  rowCount: number | null;
}

export interface DataDatabaseInfo {
  appId: string;
  displayName: string;
  identifier: string;
  state: DataDatabaseState;
  revision: string | null;
  byteLength: number | null;
  schemaVersion: number | null;
  tables: DataSchemaObject[];
  views: DataSchemaObject[];
  integrity: DataIntegrity;
  warning: string | null;
}

export interface DataInspectorSnapshot {
  catalogRevision: number | null;
  databases: DataDatabaseInfo[];
}

export interface DataQueryRequest {
  appId: string;
  sql: string;
  queryId: string;
  expectedRevision: string | null;
}

export type DataCell = string | number | boolean | null;

export interface DataQueryResult {
  previewId: string;
  queryId: string;
  appId: string;
  databaseRevision: string;
  columns: string[];
  rows: DataCell[][];
  rowCount: number;
  resultBytes: number;
  truncated: boolean;
  elapsedMs: number;
}

export interface DataExport {
  filename: string;
  mimeType: string;
  format: "json" | "csv";
  content: string;
  byteCount: number;
}

export interface SupportBundlePreview {
  previewId: string;
  catalogRevision: number | null;
  expiresAtMs: number;
  estimatedBytes: number;
  databaseCount: number;
  includedSections: string[];
  omittedSections: string[];
  redactionVersion: string;
}

export interface SupportBundleExport {
  filename: string;
  mimeType: string;
  content: string;
  byteCount: number;
  redactionVersion: string;
}
