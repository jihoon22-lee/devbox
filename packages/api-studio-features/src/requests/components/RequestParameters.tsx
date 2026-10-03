import { lazy, Suspense } from "react";
const TlsSettings = lazy(() => import("../TlsSettings").then((module) => ({ default: module.TlsSettings })));
const AssertionEditor = lazy(() =>
  import("../AssertionEditor").then((module) => ({ default: module.AssertionEditor })),
);
const CaptureEditor = lazy(() => import("../CaptureEditor").then((module) => ({ default: module.CaptureEditor })));
const SessionVariablesPanel = lazy(() =>
  import("../SessionVariablesPanel").then((module) => ({ default: module.SessionVariablesPanel })),
);
const OAuth2Editor = lazy(() => import("../OAuth2Editor").then((module) => ({ default: module.OAuth2Editor })));
import { KeyValueEditor } from "./KeyValueEditor";
import { emptyGraphql } from "../lib/requestPresentation";
import { pickMultipartFile } from "../api";
import { CookieEditor } from "../CookieEditor";
import { GraphqlEditor } from "../GraphqlEditor";
import { HeaderTable } from "../HeaderTable";
import { MultipartEditor } from "../MultipartEditor";
import { hasActiveCookieHeader } from "../lib/cookies";
import type * as React from "react";

interface Props {
  tab: "params" | "headers" | "cookies" | "body" | "auth" | "tls" | "assertions" | "captures";
  checks: {
    assertions: import("../lib/assertions").Assertion[];
    onAssertionsChange: (values: import("../lib/assertions").Assertion[]) => void;
    captures: import("../lib/captures").Capture[];
    onCapturesChange: (values: import("../lib/captures").Capture[]) => void;
    session: import("../lib/runner").SessionVariables;
    onSessionChange: () => void;
    disabled: boolean;
  };
  req: import("../types").RequestTemplate;
  setReq: React.Dispatch<React.SetStateAction<import("../types").RequestTemplate>>;
  currentEnv: import("../lib/environments").Environment | null;
  requestEditorRevision: number;
  BODY_KINDS: string[];
  setAuth: (patch: Partial<import("../../generated/AuthConfig").AuthConfig>) => void;
  AUTH_KINDS: string[];
  oauthEnvironment: import("../lib/environments").EnvVariable[];
  oauthStatusKey: number;
  oauthLoginRequest: number;
  onOAuthLoginHandled: () => void;
}

export function RequestParameters({
  tab,
  checks,
  req,
  setReq,
  currentEnv,
  requestEditorRevision,
  BODY_KINDS,
  setAuth,
  AUTH_KINDS,
  oauthEnvironment,
  oauthStatusKey,
  oauthLoginRequest,
  onOAuthLoginHandled,
}: Props) {
  return (
    <div className="tab-body">
      {tab === "tls" && (
        <Suspense fallback={<p role="status">TLS 설정 준비 중…</p>}>
          <TlsSettings value={req.tls} onChange={(tls) => setReq((current) => ({ ...current, tls }))} />
        </Suspense>
      )}
      {tab === "assertions" && (
        <Suspense fallback={<p role="status">검증 편집 준비 중…</p>}>
          <AssertionEditor value={checks.assertions} onChange={checks.onAssertionsChange} disabled={checks.disabled} />
        </Suspense>
      )}
      {tab === "captures" && (
        <Suspense fallback={<p role="status">캡처 편집 준비 중…</p>}>
          <CaptureEditor value={checks.captures} onChange={checks.onCapturesChange} disabled={checks.disabled} />
          <SessionVariablesPanel
            session={checks.session}
            onChange={checks.onSessionChange}
            disabled={checks.disabled}
          />
        </Suspense>
      )}

      {tab === "params" && (
        <KeyValueEditor rows={req.params} onChange={(params) => setReq({ ...req, params })} namePlaceholder="쿼리 이름" />
      )}
      {tab === "headers" && (
        <HeaderTable
          rows={req.headers}
          secretNames={(currentEnv?.variables ?? [])
            .filter((variable) => variable.secret)
            .map((variable) => variable.key)}
          onChange={(headers) => setReq({ ...req, headers })}
        />
      )}
      {tab === "cookies" && (
        <CookieEditor
          key={requestEditorRevision}
          rows={req.cookies}
          secretNames={(currentEnv?.variables ?? [])
            .filter((variable) => variable.secret)
            .map((variable) => variable.key)}
          hasRawCookieHeader={hasActiveCookieHeader(req.headers)}
          onChange={(cookies) => setReq({ ...req, cookies })}
        />
      )}
      {tab === "body" && (
        <div>
          <select
            className="select-sm"
            aria-label="요청 본문 형식"
            value={req.body_kind}
            onChange={(e) => {
              const bodyKind = e.currentTarget.value;
              setReq({
                ...req,
                body_kind: bodyKind,
                method: bodyKind === "graphql" && !["GET", "POST"].includes(req.method) ? "POST" : req.method,
                graphql: bodyKind === "graphql" ? (req.graphql ?? emptyGraphql()) : null,
              });
            }}
          >
            {BODY_KINDS.map((k) => (
              <option key={k} value={k}>
                {k}
              </option>
            ))}
          </select>
          {req.body_kind === "graphql" ? (
            <GraphqlEditor
              key={requestEditorRevision}
              value={req.graphql ?? emptyGraphql()}
              onChange={(graphql) => setReq({ ...req, graphql, body: "" })}
            />
          ) : req.body_kind === "multipart" ? (
            <MultipartEditor
              key={requestEditorRevision}
              rows={req.multipart}
              secretNames={(currentEnv?.variables ?? [])
                .filter((variable) => variable.secret)
                .map((variable) => variable.key)}
              onChange={(multipart) => setReq({ ...req, multipart })}
              onPickFile={pickMultipartFile}
            />
          ) : (
            req.body_kind !== "none" && (
              <textarea
                className="body-input"
                aria-label="요청 본문"
                rows={8}
                placeholder={req.body_kind === "json" ? '{ "key": "value" }' : "key=value"}
                value={req.body}
                onChange={(e) => setReq({ ...req, body: e.currentTarget.value })}
                spellCheck={false}
              />
            )
          )}
        </div>
      )}
      {tab === "auth" && (
        <div className="auth-body">
          <select
            className="select-sm"
            aria-label="인증 종류"
            value={req.auth?.kind ?? "none"}
            onChange={(e) => setAuth({ kind: e.currentTarget.value })}
          >
            {AUTH_KINDS.map((k) => (
              <option key={k} value={k}>
                {k === "oauth2" ? "OAuth 2.0" : k}
              </option>
            ))}
          </select>
          {req.auth?.kind === "oauth2" && (
            <Suspense fallback={<p>인증 설정을 불러오는 중…</p>}>
              <OAuth2Editor
                auth={req.auth}
                environment={oauthEnvironment}
                onChange={(oauth2) => setAuth({ oauth2 })}
                statusKey={oauthStatusKey}
                loginRequest={oauthLoginRequest}
                onLoginHandled={onOAuthLoginHandled}
              />
            </Suspense>
          )}
          {req.auth?.kind === "basic" && (
            <div className="kv-row">
              <input
                aria-label="사용자 이름"
                placeholder="사용자 이름"
                value={req.auth.username}
                onChange={(e) => setAuth({ username: e.currentTarget.value })}
              />
              <input
                aria-label="비밀번호"
                placeholder="비밀번호"
                type="password"
                value={req.auth.password}
                onChange={(e) => setAuth({ password: e.currentTarget.value })}
              />
            </div>
          )}
          {req.auth?.kind === "bearer" && (
            <div className="kv-row">
              <input
                aria-label="토큰"
                placeholder="토큰"
                value={req.auth.token}
                onChange={(e) => setAuth({ token: e.currentTarget.value })}
              />
            </div>
          )}
          {req.auth?.kind === "apikey" && (
            <div className="kv-row">
              <input
                aria-label="API 키 헤더 이름"
                placeholder="헤더 이름 (예: X-API-Key)"
                value={req.auth.api_key}
                onChange={(e) => setAuth({ api_key: e.currentTarget.value })}
              />
              <input
                aria-label="API 키 값"
                placeholder="값"
                value={req.auth.api_value}
                onChange={(e) => setAuth({ api_value: e.currentTarget.value })}
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
}
