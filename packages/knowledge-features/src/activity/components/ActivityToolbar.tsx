import { VIEW_LABELS } from "../lib/activityPresentation";
import { parseDateKey } from "../lib/contextMenu";
import { isTauri } from "../lib/isTauri";

interface Props {
  onDaily: (() => void) | undefined;
  contextActionBusy: boolean;
  shift: (delta: number) => void;
  dateStr: string;
  selectDate: (next: Date) => void;
  dateContextMenu: import("../../../../context-menu/src/useContextMenu").ContextMenuController;
  loading: boolean;
  cancelCurrentLoad: () => Promise<void>;
  view: import("../lib/activityPresentation").ViewTab;
  selectView: (next: import("../lib/activityPresentation").ViewTab) => void;
  load: () => Promise<void>;
  openExportDialog: () => void;
}

export function ActivityToolbar({
  onDaily,
  contextActionBusy,
  shift,
  dateStr,
  selectDate,
  dateContextMenu,
  loading,
  cancelCurrentLoad,
  view,
  selectView,
  load,
  openExportDialog,
}: Props) {
  return (
    <header className="toolbar">
      {onDaily && (
        <button type="button" className="btn" disabled={contextActionBusy} onClick={onDaily}>
          이 날짜의 일일 기록
        </button>
      )}
      <button
        type="button"
        className="btn"
        aria-label="이전 날짜"
        onClick={() => shift(-1)}
        disabled={contextActionBusy}
      >
        ◀
      </button>
      <input
        type="date"
        className="date-input"
        value={dateStr}
        data-date={dateStr}
        aria-label={`${dateStr} 선택된 날짜`}
        disabled={contextActionBusy}
        onChange={(e) => {
          const parsed = parseDateKey(e.currentTarget.value);
          if (parsed) selectDate(parsed);
        }}
        onContextMenu={dateContextMenu.triggerProps.onContextMenu}
        onKeyDown={dateContextMenu.triggerProps.onKeyDown}
      />
      <button
        type="button"
        className="btn"
        aria-label="다음 날짜"
        onClick={() => shift(1)}
        disabled={contextActionBusy}
      >
        ▶
      </button>
      <button type="button" className="btn" onClick={() => selectDate(new Date())} disabled={contextActionBusy}>
        오늘
      </button>
      <span className="spacer" />
      {loading && (
        <>
          <span className="loading" role="status" aria-live="polite">
            불러오는 중…
          </span>
          <button
            type="button"
            className="btn"
            onClick={() => void cancelCurrentLoad()}
            aria-label="데이터 불러오기 취소"
          >
            취소
          </button>
        </>
      )}
      {(["day", "week", "month", "timeline", "settings"] as const).map((t) => (
        <button
          type="button"
          key={t}
          className={`btn ${view === t ? "active" : ""}`}
          aria-pressed={view === t}
          onClick={() => selectView(t)}
          disabled={contextActionBusy}
        >
          {VIEW_LABELS[t]}
        </button>
      ))}
      <button type="button" className="btn refresh" onClick={() => void load()} disabled={contextActionBusy}>
        새로 고침
      </button>
      <button type="button" className="btn" onClick={openExportDialog} disabled={contextActionBusy || loading}>
        {isTauri() ? "기간 내보내기" : "내보내기 미리보기"}
      </button>
    </header>
  );
}
