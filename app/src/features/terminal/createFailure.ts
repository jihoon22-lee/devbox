import { RpcError } from "../../rpc/client";
import { messageFor } from "../../rpc/messages";
import { useToasts } from "../../ui";

/** Tells why a terminal could not be created: the daemon's reason when it gave one. */
export function pushCreateFailure(error: unknown) {
  if (error instanceof RpcError) {
    const { title, action } = messageFor(error.code, error.detail);
    useToasts.getState().push({ tone: "danger", title, ...(action ? { body: action } : {}) });
    return;
  }
  useToasts.getState().push({ tone: "danger", title: "터미널을 만들지 못했습니다. 다시 시도하세요" });
}
