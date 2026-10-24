/** 인증 편집: 종류마다 초안을 따로 두고 활성 종류만 보낸다(AU-AS-07). 비밀은 `{{secret:NAME}}` 참조를 권한다. */
import s from "./Api.module.css";
import { type AuthDrafts, type AuthKind, type RequestDraft, setAuthKind, updateAuth } from "./model/draft";

const KINDS: [AuthKind, string][] = [
  ["none", "없음"],
  ["basic", "Basic"],
  ["bearer", "Bearer 토큰"],
  ["apiKey", "API 키"],
  ["oauth2ClientCredentials", "OAuth 2.0 클라이언트 자격 증명"],
  ["oauth2AuthCode", "OAuth 2.0 인증 코드"],
];
const LABELS: Record<string, string> = {
  username: "사용자 이름",
  password: "비밀번호",
  token: "토큰",
  name: "키 이름",
  value: "키 값",
  tokenUrl: "토큰 URL",
  authorizationUrl: "인증 URL",
  clientId: "client_id",
  clientSecret: "client_secret",
  scope: "scope",
};

export function AuthEditor({ draft, onChange }: { draft: RequestDraft; onChange: (draft: RequestDraft) => void }) {
  const kind = draft.authKind;
  const current = kind === "none" ? undefined : (draft.auths[kind] as Record<string, string> | undefined);
  return (
    <div className={s.table}>
      <label className={s.bar}>
        인증 방식
        <select
          className={s.select}
          value={kind}
          onChange={(event) => onChange(setAuthKind(draft, event.target.value as AuthKind))}
        >
          {KINDS.map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
      </label>
      {kind !== "none" &&
        current &&
        Object.entries(current).map(([field, value]) =>
          field === "placement" ? (
            <label key={field} className={s.bar}>
              보내는 곳
              <select
                className={s.select}
                value={value}
                onChange={(event) =>
                  onChange(
                    updateAuth(draft, "apiKey", { placement: event.target.value as AuthDrafts["apiKey"]["placement"] }),
                  )
                }
              >
                <option value="header">헤더</option>
                <option value="query">쿼리</option>
              </select>
            </label>
          ) : (
            <label key={field} className={s.bar}>
              <span>{LABELS[field] ?? field}</span>
              <input
                className={`${s.input} ${s.mono} ${s.grow}`}
                value={value}
                placeholder={/secret|password|token|value/i.test(field) ? "{{secret:NAME}}" : undefined}
                onChange={(event) =>
                  onChange(updateAuth(draft, kind as keyof AuthDrafts, { [field]: event.target.value } as never))
                }
              />
            </label>
          ),
        )}
    </div>
  );
}
