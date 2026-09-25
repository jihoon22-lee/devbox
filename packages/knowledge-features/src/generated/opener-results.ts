import type { NavigationReceipt } from "./NavigationReceipt";
import type { OpenTargetChoice } from "./OpenTargetChoice";
import type { OpenedSource } from "./OpenedSource";

export type OpenerResults = {
  open_file: OpenedSource;
  reveal_file: OpenedSource;
  open_targets: Array<OpenTargetChoice>;
  open_in: NavigationReceipt;
};
