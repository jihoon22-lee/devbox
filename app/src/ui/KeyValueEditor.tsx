import { Button } from "./Button";
import s from "./KeyValueEditor.module.css";

export interface KeyValueRow {
  id: string;
  key: string;
  value: string;
}
export function KeyValueEditor({
  value,
  onChange,
  disabled = false,
}: {
  value: KeyValueRow[];
  onChange: (value: KeyValueRow[]) => void;
  disabled?: boolean;
}) {
  const change = (id: string, patch: Partial<KeyValueRow>) =>
    onChange(value.map((row) => (row.id === id ? { ...row, ...patch } : row)));
  return (
    <fieldset className={s.editor} disabled={disabled}>
      <legend>환경 변수</legend>
      {value.map((row, index) => (
        <div className={s.row} key={row.id}>
          <input
            aria-label={`환경 변수 이름 ${index + 1}`}
            value={row.key}
            onChange={(event) => change(row.id, { key: event.target.value })}
          />
          <input
            aria-label={`환경 변수 값 ${index + 1}`}
            value={row.value}
            onChange={(event) => change(row.id, { value: event.target.value })}
          />
          <Button
            size="sm"
            aria-label={`환경 변수 삭제 ${index + 1}`}
            onClick={() => onChange(value.filter((item) => item.id !== row.id))}
          >
            삭제
          </Button>
        </div>
      ))}
      <Button size="sm" onClick={() => onChange([...value, { id: crypto.randomUUID(), key: "", value: "" }])}>
        환경 변수 추가
      </Button>
      <small>비밀 값은 직접 입력하지 말고 아래 비밀 참조에 이름을 입력하세요.</small>
    </fieldset>
  );
}
