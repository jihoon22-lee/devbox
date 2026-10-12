import type { RpcError } from "../../rpc/client";
import { messageFor } from "../../rpc/messages";
import { Button, ErrorState } from "../../ui";
export function ProjectError({ error, retry }: { error: RpcError; retry?: () => void }) {
  const message = messageFor(error.code, error.detail);
  return (
    <ErrorState
      title={message.title}
      diagnosticId={error.diagnosticId}
      action={
        <>
          <span>{message.action}</span>
          {retry && <Button onClick={retry}>다시 시도</Button>}
        </>
      }
    />
  );
}
