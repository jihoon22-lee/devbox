// §12.5 변경 집합 preview 부품 (일반화).
//
// 입력: "파일 경로 → (before, after)" 목록. 항목 단위·전체 단위 승인을 모두
// 받는다. 실제 적용은 이 컴포넌트 밖에서 수행한다 — 컴포넌트는 preview와
// 선택까지만 책임진다. Code Pad 문서 모델에 결합하지 않는다.

import { useMemo, useState } from "react";

export interface ChangeSetItem {
  path: string;
  before: string;
  after: string;
}

interface Props {
  items: ChangeSetItem[];
  title?: string;
  approveLabel?: string;
  /** 승인할 항목 경로 목록을 넘긴다. */
  onApprove: (paths: string[]) => void;
  /** 거부할 항목 경로 목록을 넘긴다. */
  onReject: (paths: string[]) => void;
  onCancel?: () => void;
}

export default function ChangeSetPreview({
  items,
  title = "변경 사항",
  approveLabel = "적용",
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
        <button className="btn mini" onClick={toggleAll}>
          {allSelected ? "전체 해제" : "전체 선택"}
        </button>
      </div>

      <div className="changeset-list">
        {items.map((item) => (
          <div key={item.path} className={`changeset-item ${selected.has(item.path) ? "selected" : ""}`}>
            <label className="changeset-check">
              <input type="checkbox" checked={selected.has(item.path)} onChange={() => toggle(item.path)} />
            </label>
            <div className="changeset-body">
              <div className="changeset-path">{item.path}</div>
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
        <button className="btn" disabled={selectedItems.length === 0} onClick={() => onApprove(selectedItems.map((i) => i.path))}>
          {approveLabel} ({selectedItems.length})
        </button>
        <button className="btn" disabled={selectedItems.length === 0} onClick={() => onReject(selectedItems.map((i) => i.path))}>
          폐기 ({selectedItems.length})
        </button>
        {onCancel && (
          <button className="btn" onClick={onCancel}>
            취소
          </button>
        )}
      </div>
    </div>
  );
}
