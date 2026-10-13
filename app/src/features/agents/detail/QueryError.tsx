import type { RpcError } from "../../../rpc/client";
import { messageFor } from "../../../rpc/messages";
import { Button, ErrorState } from "../../../ui";
export function QueryError({ query }: { query: { error: RpcError | null; refetch: () => Promise<unknown> } }) {
  if (!query.error) return null;
  const message = messageFor(query.error.code, query.error.detail);
  return (
    <ErrorState
      title={message.title}
      diagnosticId={query.error.diagnosticId}
      action={<Button onClick={() => void query.refetch()}>다시 확인</Button>}
    />
  );
}
