import type { ReactNode } from "react";
import type { RpcError } from "../../rpc/client";
import { useRpcQuery } from "../../rpc/query";
import { Button, ErrorState, StatusDot } from "../../ui";

function QueryResult({
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
export function SettingsPage() {
  const info = useRpcQuery("system.info", {});
  const doctor = useRpcQuery("system.doctor", {});
  return (
    <div style={{ padding: "var(--space-6)" }}>
      <h2>앱 진단</h2>
      <section aria-label="데몬 정보">
        <h3>데몬 정보</h3>
        <QueryResult pending={info.isPending} error={info.error} retry={() => void info.refetch()}>
          {info.data && (
            <p>
              데몬 {info.data.version} · 인스턴스 {info.data.instance} · 데이터 {info.data.dataDir}
            </p>
          )}
        </QueryResult>
      </section>
      <section aria-label="환경 점검">
        <h3>환경 점검</h3>
        <QueryResult pending={doctor.isPending} error={doctor.error} retry={() => void doctor.refetch()}>
          <ul style={{ listStyle: "none", padding: 0 }}>
            {doctor.data?.checks.map((check) => (
              <li
                key={check.id}
                style={{ display: "flex", gap: "var(--space-2)", flexWrap: "wrap", marginBottom: "var(--space-2)" }}
              >
                <StatusDot
                  tone={check.status === "ok" ? "ok" : check.status === "warn" ? "warn" : "danger"}
                  label={check.status === "ok" ? "정상" : check.status === "warn" ? "주의" : "실패"}
                />
                <span>{check.message}</span>
                {check.fix && <code>{check.fix}</code>}
              </li>
            ))}
          </ul>
          {doctor.data?.checks.length === 0 && <p>점검 항목이 없습니다</p>}
        </QueryResult>
      </section>
    </div>
  );
}
