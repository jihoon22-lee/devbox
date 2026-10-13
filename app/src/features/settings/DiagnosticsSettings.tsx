import { useRpcQuery } from "../../rpc/query";
import { StatusDot } from "../../ui";
import { QueryResult } from "./QueryResult";
export function DiagnosticsSettings() {
  const doctor = useRpcQuery("system.doctor", {});
  return (
    <div style={{ padding: "var(--space-6)" }}>
      <h3>앱 진단</h3>
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
