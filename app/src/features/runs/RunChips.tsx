import { useEffect, useState } from "react";
import type { Run } from "../../rpc/gen/rpc";
import { useRpcQuery } from "../../rpc/query";
import { openExternal } from "../../shell/external";
import { Button, useToasts } from "../../ui";
import s from "./RunsPage.module.css";
export function RunChips({ run }: { run: Run }) {
  const ports = useRpcQuery("observe.ports", {}, { refetchInterval: 3000 });
  const processes = useRpcQuery(
    "observe.processes",
    { projectId: run.projectId },
    { enabled: !run.windows, refetchInterval: run.windows ? false : 3000 },
  );
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (run.endedMs !== null) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [run.endedMs]);
  const owned = run.windows ? [] : (processes.data?.filter((process) => process.runId === run.id) ?? []);
  const cpu = owned.reduce((sum, process) => sum + process.cpuPercent, 0);
  const memory = owned.reduce((sum, process) => sum + process.rssBytes, 0);
  const numbers = [
    ...new Set([
      ...(ports.data?.filter((port) => port.runId === run.id).map((port) => port.port) ?? []),
      ...(run.windows && run.ready && run.port ? [run.port] : []),
    ]),
  ];
  const elapsed = Math.max(0, Math.floor(((run.endedMs ?? now) - run.startedMs) / 1000));
  return (
    <div className={s.chips} aria-label="실행 자원">
      {numbers.map((port) => (
        <Button
          key={port}
          size="sm"
          aria-label={`포트 ${port} 브라우저로 열기`}
          onClick={() =>
            void openExternal(`http://localhost:${port}`).catch(() =>
              useToasts.getState().push({ tone: "danger", title: "브라우저를 열지 못했습니다" }),
            )
          }
        >
          :{port}
        </Button>
      ))}
      {run.windows && <span>Windows 자원 측정 없음</span>}
      <span>CPU {owned.length ? `${cpu.toFixed(1)}%` : "—"}</span>
      <span>메모리 {owned.length ? `${(memory / 1048576).toFixed(1)} MiB` : "—"}</span>
      <span>
        경과 {Math.floor(elapsed / 60)}분 {elapsed % 60}초
      </span>
      {(ports.isError || processes.isError) && (
        <Button
          size="sm"
          onClick={() => {
            void ports.refetch();
            void processes.refetch();
          }}
        >
          자원 다시 불러오기
        </Button>
      )}
    </div>
  );
}
