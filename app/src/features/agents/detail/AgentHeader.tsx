import type { Methods } from "../../../rpc/gen/rpc";
import { useRpcQuery } from "../../../rpc/query";
import { Button } from "../../../ui";
import { AgentStateMark } from "../AgentStateMark";
import type { AgentTask } from "../model";
import { attentionNote, failureLabel } from "../model";
import { TokenMetrics } from "../TokenMetrics";
import { useAgentsUi } from "../ui";
import { CleanupNotice } from "./CleanupNotice";
import s from "./Detail.module.css";
import { openAgentPr } from "./links";
export function AgentHeader({
  task,
  projectName,
  toolName,
  now,
  resource,
  behind,
  popout = false,
}: {
  task: AgentTask;
  projectName: string;
  toolName: string;
  now: number;
  resource?: Methods["agents.resources"]["result"]["items"][number];
  behind?: number;
  /** Shown in a popout window, where [같은 지시로 새 작업] is left out. */
  popout?: boolean;
}) {
  const usage = useRpcQuery("agents.usage", { id: task.id });
  const seconds = Math.max(0, Math.floor(((task.closedMs ?? now) - task.createdMs) / 1000));
  const tokens = usage.data ? usage.data.tokens : task.tokens;
  const note = attentionNote(task);
  return (
    <header className={s.header}>
      <div className={s.heading}>
        <h1>{task.title}</h1>
        {/* Open or closed: only fills the new task form with the first prompt and the project. [시작] starts it.
            Not in a popout: the form opens the new task in the window it started from, which would turn the
            popout into the agents page. */}
        {!popout && (
          <Button
            size="sm"
            onClick={() => useAgentsUi.getState().setCreating(true, { prompt: task.prompt, projectId: task.projectId })}
          >
            같은 지시로 새 작업
          </Button>
        )}
      </div>
      <div className={s.metrics}>
        <AgentStateMark state={task.state} />
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
      {note && <p>{note}</p>}
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
      {task.cleanupError && <CleanupNotice error={task.cleanupError} />}
    </header>
  );
}
