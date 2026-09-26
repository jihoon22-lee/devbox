import { type ExportFormat } from "../api";
import { isTauri } from "../lib/isTauri";
import type * as React from "react";

interface Props {
  exportDialogRef: React.RefObject<HTMLElement | null>;
  contextActionBusy: boolean;
  exportFirstFieldRef: React.RefObject<HTMLInputElement | null>;
  exportStartDate: string;
  setExportStartDate: React.Dispatch<React.SetStateAction<string>>;
  exportEndDate: string;
  setExportEndDate: React.Dispatch<React.SetStateAction<string>>;
  exportFormat: import("../../generated/ExportFormat").ExportFormat;
  setExportFormat: React.Dispatch<React.SetStateAction<import("../../generated/ExportFormat").ExportFormat>>;
  setExportDialogOpen: React.Dispatch<React.SetStateAction<boolean>>;
  submitRangeExport: () => Promise<void>;
}

export function ActivityExportDialog({
  exportDialogRef,
  contextActionBusy,
  exportFirstFieldRef,
  exportStartDate,
  setExportStartDate,
  exportEndDate,
  setExportEndDate,
  exportFormat,
  setExportFormat,
  setExportDialogOpen,
  submitRangeExport,
}: Props) {
  return (
    <section
      className="export-dialog"
      ref={exportDialogRef}
      role="dialog"
      aria-modal="true"
      aria-labelledby="life-log-export-title"
      aria-describedby="life-log-export-description"
      aria-busy={contextActionBusy}
      tabIndex={-1}
    >
      <h2 id="life-log-export-title">Life Log 내보내기</h2>
      <p id="life-log-export-description" className="dim">
        {isTauri()
          ? "선택한 기간의 활동·Git·검증된 로컬 소스 요약을 파일로 저장합니다."
          : "브라우저 미리보기는 로컬 DB·Git·snapshot을 포함하지 않습니다."}
      </p>
      <div className="export-fields">
        <label htmlFor="life-log-export-start">
          시작 날짜
          <input
            id="life-log-export-start"
            ref={exportFirstFieldRef}
            type="date"
            value={exportStartDate}
            onChange={(event) => setExportStartDate(event.currentTarget.value)}
            disabled={contextActionBusy}
          />
        </label>
        <label htmlFor="life-log-export-end">
          종료 날짜
          <input
            id="life-log-export-end"
            type="date"
            value={exportEndDate}
            onChange={(event) => setExportEndDate(event.currentTarget.value)}
            disabled={contextActionBusy}
          />
        </label>
        <label htmlFor="life-log-export-format">
          형식
          <select
            id="life-log-export-format"
            value={exportFormat}
            onChange={(event) => setExportFormat(event.currentTarget.value as ExportFormat)}
            disabled={contextActionBusy}
          >
            <option value="markdown">Markdown</option>
            <option value="json">JSON</option>
            <option value="csv">CSV</option>
          </select>
        </label>
      </div>
      <div className="export-actions">
        <button type="button" className="btn" onClick={() => setExportDialogOpen(false)} disabled={contextActionBusy}>
          취소
        </button>
        <button
          type="button"
          className="btn active"
          onClick={() => void submitRangeExport()}
          disabled={contextActionBusy}
        >
          {contextActionBusy ? "내보내는 중…" : isTauri() ? "저장" : "미리보기 다운로드"}
        </button>
      </div>
    </section>
  );
}
