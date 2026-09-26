import type { WorkspaceDiagnosticState } from "../lib/taskViewTypes";
import {
  targetLabel,
  scheduleLabel,
  shortRevision,
  workspaceSourceLabel,
  canUseWorkspaceTask,
  workspaceTaskGateHint,
  isWorkspaceOperationTerminal,
  workspaceOperationStatusLabel,
  workspaceOperationRunStatusLabel,
  workspaceOperationProgressLabel,
  isWorkspaceOperationRunTerminal,
} from "../lib/taskPresentation";
import { isKeyboardActivation } from "@devbox/a11y";
import { friendlyErrorMessage } from "../api";
import type * as React from "react";

interface Props {
  openCreate: () => void;
  importTriggerRef: React.RefObject<HTMLButtonElement | null>;
  setImportOpen: React.Dispatch<React.SetStateAction<boolean>>;
  loading: boolean;
  jobs: import("../types").Job[];
  status: import("../types").RuntimeStatus | null;
  workspaceTaskByJobId: Map<string, import("../types").WorkspaceTaskState>;
  workspaceOperationByRootJobId: Map<string, import("../types").WorkspaceTaskOperation>;
  workspaceSnapshotFresh: boolean;
  selectedJobId: string | null;
  setSelectedJobId: React.Dispatch<React.SetStateAction<string | null>>;
  jobContextTrigger: import("../../../../context-menu/src/useContextMenu").ContextMenuTriggerProps;
  activeRuns: Record<string, import("../types").Run | null>;
  workspaceDiagnostics: Record<string, WorkspaceDiagnosticState>;
  retryWorkspaceTaskDiagnostics: (runId: string) => void;
  handleOpenWorkspaceTaskDiagnostic: (runId: string, diagnosticIndex: number) => Promise<void>;
  busy: boolean;
  handleRunNow: (job: import("../types").Job) => Promise<void>;
  activeSnapshotFresh: boolean;
  handleStopRun: (job: import("../types").Job) => Promise<void>;
  handleTrustWorkspaceTask: (task: import("../types").WorkspaceTaskState) => Promise<void>;
  openShellTrustConfirmation: (task: import("../types").WorkspaceTaskState) => void;
  openEdit: (job: import("../types").Job) => void;
  handleDelete: (job: import("../types").Job) => Promise<void>;
}

export function JobSection({
  openCreate,
  importTriggerRef,
  setImportOpen,
  loading,
  jobs,
  status,
  workspaceTaskByJobId,
  workspaceOperationByRootJobId,
  workspaceSnapshotFresh,
  selectedJobId,
  setSelectedJobId,
  jobContextTrigger,
  activeRuns,
  workspaceDiagnostics,
  retryWorkspaceTaskDiagnostics,
  handleOpenWorkspaceTaskDiagnostic,
  busy,
  handleRunNow,
  activeSnapshotFresh,
  handleStopRun,
  handleTrustWorkspaceTask,
  openShellTrustConfirmation,
  openEdit,
  handleDelete,
}: Props) {
  return (
    <section className="jobs-section" aria-labelledby="jobs-title">
      <div className="section-toolbar">
        <div>
          <p className="subtitle">예약된 작업을 활성화하고 실행 정책을 관리합니다.</p>
          <h3 id="jobs-title" className="visually-hidden">
            작업 목록
          </h3>
        </div>
        <button type="button" className="button-primary" onClick={openCreate}>
          + 새 작업
        </button>
        <button ref={importTriggerRef} type="button" className="button-secondary" onClick={() => setImportOpen(true)}>
          정의와 task 가져오기
        </button>
      </div>
      {loading ? (
        <div className="empty-card compact">
          <div className="pulse" />
          <p>작업을 불러오는 중…</p>
        </div>
      ) : null}
      {!loading && jobs.length === 0 ? (
        <section className="empty-card" aria-labelledby="empty-title">
          <div className="pulse" aria-hidden="true" />
          <h3 id="empty-title">실행할 작업이 아직 없습니다</h3>
          <p>명령과 cron 일정을 정의하면 로컬 스케줄러가 다음 실행 시각을 미리 보여줍니다.</p>
          <button type="button" className="button-primary" onClick={openCreate}>
            첫 작업 만들기
          </button>
          <dl>
            <div>
              <dt>시작 방식</dt>
              <dd>{status?.backgroundLaunch ? "백그라운드" : "일반"}</dd>
            </div>
            <div>
              <dt>데이터베이스</dt>
              <dd title={status?.databasePath}>{status?.databasePath ?? "준비 중"}</dd>
            </div>
          </dl>
        </section>
      ) : null}
      {!loading && jobs.length > 0 ? (
        <div className="job-list">
          {jobs.map((job) => {
            const workspaceTask = workspaceTaskByJobId.get(job.id);
            const workspaceOperation = workspaceOperationByRootJobId.get(job.id);
            const workspaceOperationActive =
              workspaceOperation !== undefined && !isWorkspaceOperationTerminal(workspaceOperation.status);
            const workspaceRunnable = canUseWorkspaceTask(workspaceTask, workspaceSnapshotFresh);
            const operationChildProgress = workspaceOperation?.runs
              .map((run) => {
                const childJob = jobs.find((candidate) => candidate.id === run.jobId);
                return `${childJob?.name ?? run.jobId}: ${workspaceOperationRunStatusLabel(run.status)}`;
              })
              .join(" · ");
            const diagnosticRuns =
              workspaceOperation?.runs.filter((run) => {
                const task = workspaceTaskByJobId.get(run.jobId);
                return (
                  Boolean(run.runId) && isWorkspaceOperationRunTerminal(run.status) && task?.hasProblemMatcher === true
                );
              }) ?? [];
            return (
              <article
                className={`job-card ${selectedJobId === job.id ? "selected" : ""}`}
                key={job.id}
                tabIndex={0}
                aria-current={selectedJobId === job.id ? "true" : undefined}
                data-job-id={job.id}
                onClick={() => setSelectedJobId(job.id)}
                onContextMenu={jobContextTrigger.onContextMenu}
                onKeyDown={(event) => {
                  jobContextTrigger.onKeyDown?.(event);
                  if (event.defaultPrevented || event.target !== event.currentTarget || !isKeyboardActivation(event))
                    return;
                  event.preventDefault();
                  setSelectedJobId(job.id);
                }}
              >
                <div className="job-card-main">
                  <div className="job-title-row">
                    <h3>{job.name}</h3>
                    <span
                      className={`job-state ${workspaceOperationActive || activeRuns[job.id] ? "running" : job.enabled ? "enabled" : "disabled"}`}
                    >
                      {workspaceOperationActive
                        ? `오케스트레이션 ${workspaceOperationStatusLabel(workspaceOperation!.status)}`
                        : activeRuns[job.id]
                          ? "실행 중"
                          : job.enabled
                            ? "활성"
                            : "비활성"}
                    </span>
                  </div>
                  <code title={job.command}>{job.command}</code>
                  <div className="job-meta">
                    <span>{targetLabel(job)}</span>
                    <span>{scheduleLabel(job)}</span>
                    <span>
                      {job.overlapPolicy === "skip"
                        ? "중복 건너뛰기"
                        : job.overlapPolicy === "queue"
                          ? "대기열"
                          : "이전 종료"}
                    </span>
                    {job.envConfigured ? <span className="secret-badge">환경변수 보호됨</span> : null}
                    {workspaceTask ? (
                      <>
                        <span className="workspace-task-badge">VS Code {workspaceTask.taskKind}</span>
                        <span title={workspaceTask.sourceRoot}>
                          소스 {workspaceSourceLabel(workspaceTask.sourceRoot)} · rev{" "}
                          {shortRevision(workspaceTask.revision)}
                        </span>
                        <span className={workspaceTask.trusted ? "workspace-task-trusted" : "workspace-task-untrusted"}>
                          {workspaceTask.trusted ? "소스 승인됨" : "소스 승인 필요"}
                        </span>
                        {workspaceTask.taskKind === "shell" ? (
                          <span
                            className={
                              workspaceTask.shellTrusted ? "workspace-task-trusted" : "workspace-task-untrusted"
                            }
                          >
                            {workspaceTask.shellTrusted ? "셸 실행 승인됨" : "셸 실행 승인 필요"}
                          </span>
                        ) : null}
                        <span
                          className={workspaceTask.available ? "workspace-task-trusted" : "workspace-task-unavailable"}
                        >
                          {workspaceTask.available ? "원본 사용 가능" : "원본 변경됨 · 사용 불가"}
                        </span>
                        {workspaceTask.dependsOn.length > 0 ? (
                          <span>
                            선행 task: {workspaceTask.dependsOn.join(", ")} ·{" "}
                            {workspaceTask.dependsOrder === "sequence" ? "순차" : "병렬"}
                          </span>
                        ) : null}
                        {workspaceTask.hasProblemMatcher ? <span>problem matcher 지원됨</span> : null}
                        {workspaceTask.environmentKeys.length > 0 ? (
                          <span>환경 키: {workspaceTask.environmentKeys.join(", ")}</span>
                        ) : null}
                      </>
                    ) : null}
                    {workspaceOperation ? (
                      <>
                        <span
                          className={`workspace-operation-badge workspace-operation-${workspaceOperation.status}`}
                          aria-live="polite"
                          aria-label={`workspace task operation 상태: ${workspaceOperationStatusLabel(workspaceOperation.status)}`}
                        >
                          오케스트레이션 {workspaceOperationStatusLabel(workspaceOperation.status)} ·{" "}
                          {workspaceOperationProgressLabel(workspaceOperation)}
                        </span>
                        {operationChildProgress ? (
                          <span title={operationChildProgress}>하위 작업 진행: {operationChildProgress}</span>
                        ) : null}
                        {workspaceOperation.failureCode ? (
                          <span className="workspace-task-unavailable">
                            {friendlyErrorMessage(workspaceOperation.failureCode)}
                          </span>
                        ) : null}
                      </>
                    ) : null}
                  </div>
                  {diagnosticRuns.length > 0 ? (
                    <div className="workspace-diagnostics" aria-label={`${job.name} 진단`}>
                      <strong>problem matcher 진단</strong>
                      {diagnosticRuns.map((run) => {
                        const runId = run.runId!;
                        const state = workspaceDiagnostics[runId];
                        const childJob = jobs.find((candidate) => candidate.id === run.jobId);
                        return (
                          <div className="workspace-diagnostic-run" key={runId}>
                            <span className="workspace-diagnostic-run-label">{childJob?.name ?? run.jobId}</span>
                            {state?.status === "loading" || !state ? <span>진단을 불러오는 중…</span> : null}
                            {state?.status === "error" ? (
                              <>
                                <span className="workspace-task-unavailable">{state.error}</span>
                                <button
                                  type="button"
                                  className="button-secondary small"
                                  onClick={() => retryWorkspaceTaskDiagnostics(runId)}
                                >
                                  다시 시도
                                </button>
                              </>
                            ) : null}
                            {state?.status === "ready" ? (
                              <>
                                {state.diagnostics?.items.length ? (
                                  <div className="workspace-diagnostic-items">
                                    {state.diagnostics.items.map((item) => {
                                      const location = `${item.file}:${item.line}${item.column ? `:${item.column}` : ""}`;
                                      return (
                                        <button
                                          type="button"
                                          className="workspace-diagnostic-item"
                                          key={`${runId}:${item.index}`}
                                          aria-label={`${location} ${item.message}`}
                                          title={`${location} · ${item.message}`}
                                          onClick={() => void handleOpenWorkspaceTaskDiagnostic(runId, item.index)}
                                        >
                                          <span>{location}</span>
                                          <span>{item.message}</span>
                                          <span>
                                            {item.severity ?? "진단"} · {item.stream}
                                          </span>
                                        </button>
                                      );
                                    })}
                                  </div>
                                ) : (
                                  <span>진단 없음</span>
                                )}
                                {state.diagnostics?.truncated ? (
                                  <span className="workspace-diagnostics-truncated">일부 진단만 표시됨</span>
                                ) : null}
                              </>
                            ) : null}
                          </div>
                        );
                      })}
                    </div>
                  ) : null}
                </div>
                <div className="job-actions">
                  <button
                    type="button"
                    className="button-primary"
                    disabled={busy || !workspaceRunnable || workspaceOperationActive}
                    title={
                      workspaceOperationActive
                        ? "이미 workspace task orchestration이 실행 중입니다."
                        : !workspaceRunnable
                          ? workspaceSnapshotFresh
                            ? workspaceTaskGateHint(workspaceTask)
                            : "workspace task 상태를 다시 불러와야 합니다."
                          : undefined
                    }
                    onClick={() => void handleRunNow(job)}
                  >
                    {workspaceOperationActive ? "실행 중…" : "지금 실행"}
                  </button>
                  <button
                    type="button"
                    className="button-danger"
                    disabled={
                      busy ||
                      (workspaceOperationActive
                        ? workspaceOperation?.status === "stopping"
                        : !activeSnapshotFresh || !activeRuns[job.id])
                    }
                    onClick={() => void handleStopRun(job)}
                  >
                    {workspaceOperationActive && workspaceOperation?.status === "stopping"
                      ? "중지 중…"
                      : workspaceOperationActive
                        ? "오케스트레이션 중지"
                        : "중지"}
                  </button>
                  {workspaceTask && !workspaceTask.trusted ? (
                    <button
                      type="button"
                      className="button-secondary"
                      disabled={busy || !workspaceTask.available}
                      onClick={() => void handleTrustWorkspaceTask(workspaceTask)}
                    >
                      소스 승인
                    </button>
                  ) : null}
                  {workspaceTask &&
                  workspaceTask.taskKind === "shell" &&
                  workspaceTask.trusted &&
                  !workspaceTask.shellTrusted ? (
                    <button
                      type="button"
                      className="button-danger"
                      disabled={busy || !workspaceTask.available}
                      onClick={() => openShellTrustConfirmation(workspaceTask)}
                    >
                      셸 실행 승인
                    </button>
                  ) : null}
                  <button type="button" className="button-secondary" onClick={() => openEdit(job)}>
                    편집
                  </button>
                  <button
                    type="button"
                    className="button-danger"
                    disabled={busy || !activeSnapshotFresh || Boolean(activeRuns[job.id]) || workspaceOperationActive}
                    onClick={() => void handleDelete(job)}
                  >
                    삭제
                  </button>
                </div>
              </article>
            );
          })}
        </div>
      ) : null}
    </section>
  );
}
