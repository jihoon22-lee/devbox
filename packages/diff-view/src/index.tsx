//! §12.5 변경 집합 preview 부품 (일반화).
//!
//! 추출 근거: code-pad(첫 소비자, crash recovery)와 run-manager(두 번째 소비자,
//! definition import)가 같은 "적용 전 변경 집합을 보여주고 사용자 승인을 받는" UI를
//! 필요로 한다. 입력은 "경로/ID → (before, after)" 목록으로 일반화했고, 실제 적용은
//! 컴포넌트 밖에서 수행한다 (preview와 선택까지만 책임).

import { useMemo, useState } from "react";

export interface ChangeSetItem {
  /** 식별자 (경로 또는 id). */
  path: string;
  before: string;
  after: string;
  /** 추가 메타 (예: import 충돌 표시). */
  meta?: string;
}

interface Props {
  items: ChangeSetItem[];
  title?: string;
  approveLabel?: string;
  /** false면 전체 변경을 하나의 transaction처럼 승인하는 고정 목록으로 표시한다. */
  selectable?: boolean;
  disabled?: boolean;
  /** 승인할 항목의 path 목록을 넘긴다. */
  onApprove: (paths: string[]) => void;
  /** 거부할 항목의 path 목록을 넘긴다. */
  onReject?: (paths: string[]) => void;
  onCancel?: () => void;
}

export default function ChangeSetPreview({
  items,
  title = "변경 사항",
  approveLabel = "적용",
  selectable = true,
  disabled = false,
  onApprove,
  onReject,
  onCancel,
}: Props) {
  const [selected, setSelected] = useState<Set<string>>(
    () => new Set(items.map((i) => i.path)),
  );

  const toggle = (path: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const allSelected = useMemo(
    () => items.length > 0 && selected.size === items.length,
    [items, selected],
  );

  const selectedItems = useMemo(
    () => items.filter((i) => selected.has(i.path)),
    [items, selected],
  );

  const toggleAll = () => {
    setSelected(allSelected ? new Set() : new Set(items.map((i) => i.path)));
  };

  return (
    <div className="changeset">
      <div className="changeset-head">
        <span className="changeset-title">{title} ({items.length})</span>
        {selectable && (
          <button className="btn mini" disabled={disabled} onClick={toggleAll}>
            {allSelected ? "전체 해제" : "전체 선택"}
          </button>
        )}
      </div>

      <div className="changeset-list">
        {items.map((item) => (
          <div key={item.path} className={`changeset-item ${selected.has(item.path) ? "selected" : ""}`}>
            {selectable && (
              <label className="changeset-check">
                <input
                  type="checkbox"
                  checked={selected.has(item.path)}
                  disabled={disabled}
                  onChange={() => toggle(item.path)}
                />
              </label>
            )}
            <div className="changeset-body">
              <div className="changeset-path">
                {item.path}
                {item.meta ? <span className="changeset-meta"> — {item.meta}</span> : null}
              </div>
              <div className="changeset-diff">
                <pre className="changeset-before">{item.before}</pre>
                <pre className="changeset-after">{item.after}</pre>
              </div>
            </div>
          </div>
        ))}
        {items.length === 0 && <div className="empty">변경 사항이 없습니다.</div>}
      </div>

      <div className="changeset-actions">
        <button
          className="btn"
          disabled={disabled || (selectable ? selectedItems.length === 0 : items.length === 0)}
          onClick={() => onApprove((selectable ? selectedItems : items).map((i) => i.path))}
        >
          {approveLabel} ({selectable ? selectedItems.length : items.length})
        </button>
        {onReject && (
          <button className="btn" disabled={disabled || selectedItems.length === 0} onClick={() => onReject(selectedItems.map((i) => i.path))}>
            폐기 ({selectedItems.length})
          </button>
        )}
        {onCancel && (
          <button className="btn" disabled={disabled} onClick={onCancel}>
            취소
          </button>
        )}
      </div>
    </div>
  );
}
