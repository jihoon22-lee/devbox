import type { ContainerInfo, ContainersResult } from "../../rpc/gen/rpc";
import { Button, StatusDot } from "../../ui";
import { DataTable } from "../../ui/DataTable";
import type { RunSelection } from "./RunList";
import s from "./RunsPage.module.css";

type RowProps = {
  selected?: RunSelection;
  busy: boolean;
  onSelect: (selection: RunSelection) => void;
  onAction: (action: "start" | "stop" | "restart", container: ContainerInfo) => void;
};
/** Only the current project's containers are listed open; the rest stay reachable in a folded group. */
export function ContainerList({
  result,
  projectId,
  ...row
}: { result?: ContainersResult; projectId: string } & RowProps) {
  if (!result?.available) return null;
  const primary = result.projectsUnavailable
    ? result.items
    : result.items.filter((item) => item.projectId === projectId);
  const others = result.projectsUnavailable ? [] : result.items.filter((item) => item.projectId !== projectId);
  return (
    <section aria-label="컨테이너">
      <h3>
        컨테이너 <small>{primary.length}</small>
      </h3>
      {result.projectsUnavailable && <p>프로젝트 목록을 읽지 못해 컨테이너를 모두 함께 표시합니다.</p>}
      {primary.length === 0 ? (
        <p className={s.muted}>{others.length ? "현재 프로젝트의 컨테이너 없음" : "컨테이너 없음"}</p>
      ) : (
        <ContainerTable label="컨테이너 목록" rows={primary} {...row} />
      )}
      {others.length > 0 && (
        <details className={s.otherContainers}>
          <summary>다른 컨테이너 {others.length}개</summary>
          <ContainerTable label="다른 컨테이너 목록" rows={others} {...row} />
        </details>
      )}
    </section>
  );
}
function ContainerTable({
  label,
  rows,
  selected,
  busy,
  onSelect,
  onAction,
}: { label: string; rows: ContainerInfo[] } & RowProps) {
  return (
    <DataTable
      label={label}
      rows={rows}
      rowKey={(row) => row.id}
      columns={[
        {
          key: "name",
          title: "이름 · 상태",
          render: (row) => (
            <div className={s.rowSummary}>
              <Button
                variant="ghost"
                size="sm"
                aria-label={`${row.name} 로그`}
                aria-pressed={selected && "containerId" in selected && selected.containerId === row.id}
                onClick={() => onSelect({ containerId: row.id })}
              >
                <StatusDot tone={row.state === "running" ? "ok" : "idle"} label={row.state} />
                {row.name}
              </Button>
              <span>
                {row.status} · {row.image}
              </span>
            </div>
          ),
        },
        {
          key: "actions",
          title: "동작",
          render: (row) => (
            <div className={s.actions}>
              <Button
                size="sm"
                disabled={busy || row.state === "running"}
                onClick={() => onAction("start", row)}
                aria-label={`${row.name} 시작`}
              >
                시작
              </Button>
              <Button
                size="sm"
                disabled={busy || row.state !== "running"}
                onClick={() => onAction("stop", row)}
                aria-label={`${row.name} 중지`}
              >
                중지
              </Button>
              <Button
                size="sm"
                disabled={busy}
                onClick={() => onAction("restart", row)}
                aria-label={`${row.name} 재시작`}
              >
                재시작
              </Button>
            </div>
          ),
        },
      ]}
    />
  );
}
