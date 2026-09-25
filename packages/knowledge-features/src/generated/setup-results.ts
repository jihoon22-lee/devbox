import type { StartupStatus } from "./StartupStatus";
import type { VaultJob } from "./VaultJob";
import type { VaultJobStarted } from "./VaultJobStarted";
import type { VaultScheduleView } from "./VaultScheduleView";

export type SetupResults = {
  status: StartupStatus;
  start_empty: StartupStatus;
  continue_existing: StartupStatus;
  vault_change_status: VaultScheduleView;
  schedule_vault_change: VaultScheduleView;
  cancel_vault_change: VaultScheduleView;
  discard_vault_preview: null;
  vault_change_job: VaultJob;
  prepare_vault_change: VaultJobStarted;
  apply_vault_change: VaultJobStarted;
};
