import type { DevSetupApplyView } from "./DevSetupApplyView";
import type { DevSetupAuditView } from "./DevSetupAuditView";
import type { DevSetupConfigurationExportView } from "./DevSetupConfigurationExportView";
import type { DevSetupConfigurationReviewView } from "./DevSetupConfigurationReviewView";
import type { DiagnosisItem } from "./DiagnosisItem";
import type { RelatedToolActionView } from "./RelatedToolActionView";
import type { RelatedToolView } from "./RelatedToolView";
import type { SupportBundleExport } from "./SupportBundleExport";
import type { SupportBundlePreview } from "./SupportBundlePreview";
import type { SupportBundleStatus } from "./SupportBundleStatus";

export type ToolsResults = {
  run_diagnosis: Array<DiagnosisItem>;
  preview_support_bundle: SupportBundlePreview;
  cancel_support_bundle: SupportBundleStatus;
  export_support_bundle: SupportBundleExport;
  dev_setup_audit: DevSetupAuditView;
  import_dev_setup_configuration: DevSetupConfigurationReviewView | null;
  discard_dev_setup_configuration: null;
  export_dev_setup_configuration: DevSetupConfigurationExportView;
  apply_dev_setup_configuration: DevSetupApplyView;
  cancel_dev_setup_apply: null;
  related_tools: Array<RelatedToolView>;
  install_related_tool: RelatedToolActionView;
  launch_related_tool: RelatedToolActionView;
  open_related_url: null;
};
