import { renameFileStatusLabel } from "../lib/documentPresentation";
import { trapDialogKeyDown } from "@devbox/a11y";
import ChangeSetPreview, { type ChangeSetItem } from "./ChangeSetPreview";
import type * as React from "react";

interface Props {
  appDialogRef: React.RefObject<HTMLDivElement | null>;
  cancelPendingRename: () => void;
  renameApplyBusy: boolean;
  setRenameResult: React.Dispatch<React.SetStateAction<import("../types").LspRenameApplyResult | null>>;
  renamePreview: {
    preview: import("../types").LspRenamePreview;
    revisions: Map<string, number>;
    workspaceRoot: string;
  } | null;
  applyPendingRename: () => void;
  renameResult: import("../types").LspRenameApplyResult | null;
}

export function RenameReviewDialog({
  appDialogRef,
  cancelPendingRename,
  renameApplyBusy,
  setRenameResult,
  renamePreview,
  applyPendingRename,
  renameResult,
}: Props) {
  return (
    <div
      ref={appDialogRef}
      className="rename-dialog"
      role="dialog"
      aria-modal="true"
      aria-label="여러 파일 이름 변경 미리보기"
      onKeyDown={(event) => {
        if (appDialogRef.current) {
          trapDialogKeyDown(event, appDialogRef.current, () => {
            cancelPendingRename();
            if (!renameApplyBusy) setRenameResult(null);
          });
        }
      }}
    >
      {renamePreview && (
        <>
          <h2>여러 파일 이름 변경 미리보기</h2>
          <p className="rename-note">
            변경 범위와 위치를 확인한 뒤 적용하세요. 모든 파일은 적용 직전에 mtime·크기·SHA-256을 다시 확인하며,
            하나라도 실패하면 이미 바뀐 파일을 백업으로 되돌립니다.
          </p>
          <ChangeSetPreview
            items={renamePreview.preview.files.map(
              (file): ChangeSetItem => ({
                path: file.path,
                before: file.before,
                after: file.after,
                meta: file.ranges
                  .map(
                    ({ range }) =>
                      `${range.start.line + 1}:${range.start.character + 1}–${range.end.line + 1}:${range.end.character + 1}`,
                  )
                  .join(", "),
              }),
            )}
            title="LSP 이름 변경"
            approveLabel="전체 적용"
            selectable={false}
            disabled={renameApplyBusy}
            cancelDisabled={false}
            onApprove={() => applyPendingRename()}
            onCancel={() => cancelPendingRename()}
          />
        </>
      )}
      {renameResult && (
        <>
          <h2>{renameResult.success ? "이름 변경 완료" : "이름 변경 결과"}</h2>
          <p className="rename-note">{renameResult.error ?? "변경된 파일별 결과를 확인하세요."}</p>
          <ul className="rename-results">
            {renameResult.files.map((file) => (
              <li key={file.path} className={`rename-result ${file.status}`}>
                <code>{file.path}</code>
                <span>{renameFileStatusLabel(file.status)}</span>
                {file.error && <small>{file.error}</small>}
              </li>
            ))}
          </ul>
          <div className="confirm-dialog-actions">
            <button type="button" className="toolbar-button selected" onClick={() => setRenameResult(null)}>
              닫기
            </button>
          </div>
        </>
      )}
    </div>
  );
}
