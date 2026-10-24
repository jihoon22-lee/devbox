/**
 * MCP 클라이언트(07b S5-5): stdio(로컬 명령) 또는 Streamable HTTP 서버에 연결해 도구·리소스·프롬프트를 쓴다.
 * 실행 규칙: 검토하지 않은 프로젝트 `*.mcp.json`은 [검토하고 사용] 전에 연결하지 않고, stdio는 출처와 관계없이
 * 정확한 명령·실제 작업 폴더·env `이름=값`(비밀 참조는 참조로)을 보여 주는 확인 대화상자를 거친 뒤에만 실행한다. 데몬도 파일 출처를 다시
 * 확인한다(digest·검토·정의 일치). 검토하지 않은 프로젝트 환경의 변수 값은 쓰지 않는다.
 */
import { useState } from "react";
import type { CollectionRoot, McpConnect, TlsOptions } from "../../../../rpc/gen/rpc";
import { openExternal } from "../../../../shell/external";
import { Banner, Button, ConfirmDialog } from "../../../../ui";
import s from "../Api.module.css";
import { type Row, row } from "../model/draft";
import { ParamTable } from "../ParamTable";
import { ProfileFiles, type RootChoice, useProfileFile } from "../profileFile";
import { failureText } from "../realtime/messages";
import { toolEnv } from "../toolEnv";
import { McpCatalog } from "./McpCatalog";
import {
  commandLine,
  effectiveCwd,
  envPairs,
  type FileTransport,
  MCP_FILE_SUFFIX,
  parseMcpFile,
  toTransport,
} from "./session";
import { useMcpSession } from "./useMcpSession";

const TLS: TlsOptions = { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null };
const STDIO_URL = "http://stdio.invalid/";

const fileRows = (rows: Row[]) => rows.map(({ name, value, enabled }) => ({ name, value, enabled }));

function parsed(text: string): { transport: FileTransport } | { error: string } {
  try {
    return { transport: parseMcpFile(text).transport };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "MCP 파일을 읽지 못했습니다" };
  }
}

/** 변수 규칙은 데몬 치환 범위와 같다: stdio는 env 값만, HTTP는 주소·헤더·TLS. */
function variables(t: FileTransport) {
  const toRows = (list: { name: string; value: string; enabled: boolean }[]) =>
    list.map((r) => ({ ...row(r.name, r.value), enabled: r.enabled }));
  return t.kind === "stdio"
    ? toolEnv({ url: STDIO_URL, headers: toRows(t.env), tls: TLS })
    : toolEnv({ url: t.url, headers: toRows(t.headers), tls: t.tls });
}

export function McpPanel({ roots }: { roots: RootChoice[] }) {
  const profile = useProfileFile();
  const mcp = useMcpSession();
  const [kind, setKind] = useState<"stdio" | "http">("stdio");
  const [command, setCommand] = useState("");
  const [argsText, setArgsText] = useState("");
  const [cwd, setCwd] = useState("");
  const [env, setEnv] = useState<Row[]>([]);
  const [url, setUrl] = useState("");
  const [headers, setHeaders] = useState<Row[]>([]);
  const [verify, setVerify] = useState(true);
  const [notice, setNotice] = useState<string>();
  const [confirm, setConfirm] = useState<{ params: McpConnect; cwd: string } | null>(null);

  const file = profile.file;
  const fromFile = file ? parsed(file.text) : null;
  const draft: FileTransport =
    kind === "stdio"
      ? {
          kind,
          command: command.trim(),
          // 한 줄이 인자 하나다. 완전히 빈 줄만 뺀다(공백이 든 인자는 그대로).
          args: argsText.split("\n").filter((line) => line !== ""),
          cwd: cwd.trim() || null,
          env: fileRows(env),
        }
      : { kind, url: url.trim(), headers: fileRows(headers), tls: { ...TLS, verify } };
  const transport = fromFile ? ("transport" in fromFile ? fromFile.transport : null) : draft;
  const blocked = file !== null && !file.reviewed;
  const live = mcp.state?.status === "connecting" || mcp.state?.status === "ready";
  const project = file?.root.kind === "project" ? (roots.find((r) => sameRoot(r.root, file.root)) ?? null) : null;
  const cwdShown = transport ? effectiveCwd(transport, project) : null;
  // stdio는 실제 작업 폴더가 있어야 한다(초안·개인 파일은 cwd를 적어야 함, 데몬도 같은 규칙).
  const noCwd = transport?.kind === "stdio" && cwdShown === null;
  const empty = !transport || (transport.kind === "stdio" ? !transport.command || noCwd : !transport.url);

  function request() {
    if (!transport || blocked || empty) return;
    const p = variables(transport);
    if ("blocked" in p) return setNotice(p.blocked);
    setNotice(undefined);
    const params: McpConnect = {
      transport: toTransport(transport),
      env: p.env,
      source: file ? { kind: "file", root: file.root, path: file.path, digest: file.digest } : { kind: "draft" },
    };
    if (transport.kind === "stdio") setConfirm({ params, cwd: cwdShown ?? "" });
    else void mcp.connect(params);
  }

  async function open(root: CollectionRoot, path: string) {
    mcp.close();
    await profile.load(root, path);
  }

  const state = mcp.state;
  const shownNotice = notice ?? profile.notice ?? (fromFile && "error" in fromFile ? fromFile.error : undefined);
  return (
    <section className={s.pane} aria-label="MCP">
      <ProfileFiles roots={roots} suffix={MCP_FILE_SUFFIX} label="MCP 파일" onOpen={(r, p) => void open(r, p)} />
      {file && transport && (
        <div>
          <p className={s.muted}>
            {file.path}
            <Button size="sm" variant="ghost" onClick={profile.clear}>
              파일 닫기
            </Button>
          </p>
          <pre className={s.mono} aria-label={transport.kind === "stdio" ? "실행할 명령" : "연결 주소"}>
            {commandLine(transport)}
          </pre>
          {transport.kind === "stdio" && <RunDetails transport={transport} cwd={cwdShown} />}
        </div>
      )}
      {file && noCwd && (
        <Banner tone="warn">
          이 파일에는 작업 폴더(cwd)를 적어야 합니다. 프로젝트 파일이 아니면 등록 프로젝트 안의 절대 경로가 필요합니다.
        </Banner>
      )}
      {blocked && (
        <Banner
          tone="warn"
          actions={
            <Button size="sm" onClick={() => void profile.review()}>
              검토하고 사용
            </Button>
          }
        >
          검토 필요: 이 프로젝트 파일은 이 PC에서 검토하지 않았습니다. 위 명령·주소를 확인하세요.
        </Banner>
      )}
      {!file && (
        <McpDraftForm
          kind={kind}
          onKind={setKind}
          stdio={{ command, setCommand, argsText, setArgsText, cwd, setCwd, env, setEnv }}
          http={{ url, setUrl, headers, setHeaders, verify, setVerify }}
        />
      )}
      <div className={s.bar}>
        <Button variant="primary" disabled={blocked || empty || live} onClick={request}>
          연결 열기
        </Button>
        {live && <Button onClick={mcp.close}>연결 닫기</Button>}
      </div>
      {shownNotice && <Banner tone="warn">{shownNotice}</Banner>}
      {state?.authorizeUrl && (
        <Banner
          tone="warn"
          actions={
            <Button
              size="sm"
              onClick={() =>
                void openExternal(state.authorizeUrl ?? "").catch(() => setNotice("브라우저를 열지 못했습니다."))
              }
            >
              브라우저에서 로그인
            </Button>
          }
        >
          이 MCP 서버는 로그인이 필요합니다. 브라우저에서 로그인을 마치면 연결이 이어집니다.
        </Banner>
      )}
      {state && (
        <p className={s.muted}>
          {state.status === "connecting" && "연결 중…"}
          {state.status === "ready" &&
            state.server &&
            `연결됨: ${state.server.name} ${state.server.version} (MCP ${state.server.protocol})`}
          {state.status === "closed" && "연결이 끝났습니다."}
        </p>
      )}
      {state?.failure && <Banner tone="danger">{failureText(state.failure)}</Banner>}
      {state?.status === "ready" && <McpCatalog state={state} send={mcp.send} />}
      <ConfirmDialog
        open={confirm !== null}
        title="MCP 서버 실행"
        body="아래 명령을 이 PC에서 사용자 권한으로 실행합니다. 믿을 수 있는 명령인지 확인하세요."
        confirmLabel="실행"
        onCancel={() => setConfirm(null)}
        onConfirm={() => {
          const pending = confirm;
          setConfirm(null);
          if (pending) void mcp.connect(pending.params);
        }}
      >
        {confirm?.params.transport.kind === "stdio" && <ConfirmBody params={confirm.params} cwd={confirm.cwd} />}
      </ConfirmDialog>
    </section>
  );
}

const sameRoot = (a: CollectionRoot, b: CollectionRoot) => JSON.stringify(a) === JSON.stringify(b);

/** 검토 화면: env `이름=값`(적힌 그대로, 비밀 참조는 참조로)과 실제 작업 폴더. */
function RunDetails({ transport, cwd }: { transport: FileTransport; cwd: string | null }) {
  const env = envPairs(transport);
  return (
    <>
      <pre className={s.mono} aria-label="실행 환경 변수">
        {env.length ? env.join("\n") : "(없음)"}
      </pre>
      <p className={s.mono} aria-label="실행 작업 폴더">
        {cwd ?? "없음"}
      </p>
    </>
  );
}

/** 확인 대화상자는 보낼 요청 그대로를 보인다(env 값은 풀지 않은 그대로, 작업 폴더는 실제로 쓸 곳). */
function ConfirmBody({ params, cwd }: { params: McpConnect; cwd: string }) {
  const t = params.transport;
  if (t.kind !== "stdio") return null;
  const shown: FileTransport = { ...t, env: t.env.map((p) => ({ ...p, enabled: true })) };
  const env = envPairs(shown);
  return (
    <div>
      <pre className={s.mono}>{commandLine(shown)}</pre>
      <p>작업 폴더: {cwd}</p>
      <p>환경 변수:</p>
      <pre className={s.mono}>{env.length ? env.join("\n") : "(없음)"}</pre>
      <p>출처: {params.source.kind === "file" ? `파일 ${params.source.path}` : "직접 입력"}</p>
    </div>
  );
}

function McpDraftForm({
  kind,
  onKind,
  stdio,
  http,
}: {
  kind: "stdio" | "http";
  onKind: (kind: "stdio" | "http") => void;
  stdio: {
    command: string;
    setCommand: (v: string) => void;
    argsText: string;
    setArgsText: (v: string) => void;
    cwd: string;
    setCwd: (v: string) => void;
    env: Row[];
    setEnv: (v: Row[]) => void;
  };
  http: {
    url: string;
    setUrl: (v: string) => void;
    headers: Row[];
    setHeaders: (v: Row[]) => void;
    verify: boolean;
    setVerify: (v: boolean) => void;
  };
}) {
  return (
    <div>
      <select
        className={s.input}
        aria-label="연결 방식"
        value={kind}
        onChange={(event) => onKind(event.target.value === "http" ? "http" : "stdio")}
      >
        <option value="stdio">stdio(로컬 명령)</option>
        <option value="http">HTTP</option>
      </select>
      {kind === "stdio" ? (
        <>
          <input
            className={`${s.input} ${s.mono} ${s.grow}`}
            aria-label="명령"
            placeholder="node, python3, /usr/local/bin/server …"
            value={stdio.command}
            onChange={(event) => stdio.setCommand(event.target.value)}
          />
          <textarea
            className={`${s.input} ${s.mono}`}
            aria-label="인자"
            placeholder="한 줄에 인자 하나"
            value={stdio.argsText}
            onChange={(event) => stdio.setArgsText(event.target.value)}
          />
          <input
            className={`${s.input} ${s.mono}`}
            aria-label="작업 폴더"
            placeholder="등록한 프로젝트 안의 절대 경로(필수)"
            value={stdio.cwd}
            onChange={(event) => stdio.setCwd(event.target.value)}
          />
          <ParamTable label="환경 변수" rows={stdio.env} onChange={stdio.setEnv} />
        </>
      ) : (
        <>
          <input
            className={`${s.input} ${s.mono} ${s.grow}`}
            aria-label="MCP 주소"
            placeholder="https://…/mcp"
            value={http.url}
            onChange={(event) => http.setUrl(event.target.value)}
          />
          <ParamTable label="헤더" rows={http.headers} onChange={http.setHeaders} />
          <label className={s.bar}>
            <input type="checkbox" checked={http.verify} onChange={(event) => http.setVerify(event.target.checked)} />
            인증서 검증
          </label>
        </>
      )}
    </div>
  );
}
