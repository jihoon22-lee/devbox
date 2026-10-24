import { lazy, Suspense, useEffect, useState } from "react";
import type { Language, ServerState, ServersResult } from "../../../rpc/gen/rpc";
import { useRpcSession } from "../../../rpc/RpcProvider";
import s from "../source.module.css";

const TerminalView = lazy(() =>
  import("../../terminal/TerminalView").then((module) => ({ default: module.TerminalView })),
);
const LABEL: Record<ServerState, string> = {
  missing: "설치되지 않음",
  starting: "시작 중",
  ready: "실행 중",
  failed: "실행 실패",
  idle_stopped: "유휴 상태로 중지됨",
};

export function LspStatus({ projectId, language }: { projectId: string; language: Language }) {
  const { client, state } = useRpcSession();
  const [data, setData] = useState<ServersResult>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [terminal, setTerminal] = useState<string>();
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    setData(undefined);
    if (!client || state !== "ready") return;
    let live = true,
      requested = refresh;
    const load = async () => {
      const request = ++requested;
      try {
        const value = await client.call("lsp.servers", { projectId });
        if (live && requested === request) setData(value);
      } catch {
        if (live && requested === request) setError("언어 서버 상태를 불러오지 못했습니다.");
      }
    };
    void load();
    const timer = setInterval(() => void load(), 3000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [client, state, projectId, refresh]);
  const item = data?.items.find((entry) => entry.language === language);
  const action = async (install: boolean) => {
    if (!item || busy) return;
    setBusy(true);
    setError(undefined);
    try {
      if (install && item.install) {
        const installed = await client.call("lsp.install", { projectId, language, expectedCommand: item.install });
        setTerminal(installed.terminalId);
      } else if (!install) await client.call("lsp.restart", { projectId, language });
      setRefresh((n) => n + 1);
    } catch {
      setError(
        install ? "설치 터미널을 열지 못했습니다. 상태를 새로 확인해 주세요." : "언어 서버를 다시 시작하지 못했습니다.",
      );
    } finally {
      setBusy(false);
    }
  };
  return (
    <details className={s.lspStatus}>
      <summary>언어 서버 · {item ? LABEL[item.state] : "연결 대기"}</summary>
      {item && (
        <>
          <p>
            {item.command} · <span>{LABEL[item.state]}</span>
            {item.pid !== null && ` · PID ${item.pid}`}
            {item.memoryBytes !== null && ` · ${(item.memoryBytes / 1048576).toFixed(1)} MiB`}
          </p>
          {item.install && item.state === "missing" && (
            <>
              <code>{item.install}</code>
              <button type="button" disabled={busy || state !== "ready"} onClick={() => void action(true)}>
                설치
              </button>
            </>
          )}
          <button
            type="button"
            disabled={busy || state !== "ready" || item.state === "missing"}
            onClick={() => void action(false)}
          >
            다시 시작
          </button>
        </>
      )}
      {data && (
        <p>
          소유한 언어 서버의 inotify: 인스턴스 {data.inotify.ownedInstances} / 사용자 한도{" "}
          {data.inotify.maxUserInstances ?? "알 수 없음"}, 감시 {data.inotify.ownedWatches} / 사용자 한도{" "}
          {data.inotify.maxUserWatches ?? "알 수 없음"}.{" "}
          {data.inotify.usageComplete ? "소유한 프로세스 집계입니다." : "일부 프로세스를 읽지 못한 부분 집계입니다."}{" "}
          다른 프로그램의 사용량은 포함하지 않습니다.
        </p>
      )}
      {error && <p role="alert">{error}</p>}
      {terminal && (
        <section aria-label="언어 서버 설치 터미널">
          <button type="button" onClick={() => setTerminal(undefined)}>
            터미널 숨기기
          </button>
          <div className={s.installTerminal}>
            <Suspense fallback={<p>터미널을 여는 중입니다.</p>}>
              <TerminalView id={terminal} />
            </Suspense>
          </div>
        </section>
      )}
    </details>
  );
}
