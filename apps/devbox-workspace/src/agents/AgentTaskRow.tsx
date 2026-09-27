import type { AgentTask } from "@devbox/workspace-features/generated/AgentTask";
export type RowAction = "resume" | "focus" | "reopen" | "review" | "merge" | "cleanup" | "discard" | "forget";
export type Confirmation =
  | { taskId: string; kind: "merge"; branch: string }
  | { taskId: string; kind: "discard" | "cleanup" }
  | { taskId: string; kind: "conflicts"; paths: string[] };
const labels: Record<AgentTask["state"], string> = {
  planned: "준비 중",
  created: "작업 폴더 생성됨",
  ready: "터미널 준비",
  running: "실행 중",
  merged: "병합됨",
  discarded: "버림",
};
export default function AgentTaskRow({
  task,
  stopped,
  busy,
  confirmation,
  act,
  confirm,
  cancel,
}: {
  task: AgentTask;
  stopped: boolean;
  busy: boolean;
  confirmation: Confirmation | null;
  act(action: RowAction): void;
  confirm(): void;
  cancel(): void;
}) {
  const done = task.state === "merged" || task.state === "discarded";
  return (
    <li>
      <article aria-label={task.title}>
        <h3>{task.title}</h3>
        <p>{task.state === "running" && stopped ? "터미널 종료됨" : labels[task.state]}</p>
        <p>{task.branch}</p>
        {!done && (
          <div>
            {["planned", "created", "ready"].includes(task.state) && (
              <button disabled={busy} onClick={() => act("resume")}>
                다시 시도
              </button>
            )}
            {task.state === "running" &&
              (stopped ? (
                <button disabled={busy} onClick={() => act("reopen")}>
                  터미널 다시 열기
                </button>
              ) : (
                <button disabled={busy} onClick={() => act("focus")}>
                  터미널 보기
                </button>
              ))}
            {task.worktreeId && (
              <>
                <button disabled={busy} onClick={() => act("review")}>
                  변경 검토
                </button>
                <button disabled={busy} onClick={() => act("merge")}>
                  병합
                </button>
              </>
            )}
            {task.state !== "planned" && (
              <button disabled={busy} onClick={() => act("discard")}>
                버리기
              </button>
            )}
          </div>
        )}
        {(done || task.state === "planned") && (
          <button disabled={busy} onClick={() => act("forget")}>
            목록에서 지우기
          </button>
        )}
        {confirmation?.taskId === task.id && (
          <div>
            {confirmation.kind === "merge" && (
              <p>{`'${task.branch}'을 기본 작업 폴더의 현재 branch '${confirmation.branch}'에 병합합니다.`}</p>
            )}
            {confirmation.kind === "discard" && <p>작업 폴더와 branch를 지웁니다. 커밋하지 않은 변경도 사라집니다.</p>}
            {confirmation.kind === "cleanup" && <p>병합했습니다. 작업 폴더를 정리할까요?</p>}
            {confirmation.kind === "conflicts" ? (
              <>
                <p role="status">충돌 파일 {confirmation.paths.length}개</p>
                <ul>
                  {confirmation.paths.map((path) => (
                    <li key={path}>{path}</li>
                  ))}
                </ul>
                <p>병합을 되돌렸습니다. 변경 검토에서 충돌을 해결한 뒤 다시 병합해 주세요.</p>
              </>
            ) : (
              <button disabled={busy} onClick={confirm}>
                {confirmation.kind === "merge" ? "병합 확인" : confirmation.kind === "discard" ? "버리기 확인" : "정리"}
              </button>
            )}
            <button disabled={busy} onClick={cancel}>
              닫기
            </button>
          </div>
        )}
      </article>
    </li>
  );
}
