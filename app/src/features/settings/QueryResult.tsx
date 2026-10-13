import type { ReactNode } from "react";
import type { RpcError } from "../../rpc/client";
import { Button, ErrorState } from "../../ui";
export function QueryResult({
  pending,
  error,
  retry,
  children,
}: {
  pending: boolean;
  error: RpcError | null;
  retry: () => void;
  children: ReactNode;
}) {
  if (error)
    return (
      <ErrorState
        title="진단 정보를 불러오지 못했습니다"
        diagnosticId={error.diagnosticId ?? error.code}
        action={<Button onClick={retry}>다시 시도</Button>}
      />
    );
  if (pending) return <p role="status">연결 후 진단 정보를 불러옵니다</p>;
  return children;
}
