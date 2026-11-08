import type { ContainerInfo, Definition, Run, ScheduleStatus, TrustPreviewStatus } from "../../rpc/gen/rpc";
import { Button, StatusDot } from "../../ui";
import { DataTable } from "../../ui/DataTable";
import s from "./RunsPage.module.css";
import { liveRun, resultLabel, runStateLabel, runTone, scheduleResultLabel } from "./runPresentation";
export type RunSelection = { definitionId: string; runId?: string } | { containerId: string };
export function definitionNeedsTrust(definition: Definition, definitions: Definition[], status: TrustPreviewStatus) {
  if (status !== "approval_required") return false;
  const visited = new Set<string>();
  const check = (row: Definition): boolean => {
    if (visited.has(row.def.id)) return false;
    visited.add(row.def.id);
    if (row.source === "projectFile") return true;
    const ids = row.def.kind === "schedule" ? [row.def.task] : row.def.dependsOn;
    return ids.some((id) => {
      const dependency = definitions.find((d) => d.def.id === id);
      return dependency ? check(dependency) : false;
    });
  };
  return check(definition);
}
export function RunList({
  definitions,
  runs,
  containers,
  schedules,
  trust,
  selected,
  busy,
  onSelect,
  onAction,
  onStopOrphan,
  onEdit,
  onContainerAction,
}: {
  definitions: Definition[];
  runs: Run[];
  containers: ContainerInfo[];
  schedules: ScheduleStatus[];
  trust: TrustPreviewStatus;
  selected?: RunSelection;
  busy: boolean;
  onSelect: (selection: RunSelection) => void;
  onAction: (action: "start" | "stop" | "restart", definition: Definition, run?: Run) => void;
  onStopOrphan: (run: Run) => void;
  onEdit: (definition: Definition) => void;
  onContainerAction: (action: "start" | "stop" | "restart", container: ContainerInfo) => void;
}) {
  // The daemon adds live services whose definition is gone; every other run has its definition row.
  const orphans = runs.filter((run) => !definitions.some((row) => row.def.id === run.defId));
  return (
    <div className={s.listGroups}>
      {(
        [
          ["service", "서비스"],
          ["task", "작업"],
          ["schedule", "예약"],
        ] as const
      ).map(([kind, label]) => {
        const rows = definitions.filter((row) => row.def.kind === kind);
        return (
          <section key={kind} aria-label={label}>
            <h3>
              {label} <small>{rows.length}</small>
            </h3>
            {rows.length === 0 ? (
              <p className={s.muted}>등록된 {label} 없음</p>
            ) : (
              <DataTable
                label={`${label} 목록`}
                rows={rows}
                rowKey={(row) => row.def.id}
                columns={[
                  {
                    key: "name",
                    title: "이름 · 상태",
                    render: (row) => {
                      const run = runs.find((run) => run.defId === row.def.id);
                      const schedule = schedules.find((item) => item.id === row.def.id);
                      return (
                        <div className={s.rowSummary}>
                          <Button
                            variant="ghost"
                            size="sm"
                            aria-label={`${row.def.id} 로그`}
                            aria-pressed={
                              selected && "definitionId" in selected && selected.definitionId === row.def.id
                            }
                            onClick={() => onSelect({ definitionId: row.def.id, runId: run?.id })}
                          >
                            <StatusDot tone={runTone(run)} label={run ? runStateLabel[run.state] : "대기"} />
                            {row.def.id}
                          </Button>
                          <span>
                            {row.source === "projectFile" ? "프로젝트 파일" : "이 PC"} · {resultLabel(run)} · 재시작{" "}
                            {run?.restarts ?? 0}회
                          </span>
                          {schedule && (
                            <span>
                              다음 {schedule.next ?? "없음"} · 최근{" "}
                              {schedule.last ? scheduleResultLabel[schedule.last.result] : "없음"}
                            </span>
                          )}
                          {definitionNeedsTrust(row, definitions, trust) && (
                            <span className={s.warning}>설정이 바뀌어 다음 시작에 재승인 필요</span>
                          )}
                        </div>
                      );
                    },
                  },
                  {
                    key: "actions",
                    title: "동작",
                    render: (row) => {
                      const run = runs.find((run) => run.defId === row.def.id);
                      return (
                        <div className={s.actions}>
                          <Button
                            size="sm"
                            disabled={busy || liveRun(run)}
                            onClick={() => onAction("start", row, run)}
                            aria-label={`${row.def.id} 시작`}
                          >
                            시작
                          </Button>
                          <Button
                            size="sm"
                            disabled={busy || !liveRun(run)}
                            onClick={() => onAction("stop", row, run)}
                            aria-label={`${row.def.id} 중지`}
                          >
                            중지
                          </Button>
                          <Button
                            size="sm"
                            disabled={busy || !run}
                            onClick={() => onAction("restart", row, run)}
                            aria-label={`${row.def.id} 재시작`}
                          >
                            재시작
                          </Button>
                          <Button
                            size="sm"
                            variant="ghost"
                            disabled={busy}
                            onClick={() => onEdit(row)}
                            aria-label={`${row.def.id} 수정`}
                          >
                            수정
                          </Button>
                        </div>
                      );
                    },
                  },
                ]}
              />
            )}
            {kind === "service" && (
              <OrphanRuns runs={orphans} selected={selected} busy={busy} onSelect={onSelect} onStop={onStopOrphan} />
            )}
          </section>
        );
      })}
      <section aria-label="컨테이너">
        <h3>
          컨테이너 <small>{containers.length}</small>
        </h3>
        {containers.length === 0 ? (
          <p className={s.muted}>컨테이너 없음</p>
        ) : (
          <DataTable
            label="컨테이너 목록"
            rows={containers}
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
                      onClick={() => onContainerAction("start", row)}
                      aria-label={`${row.name} 시작`}
                    >
                      시작
                    </Button>
                    <Button
                      size="sm"
                      disabled={busy || row.state !== "running"}
                      onClick={() => onContainerAction("stop", row)}
                      aria-label={`${row.name} 중지`}
                    >
                      중지
                    </Button>
                    <Button
                      size="sm"
                      disabled={busy}
                      onClick={() => onContainerAction("restart", row)}
                      aria-label={`${row.name} 재시작`}
                    >
                      재시작
                    </Button>
                  </div>
                ),
              },
            ]}
          />
        )}
      </section>
    </div>
  );
}
/** Without a definition a service can no longer start, so only its log and stop remain. */
function OrphanRuns({
  runs,
  selected,
  busy,
  onSelect,
  onStop,
}: {
  runs: Run[];
  selected?: RunSelection;
  busy: boolean;
  onSelect: (selection: RunSelection) => void;
  onStop: (run: Run) => void;
}) {
  if (runs.length === 0) return null;
  return (
    <section aria-label="정의 없는 실행">
      <h3>
        정의 없는 실행 <small>{runs.length}</small>
      </h3>
      <DataTable
        label="정의 없는 실행 목록"
        rows={runs}
        rowKey={(run) => run.id}
        columns={[
          {
            key: "name",
            title: "이름 · 상태",
            render: (run) => (
              <div className={s.rowSummary}>
                <Button
                  variant="ghost"
                  size="sm"
                  aria-label={`${run.defId} 로그`}
                  aria-pressed={selected !== undefined && "definitionId" in selected && selected.runId === run.id}
                  onClick={() => onSelect({ definitionId: run.defId, runId: run.id })}
                >
                  <StatusDot tone={runTone(run)} label={runStateLabel[run.state]} />
                  {run.defId}
                </Button>
                <span>
                  정의 없음 · {resultLabel(run)} · 재시작 {run.restarts}회
                </span>
              </div>
            ),
          },
          {
            key: "actions",
            title: "동작",
            render: (run) => (
              <div className={s.actions}>
                <Button
                  size="sm"
                  disabled={busy || !liveRun(run)}
                  onClick={() => onStop(run)}
                  aria-label={`${run.defId} 중지`}
                >
                  중지
                </Button>
              </div>
            ),
          },
        ]}
      />
    </section>
  );
}
