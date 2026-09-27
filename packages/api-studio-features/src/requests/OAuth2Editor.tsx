import { usePolling } from "@devbox/hooks";
import { useCallback, useEffect, useId, useRef, useState } from "react";
import type { AuthConfig } from "./types";
import type { OAuth2Config } from "../generated/OAuth2Config";
import type { EnvVariable } from "./lib/environments";
import { emptyOAuth2 } from "./lib/oauth2";
import { isTauri } from "./lib/isTauri";
import { apiMessages } from "../issues/catalog";
import {
  oauth2Status,
  authorizeOAuth2,
  cancelOAuth2,
  fetchOAuth2Token,
  clearOAuth2Token,
  type OAuth2TokenStatus,
} from "./api";
import "./OAuth2Editor.css";
interface Props {
  auth: AuthConfig;
  environment: EnvVariable[];
  onChange: (config: OAuth2Config) => void;
  statusKey?: number;
  loginRequest?: number;
  onLoginHandled?: () => void;
}
const missing: OAuth2TokenStatus = { state: "missing", expiresAtMs: null, scope: null };
function errorText(cause: unknown): string {
  const code = cause instanceof Error ? cause.name : typeof cause === "string" ? cause : "";
  return code.startsWith("oauth2_") && Object.prototype.hasOwnProperty.call(apiMessages, code)
    ? apiMessages[code as keyof typeof apiMessages]
    : "OAuth 2.0 작업을 마치지 못했습니다. 설정을 확인하고 다시 시도하세요.";
}
export function OAuth2Editor({ auth, environment, onChange, statusKey = 0, loginRequest = 0, onLoginHandled }: Props) {
  const config = auth.oauth2 ?? emptyOAuth2,
    native = isTauri(),
    help = useId();
  const [status, setStatus] = useState<OAuth2TokenStatus>(missing);
  const [busy, setBusy] = useState<"authorize" | "fetch" | "clear" | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const [error, setError] = useState("");
  const epoch = useRef(0),
    active = useRef<string | null>(null),
    handled = useRef(0),
    working = useRef(false);
  useEffect(() => {
    const token = ++epoch.current;
    working.current = false;
    setStatus(missing);
    setError("");
    setBusy(null);
    setCancelling(false);
    // A response may expire a cached token without changing the edited profile.
    void statusKey;
    if (native && auth.oauth2?.tokenUrl && auth.oauth2.clientId) {
      void oauth2Status(auth, environment).then(
        (value) => {
          if (epoch.current === token) setStatus(value);
        },
        (cause) => {
          if (epoch.current === token) setError(errorText(cause));
        },
      );
    }
    return () => {
      epoch.current++;
      working.current = false;
      const requestId = active.current;
      active.current = null;
      if (requestId) void cancelOAuth2(requestId).catch(() => undefined);
    };
  }, [auth, environment, native, statusKey]);
  usePolling(
    async () => {
      const token = epoch.current;
      try {
        const value = await oauth2Status(auth, environment);
        if (epoch.current === token && !working.current) setStatus(value);
      } catch (cause) {
        if (epoch.current === token && !working.current) setError(errorText(cause));
      }
    },
    {
      intervalMs: 30000,
      immediate: false,
      active: native && busy === null && Boolean(auth.oauth2?.tokenUrl && auth.oauth2.clientId),
    },
  );
  const run = useCallback(
    async (action: "authorize" | "fetch" | "clear") => {
      if (!native || working.current) return;
      working.current = true;
      const token = ++epoch.current;
      const requestId = action === "authorize" ? `oauth2-${globalThis.crypto.randomUUID()}` : null;
      active.current = requestId;
      setBusy(action);
      setError("");
      setCancelling(false);
      try {
        const result =
          action === "authorize"
            ? await authorizeOAuth2(requestId!, auth, environment)
            : action === "fetch"
              ? await fetchOAuth2Token(auth, environment)
              : (await clearOAuth2Token(auth, environment), missing);
        if (epoch.current === token) setStatus(result);
      } catch (cause) {
        if (epoch.current === token) setError(errorText(cause));
      } finally {
        if (epoch.current === token) {
          working.current = false;
          active.current = null;
          setBusy(null);
          setCancelling(false);
        }
      }
    },
    [auth, environment, native],
  );
  useEffect(() => {
    if (!loginRequest || handled.current === loginRequest) return;
    const timer = setTimeout(() => {
      handled.current = loginRequest;
      onLoginHandled?.();
      void run("authorize");
    }, 0);
    return () => clearTimeout(timer);
  }, [loginRequest, onLoginHandled, run]);
  const cancel = async () => {
    const requestId = active.current,
      token = epoch.current;
    if (!requestId) return;
    setCancelling(true);
    try {
      await cancelOAuth2(requestId);
    } catch (cause) {
      if (epoch.current === token) {
        setError(errorText(cause));
        setCancelling(false);
      }
    }
  };
  const label =
    status.state === "missing"
      ? "토큰 없음"
      : status.state === "expired"
        ? "만료됨"
        : status.expiresAtMs === null
          ? "유효 · 만료 시각 없음"
          : `유효 · ${Math.max(0, Math.ceil((status.expiresAtMs - Date.now()) / 60000))}분 뒤 만료`;
  const update = (patch: Partial<OAuth2Config>) => onChange({ ...config, ...patch });
  return (
    <section className="oauth2-editor" aria-label="OAuth 2.0">
      <fieldset disabled={busy !== null}>
        <legend>OAuth 2.0 설정</legend>
        <label>
          방식
          <select
            value={config.grantType}
            onChange={(event) => update({ grantType: event.currentTarget.value as OAuth2Config["grantType"] })}
          >
            <option value="authorizationCode">Authorization Code (PKCE)</option>
            <option value="clientCredentials">Client Credentials</option>
          </select>
        </label>
        {config.grantType === "authorizationCode" && (
          <label>
            Authorization URL
            <input
              value={config.authorizationUrl}
              onChange={(event) => update({ authorizationUrl: event.currentTarget.value })}
            />
          </label>
        )}
        <label>
          Token URL
          <input value={config.tokenUrl} onChange={(event) => update({ tokenUrl: event.currentTarget.value })} />
        </label>
        <label>
          Client ID
          <input value={config.clientId} onChange={(event) => update({ clientId: event.currentTarget.value })} />
        </label>
        <label>
          Client secret
          <input
            type="password"
            aria-describedby={help}
            value={config.clientSecret}
            onChange={(event) => update({ clientSecret: event.currentTarget.value })}
          />
        </label>
        <p id={help}>{"환경 변수의 비밀 값({{이름}})을 쓰세요."}</p>
        <label>
          Scopes
          <input value={config.scopes} onChange={(event) => update({ scopes: event.currentTarget.value })} />
        </label>
      </fieldset>
      <p role="status">{label}</p>
      {busy === "authorize" && <p>브라우저에서 로그인을 마쳐 주세요.</p>}
      {!native && <p>OAuth 2.0 인증은 데스크톱 앱에서 사용할 수 있습니다.</p>}
      <div className="oauth2-actions">
        {config.grantType === "authorizationCode" ? (
          <button type="button" disabled={!native || busy !== null} onClick={() => void run("authorize")}>
            로그인
          </button>
        ) : (
          <button type="button" disabled={!native || busy !== null} onClick={() => void run("fetch")}>
            토큰 받기
          </button>
        )}
        {busy === "authorize" && (
          <button type="button" disabled={cancelling} onClick={() => void cancel()}>
            {cancelling ? "취소 중…" : "취소"}
          </button>
        )}
        <button type="button" disabled={!native || busy !== null} onClick={() => void run("clear")}>
          토큰 지우기
        </button>
      </div>
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
