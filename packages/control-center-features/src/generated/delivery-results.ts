import type { DeliveryAccepted } from "./DeliveryAccepted";
import type { InstallationOpened } from "./InstallationOpened";
import type { RecordedHealth } from "./RecordedHealth";
import type { RecoveryStatus } from "./RecoveryStatus";
import type { RestoreInventory } from "./RestoreInventory";
import type { SuiteInventory } from "./SuiteInventory";
import type { UpdateReview } from "./UpdateReview";

export type DeliveryResults = {
  suite_inventory: SuiteInventory;
  open_installation_folder: InstallationOpened;
  suite_recovery: RecoveryStatus;
  restore_inventory: RestoreInventory;
  restore_action: DeliveryAccepted;
  record_suite_health: RecordedHealth;
  check_suite_update: UpdateReview;
  suite_update_status: UpdateReview;
  download_suite_update: UpdateReview;
  cancel_suite_update: UpdateReview;
  launch_suite_update: DeliveryAccepted;
};
