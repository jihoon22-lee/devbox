import { useEffect, useRef, useState } from "react";
import { Button, EmptyState, useToasts } from "../../ui";
import { useOptionalProjectData } from "../projects/ProjectData";
import { openPopout } from "../terminal/popout";
import { TerminalView } from "../terminal/TerminalView";
import { useAgentData, useAgentSummary } from "./AgentData";
import s from "./AgentGrid.module.css";
import { AgentStateMark } from "./AgentStateMark";
import { QueryError } from "./detail/QueryError";
import { type AgentTask, toolLabel } from "./model";

function Tile({
  task,
  projectName,
  toolName,
  onOpen,
}: {
  task: AgentTask;
  projectName: string;
  toolName: string;
  onOpen: (id: string) => void;
}) {
  const cell = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(1000);
  useEffect(() => {
    const el = cell.current;
    if (!el) return;
    const resize = new ResizeObserver(([entry]) => {
      if (entry && entry.contentRect.width > 0) setWidth(entry.contentRect.width);
    });
    const initial = el.getBoundingClientRect().width;
    if (initial > 0) setWidth(initial);
    resize.observe(el);
    return () => resize.disconnect();
  }, []);
  return (
    <div ref={cell} className={s.cell}>
      <div className={s.terminal} aria-hidden="true" inert>
        {task.terminalId ? (
          <TerminalView id={task.terminalId} readOnly scale={width / 1000} />
        ) : (
          <p>{task.state === "preparing" ? "터미널을 준비하는 중…" : "연결된 터미널이 없습니다"}</p>
        )}
      </div>
      <button
        type="button"
        className={s.cover}
        aria-label={`${task.title} 터미널 미리보기`}
        onClick={() => onOpen(task.id)}
      >
        <span className={s.caption}>
          <span className={s.heading}>
            <strong>{task.title}</strong>
            <AgentStateMark state={task.state} />
          </span>
          <span className={s.source}>
            {projectName} · {toolName}
          </span>
        </span>
      </button>
    </div>
  );
}
export function AgentGrid({ tasks, onOpen }: { tasks: AgentTask[]; onOpen: (id: string) => void }) {
  // The shell's shared project and profile lists, with the agent list's fallbacks when one is missing.
  const projects = useOptionalProjectData();
  const profiles = useAgentSummary()?.profiles.data?.items;
  const names = new Map((projects?.data?.items ?? []).map((project) => [project.id, project.name]));
  const openTasks = tasks.filter((task) => task.state !== "closed");
  const columns = openTasks.length < 5 ? 2 : openTasks.length < 10 ? 3 : 4;
  return (
    <div className={s.wrap}>
      <div className={s.toolbar}>
        <Button
          size="sm"
          onClick={() =>
            void openPopout("agents-grid").catch(() =>
              useToasts.getState().push({ tone: "danger", title: "새 창을 열지 못했습니다" }),
            )
          }
        >
          새 창으로
        </Button>
      </div>
      {!openTasks.length ? (
        <EmptyState title="에이전트 작업이 없습니다" />
      ) : (
        <section
          aria-label="에이전트 터미널 격자"
          className={s.grid}
          style={{ gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }}
        >
          {openTasks.map((task) => (
            <Tile
              key={task.id}
              task={task}
              projectName={names.get(task.projectId) ?? "프로젝트를 찾을 수 없음"}
              toolName={toolLabel(profiles, task.profile)}
              onOpen={onOpen}
            />
          ))}
        </section>
      )}
    </div>
  );
}
export function PopoutAgentGrid() {
  const data = useAgentData();
  if (data.list.error) return <QueryError query={data.list} />;
  if (!data.list.data) return <p role="status">에이전트 작업을 불러오는 중…</p>;
  return (
    <AgentGrid
      tasks={data.groups.flatMap((group) => group.items)}
      onOpen={(id) =>
        void openPopout("agent", id).catch(() =>
          useToasts.getState().push({ tone: "danger", title: "새 창을 열지 못했습니다" }),
        )
      }
    />
  );
}
