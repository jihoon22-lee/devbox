/** 실시간 연결 대상: URL·헤더(행 표)·TLS(검증·CA 경로). 변수·비밀 참조는 편집기 요청과 같이 쓴다. */

import s from "../Api.module.css";
import { ParamTable } from "../ParamTable";
import type { Target } from "./messages";

export function TargetFields({
  target,
  placeholder,
  onChange,
}: {
  target: Target;
  placeholder: string;
  onChange: (target: Target) => void;
}) {
  return (
    <>
      <input
        className={`${s.input} ${s.mono} ${s.grow}`}
        aria-label="URL"
        placeholder={placeholder}
        value={target.url}
        onChange={(event) => onChange({ ...target, url: event.target.value })}
      />
      <details>
        <summary>헤더·TLS</summary>
        <ParamTable label="헤더" rows={target.headers} onChange={(headers) => onChange({ ...target, headers })} />
        <div className={s.bar}>
          <label className={s.bar}>
            <input
              type="checkbox"
              checked={target.tls.verify}
              onChange={(event) => onChange({ ...target, tls: { ...target.tls, verify: event.target.checked } })}
            />
            인증서 검증
          </label>
          <input
            className={`${s.input} ${s.grow}`}
            aria-label="CA 인증서 경로"
            placeholder="/home/…/ca.pem"
            value={target.tls.caPath ?? ""}
            onChange={(event) =>
              onChange({ ...target, tls: { ...target.tls, caPath: event.target.value.trim() || null } })
            }
          />
        </div>
      </details>
    </>
  );
}
