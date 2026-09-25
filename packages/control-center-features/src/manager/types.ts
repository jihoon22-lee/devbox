export type SupportBundlePreview = import("../generated/SupportBundlePreview").SupportBundlePreview;

export type SupportBundleExport = import("../generated/SupportBundleExport").SupportBundleExport;

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

export type RelatedTool = import("../generated/RelatedToolView").RelatedToolView;

export type RelatedToolActionResult = import("../generated/RelatedToolActionView").RelatedToolActionView;

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

export type DevSetupAudit = Omit<
  import("../generated/DevSetupAuditView").DevSetupAuditView,
  "capabilities" | "plan"
> & { capabilities: DevSetupCapability[]; plan: DevSetupPlanItem[] };

export type DevSetupConfigurationDesired = "present" | "latest" | "version";
export type DevSetupConfigurationCurrentState = "present" | "absent" | "update-available" | "unknown";
export type DevSetupConfigurationAction = "none" | "install" | "update" | "reconcile-version" | "verify";

export type DevSetupConfigurationPackageReview =
  import("../generated/DevSetupPackageReviewView").DevSetupPackageReviewView;

export type DevSetupConfigurationReview =
  import("../generated/DevSetupConfigurationReviewView").DevSetupConfigurationReviewView;

export type DevSetupConfigurationExport =
  import("../generated/DevSetupConfigurationExportView").DevSetupConfigurationExportView;

export type DevSetupConfigurationApplyStatus = "complete" | "partial" | "cancelled";
export type DevSetupConfigurationPackageApplyStatus =
  | "unchanged"
  | "applied"
  | "failed"
  | "timed-out"
  | "cancelled"
  | "skipped";

export type DevSetupConfigurationPackageApplyResult =
  import("../generated/DevSetupPackageApplyView").DevSetupPackageApplyView;

export type DevSetupConfigurationApplyResult = import("../generated/DevSetupApplyView").DevSetupApplyView;

// Native names use `*View`; these aliases keep the frontend vocabulary
// consistent with the other Manager API DTOs while preserving that contract.
export type DevSetupConfigurationReviewView = DevSetupConfigurationReview;
export type DevSetupConfigurationExportView = DevSetupConfigurationExport;
export type DevSetupConfigurationApplyView = DevSetupConfigurationApplyResult;
export type DevSetupPackageReviewView = DevSetupConfigurationPackageReview;
export type DevSetupPackageApplyView = DevSetupConfigurationPackageApplyResult;
