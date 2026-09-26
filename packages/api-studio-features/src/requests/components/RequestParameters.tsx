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
  tab: "params" | "headers" | "cookies" | "body" | "auth";
  req: import("../types").RequestTemplate;
  setReq: React.Dispatch<React.SetStateAction<import("../types").RequestTemplate>>;
  currentEnv: import("../lib/environments").Environment | null;
  requestEditorRevision: number;
  BODY_KINDS: string[];
  setAuth: (patch: Partial<import("../../generated/AuthConfig").AuthConfig>) => void;
  AUTH_KINDS: string[];
}

export function RequestParameters({
  tab,
  req,
  setReq,
  currentEnv,
  requestEditorRevision,
  BODY_KINDS,
  setAuth,
  AUTH_KINDS,
}: Props) {
  return (
    <div className="tab-body">
      {tab === "params" && (
        <KeyValueEditor rows={req.params} onChange={(params) => setReq({ ...req, params })} namePlaceholder="키" />
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
            value={req.auth?.kind ?? "none"}
            onChange={(e) => setAuth({ kind: e.currentTarget.value })}
          >
            {AUTH_KINDS.map((k) => (
              <option key={k} value={k}>
                {k}
              </option>
            ))}
          </select>
          {req.auth?.kind === "basic" && (
            <div className="kv-row">
              <input
                placeholder="사용자 이름"
                value={req.auth.username}
                onChange={(e) => setAuth({ username: e.currentTarget.value })}
              />
              <input
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
                placeholder="토큰"
                value={req.auth.token}
                onChange={(e) => setAuth({ token: e.currentTarget.value })}
              />
            </div>
          )}
          {req.auth?.kind === "apikey" && (
            <div className="kv-row">
              <input
                placeholder="헤더 이름 (예: X-API-Key)"
                value={req.auth.api_key}
                onChange={(e) => setAuth({ api_key: e.currentTarget.value })}
              />
              <input
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
