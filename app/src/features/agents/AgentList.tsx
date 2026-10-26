import type { ReactNode } from "react";
import { messageFor } from "../../rpc/messages";
import { useFirstDataPaint } from "../../shell/paintTrace";
import { Button, EmptyState, ErrorState } from "../../ui";
import { useProjectData } from "../projects/ProjectData";
import { AgentRow } from "./AgentRow";
import s from "./Agents.module.css";
import type { useAgentsData } from "./hooks";
export function AgentList({
  data,
  selected,
  onSelect,
  header,
  banner,
}: {
  data: ReturnType<typeof useAgentsData>;
  selected?: string;
  onSelect: (id: string) => void;
  header: ReactNode;
  banner?: ReactNode;
}) {
  const projects = useProjectData();
  const names = new Map((projects.data?.items ?? []).map((p) => [p.id, p.name]));
  const error = data.list.error && messageFor(data.list.error.code, data.list.error.detail);
  const metricsError = data.statsError ?? data.resourcesError;
  // SC2 cold start ends when the default first screen shows its list or empty state, not at transport readiness.
  useFirstDataPaint(Boolean(data.list.data && !error));
  return (
    <div className={s.list}>
      <div className={s.header}>{header}</div>
      {banner}
      {error && (
        <ErrorState
          title={error.title}
          diagnosticId={data.list.error?.diagnosticId}
          action={
            <Button size="sm" onClick={() => void data.list.refetch()}>
              다시 확인
            </Button>
          }
        />
      )}
      {!data.list.data && !error && <p role="status">에이전트 작업을 불러오는 중…</p>}
      {metricsError && (
        <p role="status">
          {messageFor(metricsError.code, metricsError.detail).title} 자원과 변경 정보는 다시 확인됩니다.
        </p>
      )}
      {data.list.data && !error && data.groups.length === 0 && (
        <EmptyState title="에이전트 작업이 없습니다" body="새 작업으로 도구와 지시문을 선택하세요." />
      )}
      {data.groups.map((group) => (
        <section key={group.key} aria-label={group.label} className={s.group}>
          <h2>
            {group.label} · {group.items.length}
          </h2>
          {group.items.map((task) => (
            <AgentRow
              key={task.id}
              task={task}
              selected={task.id === selected}
              now={data.now}
              projectName={names.get(task.projectId) ?? "프로젝트를 찾을 수 없음"}
              stats={data.stats.get(task.id)}
              resources={data.resources.get(task.id)}
              onSelect={onSelect}
            />
          ))}
        </section>
      ))}
    </div>
  );
}
