import { trapDialogKeyDown } from "@devbox/a11y";
import type { RefObject } from "react";
import type { Doc } from "../types";

interface FileConfirmationDialogsProps {
  appDialogRef: RefObject<HTMLDivElement | null>;
  pendingCloseDoc: Doc | null | undefined;
  pendingCloseCount: number;
  encodingReopenPending: boolean;
  busy: boolean;
  cancelClose: () => void;
  cancelEncodingReopen: () => void;
  handleDiscardClose: () => void;
  handleSaveAndClose: () => unknown;
  confirmEncodingReopen: () => unknown;
}

export function FileConfirmationDialogs({
  appDialogRef,
  pendingCloseDoc,
  pendingCloseCount,
  encodingReopenPending,
  busy,
  cancelClose,
  cancelEncodingReopen,
  handleDiscardClose,
  handleSaveAndClose,
  confirmEncodingReopen,
}: FileConfirmationDialogsProps) {
  return (
    <>
      {pendingCloseDoc && (
        <div className="modal-backdrop" role="presentation">
          <div
            ref={appDialogRef}
            className="confirm-dialog"
            role="dialog"
            aria-modal="true"
            aria-label="저장되지 않은 변경 사항"
            onKeyDown={(event) => {
              if (appDialogRef.current) {
                trapDialogKeyDown(event, appDialogRef.current, () => cancelClose());
              }
            }}
          >
            <h2>저장되지 않은 변경 사항</h2>
            <p>
              {pendingCloseDoc.path}에 저장되지 않은 변경 사항이 있습니다. 어떻게 하시겠습니까?
              {pendingCloseCount > 1 && ` (이후 ${pendingCloseCount - 1}개 대기)`}
            </p>
            <div className="confirm-dialog-actions">
              <button type="button" className="toolbar-button" onClick={() => cancelClose()}>
                취소
              </button>
              <button type="button" className="toolbar-button" onClick={handleDiscardClose}>
                변경 내용 버리고 닫기
              </button>
              <button type="button" className="toolbar-button selected" onClick={handleSaveAndClose} disabled={busy}>
                저장 후 닫기
              </button>
            </div>
          </div>
        </div>
      )}

      {encodingReopenPending && (
        <div className="modal-backdrop" role="presentation">
          <div
            ref={appDialogRef}
            className="confirm-dialog"
            role="dialog"
            aria-modal="true"
            aria-label="인코딩 다시 열기"
            onKeyDown={(event) => {
              if (appDialogRef.current) {
                trapDialogKeyDown(event, appDialogRef.current, () => cancelEncodingReopen());
              }
            }}
          >
            <h2>인코딩을 바꿔 다시 열까요?</h2>
            <p>저장되지 않은 변경 사항이 버려집니다. 선택한 인코딩으로 디스크 파일을 엄격하게 다시 읽습니다.</p>
            <div className="confirm-dialog-actions">
              <button type="button" className="toolbar-button" onClick={() => cancelEncodingReopen()}>
                취소
              </button>
              <button type="button" className="toolbar-button selected" onClick={confirmEncodingReopen} disabled={busy}>
                변경 내용 버리고 다시 열기
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
