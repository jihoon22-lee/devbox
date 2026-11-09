import { Button, EmptyState } from "../../ui";
import { AddProjectAction } from "../projects/AddProjectAction";
import { useProjectData } from "../projects/ProjectData";
import { useAgentData } from "./AgentData";
import { useAgentsUi } from "./ui";

/**
 * The agents body, and the grid, when it shows no task: what the situation is and its one next action, in
 * view even with the list collapsed. Nothing is decided until both the project list and the task list have
 * arrived: until then it stays blank, and a failed load keeps its existing error display (the list's for tasks).
 */
export function AgentEmpty({ onOpen }: { onOpen: (id: string) => void }) {
  const { list, groups } = useAgentData();
  const projects = useProjectData();
  const setCreating = useAgentsUi((state) => state.setCreating);
  if (!list.data || !projects.data) return null;
  const open = groups.flatMap((group) => group.items).filter((task) => task.state !== "closed");
  if (!open.length)
    return projects.data.items.length === 0 ? (
      <EmptyState
        title="등록된 프로젝트가 없습니다"
        body="프로젝트를 추가하면 에이전트 작업을 시작할 수 있습니다."
        action={<AddProjectAction />}
      />
    ) : (
      <EmptyState
        title="열린 에이전트 작업이 없습니다"
        body="새 작업 창에서 도구와 지시문을 정합니다. Ctrl+Shift+N으로도 엽니다."
        action={
          <Button variant="primary" onClick={() => setCreating(true)}>
            새 작업
          </Button>
        }
      />
    );
  // Urgency order: the first is the task that has waited longest for an answer, never one that finished its turn.
  const waiting = open.filter((task) => task.state === "waiting");
  const first = waiting[0];
  return (
    <EmptyState
      title="에이전트 작업을 고르세요"
      body={first ? `입력 대기 작업이 ${waiting.length}개 있습니다.` : "입력을 기다리는 작업이 위에 모입니다."}
      action={
        first && (
          <Button variant="primary" onClick={() => onOpen(first.id)}>
            첫 입력 대기 작업 열기
          </Button>
        )
      }
    />
  );
}
