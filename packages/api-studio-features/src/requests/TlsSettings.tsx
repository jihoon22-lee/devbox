import { useEffect, useState } from "react";
import { listGrpcTlsCredentials } from "./grpcApi";
import { isTauri } from "./lib/isTauri";
import type { RequestTls } from "../generated/RequestTls";
import type { GrpcCredentialProjection } from "../generated/GrpcCredentialProjection";
export function TlsSettings({ value, onChange }: { value?: RequestTls | null; onChange: (value: RequestTls) => void }) {
  const [credentials, setCredentials] = useState<GrpcCredentialProjection[]>([]);
  const [revision, setRevision] = useState(0);
  const [loading, setLoading] = useState(false);
  const [failureRevision, setFailureRevision] = useState<number | null>(null);
  const native = isTauri();
  const credentialId = value?.credentialId ?? null;
  const verify = value?.verify !== false;
  useEffect(() => {
    if (!native) return;
    let active = true;
    setLoading(true);
    setFailureRevision(null);
    void listGrpcTlsCredentials()
      .then((rows) => {
        if (active) setCredentials(rows);
      })
      .catch(() => {
        if (active) setFailureRevision(revision);
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [native, revision]);
  return (
    <section aria-label="요청 TLS 설정">
      <label>
        TLS 자격 증명
        <select
          value={credentialId ?? ""}
          disabled={!native || loading}
          onChange={(event) => onChange({ credentialId: event.currentTarget.value || null, verify })}
        >
          <option value="">사용 안 함</option>
          {credentialId && !credentials.some((row) => row.credentialId === credentialId) && (
            <option value={credentialId}>찾을 수 없는 자격 증명</option>
          )}
          {credentials.map((row) => (
            <option key={row.credentialId} value={row.credentialId}>
              {row.label}
            </option>
          ))}
        </select>
      </label>
      <button type="button" disabled={!native || loading} onClick={() => setRevision((current) => current + 1)}>
        목록 새로 고침
      </button>
      <p>
        Protocols의 TLS 자격 증명에서 CA와 클라이언트 인증서(PEM)를 등록하세요. gRPC와 HTTP가 같은 목록을 사용합니다.
      </p>
      <label>
        <input
          type="checkbox"
          checked={verify}
          onChange={(event) => onChange({ credentialId, verify: event.currentTarget.checked })}
        />
        인증서 검증
      </label>
      {!verify && <p role="status">인증서 검증 꺼짐</p>}
      <p>사용자 TLS 설정은 데스크톱의 일반 HTTP 요청에 적용됩니다. SSE·WebSocket에서는 사용할 수 없습니다.</p>
      {!native && <p>브라우저 미리보기에서는 사용자 TLS 설정으로 전송할 수 없습니다.</p>}
      {loading && <p role="status">TLS 자격 증명 목록을 불러오는 중…</p>}
      {failureRevision === revision && (
        <p role="alert">TLS 자격 증명 목록을 불러오지 못했습니다. 기존 선택은 유지됩니다.</p>
      )}
    </section>
  );
}
