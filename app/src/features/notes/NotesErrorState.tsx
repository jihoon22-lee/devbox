import type { RpcError } from "../../rpc/client";
import { messageFor } from "../../rpc/messages";
import { Button, ErrorState } from "../../ui";

/**
 * A notes panel's failed read: its code's message, next action and diagnostic ID. While notes are
 * off, the daemon answers every notes call with `notes.disabled` until it restarts, so that one
 * offers no retry.
 */
export function NotesErrorState({ error, retry }: { error: RpcError; retry(): void }) {
  const message = messageFor(error.code, error.detail);
  return (
    <ErrorState
      title={message.title}
      diagnosticId={error.diagnosticId}
      action={
        <>
          {message.action && <span>{message.action}</span>}
          {error.code !== "notes.disabled" && <Button onClick={retry}>다시 시도</Button>}
        </>
      }
    />
  );
}
