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
