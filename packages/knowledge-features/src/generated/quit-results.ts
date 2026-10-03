import type { CollectorStatus } from "./CollectorStatus";

export type QuitResults = {
  lifecycle_status: CollectorStatus;
  pending_quit: string | null;
  decide_quit: null;
};
