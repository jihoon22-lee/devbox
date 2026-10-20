import { useEffect, useState } from "react";
import type { ClientStream } from "../../rpc/client-streams";
import type { LogFilter, LogLine, LogSource, OpenLogs } from "../../rpc/gen/rpc";
import { useRpcSession } from "../../rpc/RpcProvider";
import { LogDecoder, retainLogLines } from "./model";

export function useLogLines(sources: LogSource[], filter: LogFilter, retry: number) {
  const { client, state, generation } = useRpcSession();
  const [snapshot, setSnapshot] = useState<{ lines: LogLine[]; error: string | null; ended: boolean }>({
    lines: [],
    error: null,
    ended: false,
  });
  // A short run may finish before this view opens. Read its retained history,
  // then follow the same stream; file/container-only selections remain live tails.
  const from = sources.some((source) => source.kind === "run") ? { offset: 0 } : "tail";
  const key = JSON.stringify({ sources, from, filter } satisfies OpenLogs);
  useEffect(() => {
    // The retry counter and daemon generation deliberately reopen a failed stream.
    void retry;
    void generation;
    const params: OpenLogs = JSON.parse(key);
    setSnapshot({ lines: [], error: null, ended: false });
    if (state !== "ready" || params.sources.length === 0) return;
    let disposed = false;
    let failed = false;
    let stream: ClientStream | undefined;
    let retained: LogLine[] = [];
    const decoder = new LogDecoder();
    void client
      .openStream("logs.open", params, {
        onData(bytes, ack) {
          try {
            if (disposed || failed) return;
            retained = retainLogLines(retained, decoder.push(bytes));
            setSnapshot({ lines: retained, error: null, ended: false });
          } catch {
            failed = true;
            setSnapshot((old) => ({ ...old, error: "로그를 읽지 못했습니다. 다시 연결해 주세요.", ended: true }));
            stream?.end();
          } finally {
            ack();
          }
        },
        onEnd() {
          if (disposed || failed) return;
          try {
            decoder.finish();
            setSnapshot((old) => ({ ...old, ended: true }));
          } catch {
            setSnapshot((old) => ({ ...old, error: "로그 줄을 끝까지 받지 못했습니다.", ended: true }));
          }
        },
      })
      .then((opened) => {
        stream = opened;
        if (disposed || failed) opened.end();
      })
      .catch(() => {
        if (!disposed)
          setSnapshot((old) => ({
            ...old,
            error: "로그를 열지 못했습니다. 소스와 필터를 확인해 주세요.",
            ended: true,
          }));
      });
    return () => {
      disposed = true;
      stream?.end();
    };
  }, [client, state, generation, key, retry]);
  return { ...snapshot, connected: state === "ready" };
}
