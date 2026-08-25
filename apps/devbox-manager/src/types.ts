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
  mode: "portable" | "installer";
}

export interface Current {
  version: string;
  installedAt: number;
  previousVersion: string | null;
}
