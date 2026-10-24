/** 요청 행 표(쿼리·헤더·쿠키·form). 행마다 사용 여부·이름·값·삭제, 오류는 그 행 아래에 보인다. */
import { Button } from "../../../ui";
import s from "./Api.module.css";
import { type Row, row } from "./model/draft";

export function ParamTable({
  label,
  rows,
  onChange,
  errors = {},
}: {
  label: string;
  rows: Row[];
  onChange: (rows: Row[]) => void;
  errors?: Record<number, string>;
}) {
  const patch = (index: number, value: Partial<Row>) =>
    onChange(rows.map((item, i) => (i === index ? { ...item, ...value } : item)));
  return (
    <div className={s.table}>
      {rows.map((item, index) => {
        const n = index + 1;
        return (
          <div key={item.id} role="group" aria-label={`${label} ${n}`} className={s.row}>
            <input
              type="checkbox"
              aria-label={`${label} ${n} 사용`}
              checked={item.enabled}
              onChange={(event) => patch(index, { enabled: event.target.checked })}
            />
            <input
              className={s.input}
              aria-label={`${label} ${n} 이름`}
              value={item.name}
              onChange={(event) => patch(index, { name: event.target.value })}
            />
            <input
              className={`${s.input} ${s.mono}`}
              aria-label={`${label} ${n} 값`}
              value={item.value}
              onChange={(event) => patch(index, { value: event.target.value })}
            />
            <Button
              size="sm"
              variant="ghost"
              aria-label={`${label} ${n} 삭제`}
              onClick={() => onChange(rows.filter((_, i) => i !== index))}
            >
              삭제
            </Button>
            {errors[index] && (
              <p role="alert" className={s.rowError}>
                {errors[index]}
              </p>
            )}
          </div>
        );
      })}
      <div>
        <Button size="sm" onClick={() => onChange([...rows, row()])}>
          {`${label} 추가`}
        </Button>
      </div>
    </div>
  );
}
