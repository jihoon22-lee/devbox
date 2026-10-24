/**
 * gRPC 도구(07b S5-5): 대상·메타데이터·TLS·proto 파일 → [서비스 불러오기] → 메서드 고르기·요청 JSON → [호출].
 * 검토하지 않은 프로젝트 `*.grpc.json`은 [검토하고 사용] 전에는 불러오기·호출에 쓰지 않고, 검토하지 않은
 * 프로젝트 환경의 변수 값도 쓰지 않는다(편집기 보내기와 같은 규칙).
 */
import { useCallback, useState } from "react";
import type { CollectionRoot, GrpcCatalog, TlsOptions } from "../../../../rpc/gen/rpc";
import { Banner, Button } from "../../../../ui";
import s from "../Api.module.css";
import { SendFailure, useApiBackend } from "../backend";
import type { Row } from "../model/draft";
import { ParamTable } from "../ParamTable";
import { ProfileFiles, type RootChoice, useProfileFile } from "../profileFile";
import { failureText } from "../realtime/messages";
import { toolEnv } from "../toolEnv";
import {
  allMethods,
  type Drafts,
  draftKey,
  draftOf,
  editDraft,
  emptyDrafts,
  GRPC_FILE_SUFFIX,
  parseGrpcFile,
  resultTitle,
  STATUS_TEXT,
} from "./grpcDrafts";
import { MethodPicker } from "./MethodPicker";
import { useGrpcCalls } from "./useGrpcCalls";

const TLS: TlsOptions = { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null };

/** 대상·메타데이터·요청 JSON → 편집기 보내기와 같은 변수 규칙(`toolEnv`). */
function prepared(target: string, metadata: Row[], tls: TlsOptions, json: string | null) {
  const p = toolEnv({ url: target, headers: metadata, tls, ...(json === null ? {} : { json }) });
  if ("blocked" in p) return p;
  return { target: p.snapshot.url, metadata: p.snapshot.headers, tls: p.snapshot.tls, env: p.env };
}

export function GrpcPanel({ roots }: { roots: RootChoice[] }) {
  const backend = useApiBackend();
  const profile = useProfileFile();
  const { calls, start, cancel } = useGrpcCalls();
  const [target, setTarget] = useState("");
  const [metadata, setMetadata] = useState<Row[]>([]);
  const [tls, setTls] = useState<TlsOptions>(TLS);
  const [protoText, setProtoText] = useState("");
  const [catalog, setCatalog] = useState<GrpcCatalog | null>(null);
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [drafts, setDrafts] = useState<Drafts>(emptyDrafts);
  const [notice, setNotice] = useState<string>();
  const [failure, setFailure] = useState<{ code: string; detail?: unknown }>();
  const blocked = profile.file !== null && !profile.file.reviewed;
  const protoFiles = protoText
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const method = allMethods(catalog).find((m) => m.fullName === selected) ?? null;
  const draft = method ? draftOf(drafts, target, method) : null;
  const select = useCallback((next: string) => setSelected(next), []);

  async function open(root: CollectionRoot, path: string) {
    const loaded = await profile.load(root, path);
    if (!loaded) return;
    try {
      const file = parseGrpcFile(loaded.text);
      setTarget(file.target);
      setMetadata(file.metadata);
      setTls(file.tls);
      setProtoText(file.protoFiles.join("\n"));
      setSelected(file.method);
      setCatalog(null);
      if (file.method && file.json !== null) {
        const next = new Map(emptyDrafts());
        next.set(draftKey(file.target, file.method), { json: file.json, dirty: false });
        setDrafts(next);
      }
    } catch (error) {
      profile.clear();
      setNotice(error instanceof Error ? error.message : "gRPC 파일을 읽지 못했습니다.");
    }
  }

  async function describe() {
    if (blocked) return;
    const p = prepared(target, metadata, tls, null);
    if ("blocked" in p) return setNotice(p.blocked);
    setNotice(undefined);
    setFailure(undefined);
    try {
      setCatalog(await backend.grpcDescribe({ ...p, protoFiles: protoFiles.length ? protoFiles : null }));
    } catch (error) {
      setFailure(error instanceof SendFailure ? { code: error.code, detail: error.detail } : { code: "internal" });
    }
  }

  function invoke() {
    if (blocked || !method || !draft) return;
    const p = prepared(target, metadata, tls, draft.json);
    if ("blocked" in p) return setNotice(p.blocked);
    setNotice(undefined);
    void start({
      ...p,
      method: method.fullName,
      json: draft.json,
      timeoutMs: 30_000,
      protoFiles: protoFiles.length ? protoFiles : null,
      origin: profile.file ? { root: profile.file.root, path: profile.file.path } : null,
    });
  }

  const latest = calls[0];
  const shownNotice = notice ?? profile.notice;
  return (
    <section className={s.pane} aria-label="gRPC">
      <ProfileFiles roots={roots} suffix={GRPC_FILE_SUFFIX} label="gRPC 파일" onOpen={(r, p) => void open(r, p)} />
      {blocked && (
        <Banner
          tone="warn"
          actions={
            <Button size="sm" onClick={() => void profile.review()}>
              검토하고 사용
            </Button>
          }
        >
          검토 필요: 이 프로젝트 파일은 이 PC에서 검토하지 않았습니다. 대상·메타데이터·proto 파일을 확인하세요.
        </Banner>
      )}
      <div className={s.bar}>
        <input
          className={`${s.input} ${s.mono} ${s.grow}`}
          aria-label="gRPC 대상"
          placeholder="http://127.0.0.1:50051 또는 https://…"
          value={target}
          onChange={(event) => setTarget(event.target.value)}
        />
        <Button disabled={blocked || !target.trim()} onClick={() => void describe()}>
          서비스 불러오기
        </Button>
      </div>
      <details>
        <summary>메타데이터·TLS·proto 파일</summary>
        <ParamTable label="메타데이터" rows={metadata} onChange={setMetadata} />
        <label className={s.bar}>
          <input
            type="checkbox"
            checked={tls.verify}
            onChange={(event) => setTls({ ...tls, verify: event.target.checked })}
          />
          인증서 검증
        </label>
        <input
          className={`${s.input} ${s.grow}`}
          aria-label="CA 인증서 경로"
          placeholder="/home/…/ca.pem"
          value={tls.caPath ?? ""}
          onChange={(event) => setTls({ ...tls, caPath: event.target.value.trim() || null })}
        />
        <textarea
          className={`${s.input} ${s.mono}`}
          aria-label="proto 파일"
          placeholder="reflection이 없으면 .proto 절대 경로를 한 줄에 하나씩"
          value={protoText}
          onChange={(event) => setProtoText(event.target.value)}
        />
      </details>
      {shownNotice && <Banner tone="warn">{shownNotice}</Banner>}
      {failure && <Banner tone="danger">{failureText(failure)}</Banner>}
      <MethodPicker
        catalog={catalog}
        filter={filter}
        selected={selected}
        dirty={(m) => drafts.get(draftKey(target, m))?.dirty === true}
        onFilter={setFilter}
        onSelect={select}
      />
      <textarea
        className={`${s.input} ${s.mono}`}
        aria-label="요청 JSON"
        disabled={!method}
        value={draft?.json ?? ""}
        onChange={(event) => method && setDrafts(editDraft(drafts, target, method.fullName, event.target.value))}
      />
      <div className={s.bar}>
        <Button variant="primary" disabled={blocked || !method || method.clientStreaming} onClick={invoke}>
          호출
        </Button>
        {latest?.status === "running" && <Button onClick={() => cancel(latest.requestId)}>취소</Button>}
      </div>
      {latest && (
        <section role="region" aria-label="호출 결과">
          <h3>{resultTitle(latest, selected)}</h3>
          <p className={s.muted}>
            {latest.method} · {STATUS_TEXT[latest.status]}
            {latest.trailers &&
              ` · 상태 ${latest.trailers.code}${latest.trailers.message ? ` ${latest.trailers.message}` : ""}`}
          </p>
          {latest.failure && <Banner tone="danger">{failureText(latest.failure)}</Banner>}
          {latest.messages.map((m, i) => (
            <pre key={i} className={s.mono}>
              {m}
            </pre>
          ))}
        </section>
      )}
      <ul aria-label="호출 기록" className={s.tree}>
        {calls.map((call) => (
          <li key={call.requestId}>
            {call.method} · {STATUS_TEXT[call.status]}
          </li>
        ))}
      </ul>
    </section>
  );
}
