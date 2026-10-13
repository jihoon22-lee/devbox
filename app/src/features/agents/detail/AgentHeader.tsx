import type { Methods } from "../../../rpc/gen/rpc";
import { useRpcQuery } from "../../../rpc/query";
import { Button, StatusDot } from "../../../ui";
import type { AgentTask } from "../model";
import { failureLabel, STATE_LABEL } from "../model";
import { TokenMetrics } from "../TokenMetrics";
import s from "./Detail.module.css";
import { openAgentPr } from "./links";
export function AgentHeader({
  task,
  projectName,
  toolName,
  now,
  resource,
  behind,
}: {
  task: AgentTask;
  projectName: string;
  toolName: string;
  now: number;
  resource?: Methods["agents.resources"]["result"]["items"][number];
  behind?: number;
}) {
  const usage = useRpcQuery("agents.usage", { id: task.id });
  const seconds = Math.max(0, Math.floor(((task.closedMs ?? now) - task.createdMs) / 1000));
  const tone =
    task.state === "failed"
      ? "danger"
      : task.state === "waiting"
        ? "warn"
        : task.state === "running" || task.state === "preparing"
          ? "progress"
          : task.state === "review"
            ? "ok"
            : "idle";
  const tokens = usage.data ? usage.data.tokens : task.tokens;
  return (
    <header className={s.header}>
      <h1>{task.title}</h1>
      <div className={s.metrics}>
        <StatusDot tone={tone} label={STATE_LABEL[task.state]} />
        <span>{STATE_LABEL[task.state]}</span>
        <span>{projectName}</span>
        <span>{task.branch ?? task.baseBranch}</span>
        <span>{toolName}</span>
        <span>경과 {seconds < 60 ? `${seconds}초` : `${Math.floor(seconds / 60)}분`}</span>
        <span>CPU {resource?.cpuPercent == null ? "알 수 없음" : `${resource.cpuPercent.toFixed(1)}%`}</span>
        <span>
          메모리 {resource?.memoryBytes == null ? "알 수 없음" : `${(resource.memoryBytes / 1048576).toFixed(0)} MB`}
        </span>
        {tokens ? (
          <TokenMetrics tokens={tokens} truncated={usage.data?.truncated ?? task.tokensTruncated} />
        ) : (
          <span>토큰 알 수 없음</span>
        )}
        {behind !== undefined && <span>뒤처짐 {behind}</span>}
      </div>
      {usage.error && <p role="status">토큰 기록을 다시 확인하지 못했습니다.</p>}
      {task.state === "failed" && <p role="alert">{failureLabel(task.reason)}</p>}
      {task.attention?.message && <p>{task.attention.message}</p>}
      {task.state === "closed" && (
        <p>
          닫힘 · {task.closedHow === "merged" ? "병합됨" : task.closedHow === "pullRequest" ? "PR" : "폐기"} ·{" "}
          {new Date(task.closedMs ?? task.updatedMs).toLocaleString("ko")}
          {task.prUrl && (
            <Button size="sm" onClick={() => openAgentPr(task.prUrl!)}>
              PR 열기
            </Button>
          )}
        </p>
      )}
      {task.cleanupError && <p role="alert">정리하지 못한 항목이 있습니다. {task.cleanupError}</p>}
    </header>
  );
}
