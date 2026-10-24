/** 메서드 고르기: 필터 입력 + 목록. 필터가 지금 선택을 숨기면 보이는 첫 메서드를 자동으로 고른다(AR-F07, 초안은 지우지 않음). */
import { useEffect } from "react";
import type { GrpcCatalog } from "../../../../rpc/gen/rpc";
import s from "../Api.module.css";
import { autoSelect, filterMethods } from "./grpcDrafts";

export function MethodPicker({
  catalog,
  filter,
  selected,
  dirty,
  onFilter,
  onSelect,
}: {
  catalog: GrpcCatalog | null;
  filter: string;
  selected: string | null;
  dirty: (method: string) => boolean;
  onFilter: (filter: string) => void;
  onSelect: (method: string) => void;
}) {
  const visible = filterMethods(catalog, filter);
  const auto = autoSelect(selected, visible);
  useEffect(() => {
    if (auto && auto !== selected) onSelect(auto);
  }, [auto, selected, onSelect]);
  return (
    <div>
      <input
        className={s.input}
        aria-label="메서드 필터"
        placeholder="메서드 이름으로 찾기"
        value={filter}
        onChange={(event) => onFilter(event.target.value)}
      />
      <div role="listbox" aria-label="메서드" className={s.tree}>
        {visible.map((m) => (
          <div
            key={m.fullName}
            role="option"
            tabIndex={0}
            aria-selected={m.fullName === selected}
            className={s.treeItem}
            onClick={() => onSelect(m.fullName)}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                onSelect(m.fullName);
              }
            }}
          >
            {m.fullName}
            {m.serverStreaming && <span className={s.muted}> (스트림)</span>}
            {m.clientStreaming && <span className={s.muted}> (클라이언트 스트림 미지원)</span>}
            {dirty(m.fullName) && <span className={s.muted}> (수정됨)</span>}
          </div>
        ))}
      </div>
      {catalog && visible.length === 0 && <p className={s.muted}>일치하는 메서드가 없습니다.</p>}
    </div>
  );
}
