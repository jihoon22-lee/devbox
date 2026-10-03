import { useLayoutEffect, useRef } from "react";
import type { KeyValue } from "../types";

export function KeyValueEditor({
  rows,
  onChange,
  namePlaceholder,
}: {
  rows: KeyValue[];
  onChange: (rows: KeyValue[]) => void;
  namePlaceholder: string;
}) {
  const container = useRef<HTMLDivElement>(null);
  const pendingFocus = useRef<number | null>(null);
  const label = namePlaceholder.replace(/\s*이름$/, "");
  useLayoutEffect(() => {
    if (pendingFocus.current === null) return;
    const fields = container.current?.querySelectorAll<HTMLInputElement>(".kv-row input:first-child");
    const index = pendingFocus.current;
    pendingFocus.current = null;
    if (fields?.length) fields[Math.min(index, fields.length - 1)].focus();
    else container.current?.querySelector<HTMLButtonElement>(".kv-add")?.focus();
  }, [rows]);
  const update = (i: number, patch: Partial<KeyValue>) => {
    onChange(rows.map((r, idx) => (idx === i ? { ...r, ...patch } : r)));
  };
  return (
    <div className="kv-editor" ref={container}>
      {rows.map((r, i) => (
        <div className="kv-row" key={i}>
          <input
            aria-label={`${label} ${i + 1} 이름`}
            placeholder={namePlaceholder}
            value={r.key}
            onChange={(e) => update(i, { key: e.currentTarget.value })}
            spellCheck={false}
          />
          <input
            aria-label={`${label} ${i + 1} 값`}
            placeholder="값"
            value={r.value}
            onChange={(e) => update(i, { value: e.currentTarget.value })}
            spellCheck={false}
          />
          <button className="kv-del" aria-label={`${label} ${r.key || i + 1} 삭제`} onClick={() => {
            pendingFocus.current = i;
            onChange(rows.filter((_, idx) => idx !== i));
          }}>
            ✕
          </button>
        </div>
      ))}
      <button className="btn kv-add" onClick={() => { pendingFocus.current = rows.length; onChange([...rows, { key: "", value: "" }]); }}>
        + 추가
      </button>
    </div>
  );
}
