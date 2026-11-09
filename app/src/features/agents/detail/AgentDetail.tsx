import { useParams } from "@tanstack/react-router";
import { useState } from "react";
import { useRpcQuery } from "../../../rpc/query";
import { Button, EmptyState, Tabs, useToasts } from "../../../ui";
import { useProjectData } from "../../projects/ProjectData";
import { openPopout } from "../../terminal/popout";
import { TerminalView } from "../../terminal/TerminalView";
import { useSessions } from "../../terminal/useSessions";
import { useAgentsData } from "../hooks";
import { toolLabel } from "../model";
import { AgentActions } from "./AgentActions";
import { AgentHeader } from "./AgentHeader";
import { ChangesTab } from "./ChangesTab";
import { CommitsTab } from "./CommitsTab";
import { Compose } from "./Compose";
import s from "./Detail.module.css";
import { InstructionsTab } from "./InstructionsTab";
import { QueryError } from "./QueryError";
import { RunTab, useAgentRuns } from "./RunTab";

const tabs = [
  { id: "terminal", label: "터미널" },
  { id: "changes", label: "변경" },
  { id: "commits", label: "커밋" },
  { id: "run", label: "실행" },
  { id: "instructions", label: "지시" },
];
export function AgentDetail({
  id,
  data,
  popout = false,
}: {
  id: string;
  data: ReturnType<typeof useAgentsData>;
  /** Shown in its own popout window rather than the agents page. */
  popout?: boolean;
}) {
  const detail = useRpcQuery("agents.get", { id });
  const listed = data.tasks.find((task) => task.id === id);
  const task = detail.data ?? listed;
  const projects = useProjectData();
  const sessions = useSessions(task?.projectId ?? null, { enabled: !!task });
  const [tab, setTab] = useState("terminal");
  const [focus, setFocus] = useState<number>();
  // Asked for by the buttons that point to the terminal; a tab picked by hand carries no request.
  const [terminalFocus, setTerminalFocus] = useState<number>();
  const sessionAlive =
    !!task?.terminalId && sessions.items.some((session) => session.id === task.terminalId && session.alive);
  if (!task) return detail.error ? <QueryError query={detail} /> : <EmptyState title="에이전트 작업을 불러오는 중…" />;
  const instruct = () => setFocus((previous) => (previous ?? 0) + 1);
  const showTerminal = () => {
    setTab("terminal");
    setTerminalFocus((previous) => (previous ?? 0) + 1);
  };
  const selectTab = (next: string) => {
    setTab(next);
    setTerminalFocus(undefined);
  };
  return (
    <article className={s.detail} aria-label="에이전트 작업 상세">
      <AgentHeader
        task={task}
        projectName={
          projects.data?.items.find((project) => project.id === task.projectId)?.name ?? "프로젝트를 찾을 수 없음"
        }
        toolName={toolLabel(data.profiles.data?.items, task.profile)}
        now={data.now}
        resource={data.resources.get(id)}
        behind={data.stats.get(id)?.behind}
        popout={popout}
      />
      <QueryError query={detail} />
      <QueryError query={sessions} />
      <AgentActions
        task={task}
        sessionAlive={sessionAlive}
        sessionKnown={!!sessions.data}
        onInstruct={instruct}
        onResume={showTerminal}
        onShowTerminal={showTerminal}
        onTest={(runId) => {
          useAgentRuns.getState().set(id, runId);
          setTab("run");
        }}
      />
      <Tabs
        items={tabs}
        active={tab}
        onSelect={selectTab}
        actions={
          <Button
            size="sm"
            onClick={() =>
              void openPopout("agent", id).catch(() =>
                useToasts.getState().push({ tone: "danger", title: "새 창을 열지 못했습니다" }),
              )
            }
          >
            새 창으로
          </Button>
        }
      />
      <div className={s.content} role="tabpanel" aria-label={tabs.find((item) => item.id === tab)?.label}>
        {tab === "terminal" &&
          (task.terminalId ? (
            <div className={s.terminal}>
              <TerminalView id={task.terminalId} readOnly={task.state === "closed"} focusRequest={terminalFocus} />
            </div>
          ) : (
            <EmptyState
              title="연결된 터미널이 없습니다"
              body={task.state === "closed" ? "닫힌 작업의 기록을 확인하세요." : "재개하면 터미널을 다시 엽니다."}
            />
          ))}
        {tab === "changes" && <ChangesTab agentId={id} closed={task.state === "closed"} onQuote={instruct} />}
        {tab === "commits" && <CommitsTab agentId={id} />}
        {tab === "run" && <RunTab task={task} />}
        {tab === "instructions" && <InstructionsTab task={task} />}
      </div>
      {task.state !== "closed" && (
        <Compose
          agentId={id}
          agentState={task.state}
          disabled={!sessionAlive || task.toolExited}
          focusRequest={focus}
          onShowTerminal={showTerminal}
        />
      )}
    </article>
  );
}
export function PopoutAgent() {
  const data = useAgentsData();
  const { id } = useParams({ strict: false }) as { id?: string };
  return id ? (
    <AgentDetail key={id} id={id} data={data} popout />
  ) : (
    <EmptyState title="에이전트 작업을 찾을 수 없습니다" />
  );
}
