import type { SetupStarted } from "./SetupStarted";
import type { SetupStatus } from "./SetupStatus";

export type SetupResults = {
  start_empty: SetupStarted;
  status: SetupStatus;
};
