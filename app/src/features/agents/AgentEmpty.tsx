import { isListCollapsed, useLayout } from "../../shell/layout";
import { Button, EmptyState } from "../../ui";
import { AddProjectAction } from "../projects/AddProjectAction";
import { useProjectData } from "../projects/ProjectData";
import { useAgentData } from "./AgentData";
import s from "./Agents.module.css";
import { QueryError } from "./detail/QueryError";
import { useAgentsUi } from "./ui";

/**
 * The agents body, and the grid, when it shows no task: what the situation is and its one next action, in
 * view even with the list collapsed. Nothing is decided on a list that has not arrived, so no action flashes up
 * that does not apply; a list that failed to load shows its error and [다시 확인] instead of a blank body.
 */
export function AgentEmpty({ onOpen }: { onOpen: (id: string) => void }) {
  const { list, groups } = useAgentData();
  const projects = useProjectData();
  const listHidden = useLayout(isListCollapsed);
  const setCreating = useAgentsUi((state) => state.setCreating);
  // An open list shows its own loading text and error; a collapsed one hides them, so the body shows the error.
  if (!list.data) return listHidden ? <QueryError query={list} /> : null;
  // Nothing else on this page shows a failed project list (the switcher only once opened). Blank while it loads.
  const projectsFailed = projects.data ? null : <QueryError query={projects} />;
  const open = groups.flatMap((group) => group.items).filter((task) => task.state !== "closed");
  if (!open.length) {
    // The first step depends on the project count, so none is offered until it is known.
    if (!projects.data) return projectsFailed;
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
  }
  // Picking needs only the task list. In urgency order the first is the top "입력 대기" task in the list, never
  // one that finished its turn.
  const waiting = open.filter((task) => task.state === "waiting");
  const first = waiting[0];
  return (
    <div className={s.emptyBody}>
      {projectsFailed}
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
    </div>
  );
}
