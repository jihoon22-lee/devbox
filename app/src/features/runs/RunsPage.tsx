import { useRouterState, useSearch } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import type { Definition, LogSource, Suggestion } from "../../rpc/gen/rpc";
import { useRpcQuery, useTopicInvalidation } from "../../rpc/query";
import { useRpc } from "../../rpc/RpcProvider";
import { openFileLocation } from "../../shell/fileLocation";
import { SectionLayout } from "../../shell/SectionLayout";
import { Banner, Button, EmptyState, Menu } from "../../ui";
import { ProjectError } from "../projects/ProjectError";
import { useCurrentProject } from "../projects/store";
import { visibleTrustText } from "../projects/visibleTrustText";
import { DefineDialog } from "./DefineDialog";
import { LogView } from "./LogView";
import { ObservationNavigation } from "./ObservationNavigation";
import { usePinnedLogSources } from "./pinnedSources";
import { RunChips } from "./RunChips";
import { RunList, type RunSelection } from "./RunList";
import s from "./RunsPage.module.css";
import { resultLabel, runStateLabel } from "./runPresentation";
import { SystemOverview } from "./SystemOverview";
import { type RunsSearch, runsSearch } from "./search";
import { useRunActions } from "./useRunActions";
export function RunsPage() {
  const search = runsSearch(useSearch({ strict: false }));
  const currentProject = useCurrentProject((state) => state.id);
  const navigationKey = useRouterState({
    select: (state) => `${state.location.state.__TSR_key}:${state.location.state.runActivation ?? ""}`,
  });
  const [appliedNavigation, setAppliedNavigation] = useState<string>();
  const projectId = appliedNavigation === navigationKey ? currentProject : (search.projectId ?? currentProject);
  useEffect(() => {
    if (search.projectId) useCurrentProject.getState().set(search.projectId);
    setAppliedNavigation(navigationKey);
  }, [search.projectId, navigationKey]);
  if (search.view)
    return <SystemOverview key={`${search.view}:${projectId}`} view={search.view} projectId={projectId} />;
  return projectId ? (
    <ProjectRuns
      key={`${projectId}:${navigationKey}`}
      projectId={projectId}
      target={search.projectId === projectId ? search : undefined}
    />
  ) : (
    <>
      <ObservationNavigation />
      <EmptyState title="프로젝트를 선택하세요" body="실행은 현재 프로젝트의 작업·서비스·예약을 보여 줍니다." />
    </>
  );
}
function targetSelection(target?: RunsSearch): RunSelection | undefined {
  if (target?.runId) return { definitionId: "", runId: target.runId };
  return target?.task ? { definitionId: target.task } : undefined;
}
export function ProjectRuns({ projectId, target }: { projectId: string; target?: RunsSearch }) {
  const client = useRpc();
  const list = useRpcQuery("runtime.list", { projectId, cursor: null });
  const containers = useRpcQuery("runtime.containers", {}, { refetchInterval: 5000 });
  const schedules = useRpcQuery("runtime.schedules", { projectId }, { refetchInterval: 10000 });
  const [selection, setSelection] = useState<RunSelection | undefined>(() => targetSelection(target));
  const targetRunId = target?.runId;
  const targetTask = target?.task;
  useEffect(() => {
    setSelection(targetSelection({ runId: targetRunId, task: targetTask }));
  }, [targetRunId, targetTask]);
  const [editor, setEditor] = useState<{ definition?: Definition; suggestion?: Suggestion }>();
  const [detect, setDetect] = useState(false);
  const [externalEditor, setExternalEditor] = useState<"vscode" | "cursor">("vscode");
  const suggestions = useRpcQuery("runtime.detect", { projectId }, { enabled: detect });
  const definition =
    selection && "definitionId" in selection
      ? list.data?.defs.find((row) => row.def.id === selection.definitionId)
      : undefined;
  const runId =
    selection && "definitionId" in selection
      ? (selection.runId ?? list.data?.runs.find((run) => run.defId === selection.definitionId)?.id)
      : undefined;
  const detail = useRpcQuery("runtime.get", { runId: runId ?? "" }, { enabled: !!runId });
  useTopicInvalidation("runtime.changed", [
    ["runtime.list", { projectId, cursor: null }],
    ["runtime.get"],
    ["runtime.schedules", { projectId }],
  ]);
  const actions = useRunActions(projectId, (run) => setSelection({ definitionId: run.defId, runId: run.id }));
  useEffect(() => {
    if (!selection && list.data?.defs[0]) setSelection({ definitionId: list.data.defs[0].def.id });
  }, [selection, list.data]);
  const run = detail.data?.run;
  useEffect(() => {
    if (run && selection && "definitionId" in selection && !selection.definitionId && selection.runId === run.id)
      setSelection({ definitionId: run.defId, runId: run.id });
  }, [run, selection]);
  const container =
    selection && "containerId" in selection
      ? containers.data?.items.find((row) => row.id === selection.containerId)
      : undefined;
  const sources: LogSource[] = runId
    ? [{ kind: "run", runId }]
    : container
      ? [{ kind: "container", id: container.id }]
      : [];
  return (
    <>
      <SectionLayout
        list={
          <div className={s.list}>
            <header className={s.toolbar}>
              <h2>실행</h2>
              <Menu
                trigger={<Button size="sm">작업 추가 ▾</Button>}
                items={[
                  { label: "직접 추가", onSelect: () => setEditor({}), afterClose: true },
                  { label: "자동 감지 제안", onSelect: () => setDetect(true), afterClose: true },
                ]}
              />
            </header>
            <ObservationNavigation />
            {list.isError && (
              <Banner
                tone="danger"
                actions={
                  <Button size="sm" onClick={() => void list.refetch()}>
                    다시 불러오기
                  </Button>
                }
              >
                실행 목록을 불러오지 못했습니다. 저장된 정의는 유지됩니다.
              </Banner>
            )}
            {containers.isError && (
              <Banner
                tone="warn"
                actions={
                  <Button size="sm" onClick={() => void containers.refetch()}>
                    다시 불러오기
                  </Button>
                }
              >
                컨테이너 목록을 불러오지 못했습니다.
              </Banner>
            )}
            {containers.data && !containers.data.available && (
              <Banner tone="info">
                {containers.data.reason === "cliMissing"
                  ? "Docker CLI가 설치되어 있지 않습니다."
                  : "Docker 데몬을 사용할 수 없습니다."}
              </Banner>
            )}
            {schedules.isError && (
              <Banner
                tone="warn"
                actions={
                  <Button size="sm" onClick={() => void schedules.refetch()}>
                    예약 다시 불러오기
                  </Button>
                }
              >
                예약 상태를 불러오지 못했습니다.
              </Banner>
            )}
            {detect && (
              <section className={s.suggestions} aria-label="자동 감지 제안">
                <div className={s.toolbar}>
                  <h3>자동 감지 제안</h3>
                  <Button size="sm" onClick={() => setDetect(false)}>
                    닫기
                  </Button>
                </div>
                {suggestions.isError && (
                  <ProjectError error={suggestions.error} retry={() => void suggestions.refetch()} />
                )}
                {suggestions.isPending && <p role="status">제안 확인 중…</p>}
                {suggestions.data?.length === 0 && <p>추가할 제안이 없습니다.</p>}
                {suggestions.data?.map((item) => (
                  <div className={s.suggestion} key={`${item.from}:${item.id}`}>
                    <strong>{item.id}</strong>
                    <span>{item.from}</span>
                    <code>{visibleTrustText(item.command)}</code>
                    <Button size="sm" onClick={() => setEditor({ suggestion: item })}>
                      명령 확인 후 추가
                    </Button>
                  </div>
                ))}
              </section>
            )}
            {!list.data && !list.isError ? (
              <p role="status">실행 불러오는 중…</p>
            ) : (
              <RunList
                definitions={list.data?.defs ?? []}
                runs={list.data?.runs ?? []}
                containers={containers.data?.items ?? []}
                schedules={schedules.data?.items ?? []}
                trust={list.data?.trust ?? "not_required"}
                selected={selection}
                busy={actions.busy}
                onSelect={setSelection}
                onAction={actions.action}
                onEdit={(definition) => setEditor({ definition })}
                onContainerAction={actions.containerAction}
              />
            )}
          </div>
        }
      >
        <div className={s.page}>
          {actions.feedback}
          <header className={s.detailHeader}>
            <h2>{definition?.def.id ?? run?.defId ?? container?.name ?? "실행 로그"}</h2>
            {run && (
              <>
                <div className={s.metadata}>
                  <span>상태 {runStateLabel[run.state]}</span>
                  <span>
                    실행 ID <code>{run.id}</code>
                  </span>
                  <span>PID {run.pid ?? "—"}</span>
                  <span>재시작 {run.restarts}회</span>
                  <span>{resultLabel(run)}</span>
                </div>
                <RunChips key={run.id} run={run} />
                {run.windows && (
                  <Banner tone="info">
                    {run.windows.notice}
                    <br />
                    실행 폴더: {run.windows.directory}
                  </Banner>
                )}
              </>
            )}
            {definition?.def.kind === "schedule" && (
              <p>
                예약 {definition.def.cron} · {definition.def.tz ?? "현지 시간"} · 작업 {definition.def.task}
              </p>
            )}
            <label className={s.editorChoice}>
              파일 위치 열기{" "}
              <select
                value={externalEditor}
                onChange={(event) => setExternalEditor(event.target.value as "vscode" | "cursor")}
              >
                <option value="vscode">VS Code</option>
                <option value="cursor">Cursor</option>
              </select>
            </label>
          </header>
          {detail.isError && <ProjectError error={detail.error} retry={() => void detail.refetch()} />}
          {sources.length ? (
            <LogView
              key={runId ?? container?.id}
              sources={sources}
              projectId={projectId}
              onPinSources={(sources) => usePinnedLogSources.getState().set(projectId, sources)}
              onOpenLocation={(location) => openFileLocation(client, projectId, location, externalEditor)}
            />
          ) : (
            <EmptyState title="실행할 작업을 선택하세요" body="작업을 시작하면 로그와 종료 결과가 여기에 나타납니다." />
          )}
        </div>
      </SectionLayout>
      {editor && (
        <DefineDialog
          key={editor.definition?.def.id ?? editor.suggestion?.id ?? "new"}
          projectId={projectId}
          {...editor}
          onClose={() => setEditor(undefined)}
        />
      )}
    </>
  );
}
