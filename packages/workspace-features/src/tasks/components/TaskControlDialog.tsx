import { shortRevision, taskControlActionLabel } from "../lib/taskPresentation";
import type * as React from "react";

interface Props {
  onTaskControlDialogKeyDown: (event: React.KeyboardEvent<HTMLElement>) => void;
  taskControlPreview: import("../types").WorkspaceTaskControlPreview;
  taskControlLeaseUntil: number | null;
  taskControlCancelRef: React.RefObject<HTMLButtonElement | null>;
  busy: boolean;
  handleRejectTaskControl: () => Promise<void>;
  handleAcceptTaskControl: () => Promise<void>;
}

export function TaskControlDialog({
  onTaskControlDialogKeyDown,
  taskControlPreview,
  taskControlLeaseUntil,
  taskControlCancelRef,
  busy,
  handleRejectTaskControl,
  handleAcceptTaskControl,
}: Props) {
  return (
    <section
      className="launcher-task-dialog task-control-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="task-control-title"
      aria-describedby="task-control-description"
      onKeyDown={onTaskControlDialogKeyDown}
    >
      <h2 id="task-control-title">Workbench 요청 확인</h2>
      <p id="task-control-description">
        Workbench가 요청한 workspace task <strong>{taskControlActionLabel(taskControlPreview.action)}</strong> 작업을
        확인합니다.
        {taskControlPreview.action === "start"
          ? " 승인하면 현재 저장된 source revision을 다시 검증한 뒤 요청된 작업을 수행합니다."
          : " 승인하면 이 task가 root인 Run Manager 소유의 활성 operation만 중지합니다."}
      </p>
      <dl className="workspace-task-details task-control-details">
        <div>
          <dt>task</dt>
          <dd>{taskControlPreview.label}</dd>
        </div>
        <div>
          <dt>종류</dt>
          <dd>{taskControlPreview.taskKind}</dd>
        </div>
        <div>
          <dt>source revision</dt>
          <dd>
            <code>{shortRevision(taskControlPreview.expectedRevision)}</code>
          </dd>
        </div>
        <div>
          <dt>요청</dt>
          <dd>{taskControlActionLabel(taskControlPreview.action)}</dd>
        </div>
      </dl>
      <div className="workspace-task-notice" role="note">
        명령·경로·환경변수는 이 handoff에 포함되지 않으며, 실행 여부는 Run Manager가 다시 검증합니다.
        {taskControlLeaseUntil
          ? ` 확인 lease 만료 예정: ${new Date(taskControlLeaseUntil).toLocaleTimeString()}`
          : " 확인 요청은 제한 시간 동안만 유효합니다."}
      </div>
      <div className="launcher-task-actions">
        <button
          ref={taskControlCancelRef}
          type="button"
          className="button-secondary"
          disabled={busy}
          onClick={() => void handleRejectTaskControl()}
        >
          거절
        </button>
        <button type="button" className="button-primary" disabled={busy} onClick={() => void handleAcceptTaskControl()}>
          승인하고 {taskControlActionLabel(taskControlPreview.action)}
        </button>
      </div>
    </section>
  );
}
