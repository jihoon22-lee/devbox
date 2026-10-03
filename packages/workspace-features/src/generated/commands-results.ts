import type { CloseRequest } from "./CloseRequest";

export type CommandsResults = {
  cancel_close: null;
  confirm_close: null;
  prepare_close: CloseRequest;
};
