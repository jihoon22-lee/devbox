import { messageFor } from "../../rpc/messages";
import { useRpcQuery } from "../../rpc/query";
import { Button, EmptyState, ErrorState } from "../../ui";
import { useAgentData } from "./AgentData";
import s from "./Agents.module.css";
import { STATE_LABEL } from "./model";
/** Task 18 replaces this preview with review tabs and instructions. */
export function AgentPreview({ id }: { id: string }) {
  const { tasks } = useAgentData();
  const listed = tasks.find((task) => task.id === id);
  const detail = useRpcQuery("agents.get", { id }, { enabled: !listed });
  const task = listed ?? detail.data;
  if (!task && detail.error) {
    const message = messageFor(detail.error.code, detail.error.detail);
    return (
      <ErrorState
        title={message.title}
        diagnosticId={detail.error.diagnosticId}
        action={<Button onClick={() => void detail.refetch()}>다시 확인</Button>}
      />
    );
  }
  if (!task) return <EmptyState title="에이전트 작업을 불러오는 중…" />;
  return (
    <article className={s.preview} aria-label="에이전트 작업 미리보기">
      <h1>{task.title}</h1>
      <p>
        {STATE_LABEL[task.state]} · {task.profile}
      </p>
      <p>{task.branch ?? task.baseBranch}</p>
      <p>{task.workdir}</p>
      {task.attention?.message && <p>{task.attention.message}</p>}
      {task.prompt && <p style={{ whiteSpace: "pre-wrap" }}>{task.prompt}</p>}
    </article>
  );
}
