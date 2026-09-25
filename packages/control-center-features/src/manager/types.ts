export interface SupportBundlePreview {
  previewId: string;
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

export type RelatedToolDetection = "path" | "known-location" | "not-found" | "unavailable";
export type InstallCapabilityState = "present" | "absent" | "unknown";
export type AvailabilityCapabilityState = "available" | "unavailable" | "unknown";
export type WslBackendCapabilityState = "running" | "stopped" | "present" | "absent" | "unknown";

export interface CapabilityEvidence {
  source: string;
  result: string;
}

export interface DockerCapability {
  desktopInstall: InstallCapabilityState;
  desktopLaunch: AvailabilityCapabilityState;
  windowsCli: AvailabilityCapabilityState;
  wslBackend: WslBackendCapabilityState;
  evidence: CapabilityEvidence[];
  observedAtMs: number;
}

export interface RelatedTool {
  id: string;
  displayName: string;
  summary: string;
  wingetId: string;
  officialUrl: string;
  licenseUrl: string;
  license: string;
  platformSupported: boolean;
  installed: boolean;
  detection: RelatedToolDetection;
  installState: InstallCapabilityState;
  launchState: AvailabilityCapabilityState;
  dockerCapability: DockerCapability | null;
}

export interface RelatedToolActionResult {
  toolId: string;
  status: "installed" | "launched";
  message: string;
}

export type DevSetupCapabilityId =
  | "docker-desktop-install"
  | "docker-desktop-launch"
  | "docker-windows-cli"
  | "docker-wsl-backend"
  | "winget";

export interface DevSetupCapability {
  id: DevSetupCapabilityId;
  scope: "windows" | "wsl";
  state: string;
  evidence: CapabilityEvidence[];
}

export interface DevSetupPlanItem {
  capabilityId: DevSetupCapabilityId;
  status: "satisfied" | "review" | "unknown";
  action:
    | "none"
    | "review-install"
    | "verify-installation"
    | "review-launch-path"
    | "review-cli"
    | "start-backend"
    | "review-backend"
    | "review-winget";
}

export interface DevSetupAudit {
  schemaVersion: 1;
  observedAtMs: number;
  mode: "read-only";
  capabilities: DevSetupCapability[];
  plan: DevSetupPlanItem[];
}

export type DevSetupConfigurationDesired = "present" | "latest" | "version";
export type DevSetupConfigurationCurrentState =
  | "present"
  | "absent"
  | "update-available"
  | "unknown";
export type DevSetupConfigurationAction =
  | "none"
  | "install"
  | "update"
  | "reconcile-version"
  | "verify";

export interface DevSetupConfigurationPackageReview {
  packageId: string;
  desired: DevSetupConfigurationDesired;
  version: string | null;
  currentState: DevSetupConfigurationCurrentState;
  action: DevSetupConfigurationAction;
  requestedAgreementAcceptance: boolean;
  declaredElevation: boolean;
}

export interface DevSetupConfigurationReview {
  schemaVersion: "0.3";
  previewId: string;
  expiresAtMs: number;
  configurationDigest: string;
  sourceTrust: "external-restricted";
  mode: "package-only";
  canApply: boolean;
  hasChanges: boolean;
  requiresAgreementConfirmation: boolean;
  mayRequireAdmin: boolean;
  mayRequireReboot: boolean;
  packages: DevSetupConfigurationPackageReview[];
}

export interface DevSetupConfigurationExport {
  filename: "devbox-packages.winget";
  mimeType: "application/yaml;charset=utf-8";
  content: string;
  byteCount: number;
  sha256: string;
}

export type DevSetupConfigurationApplyStatus = "complete" | "partial" | "cancelled";
export type DevSetupConfigurationPackageApplyStatus =
  | "unchanged"
  | "applied"
  | "failed"
  | "timed-out"
  | "cancelled"
  | "skipped";

export interface DevSetupConfigurationPackageApplyResult {
  packageId: string;
  status: DevSetupConfigurationPackageApplyStatus;
}

export interface DevSetupConfigurationApplyResult {
  status: DevSetupConfigurationApplyStatus;
  observedAtMs: number;
  results: DevSetupConfigurationPackageApplyResult[];
}

// Native names use `*View`; these aliases keep the frontend vocabulary
// consistent with the other Manager API DTOs while preserving that contract.
export type DevSetupConfigurationReviewView = DevSetupConfigurationReview;
export type DevSetupConfigurationExportView = DevSetupConfigurationExport;
export type DevSetupConfigurationApplyView = DevSetupConfigurationApplyResult;
export type DevSetupPackageReviewView = DevSetupConfigurationPackageReview;
export type DevSetupPackageApplyView = DevSetupConfigurationPackageApplyResult;
