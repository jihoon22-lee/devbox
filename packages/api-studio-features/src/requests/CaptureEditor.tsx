import { useId } from "react";
import { VARIABLE_NAME, type Capture } from "./lib/captures";
import { evaluateJsonPath } from "./lib/jsonPath";
interface Props {
  value: Capture[];
  onChange: (value: Capture[]) => void;
  disabled?: boolean;
}
export function CaptureEditor({ value, onChange, disabled = false }: Props) {
  const prefix = useId();
  const patch = (index: number, change: Partial<Capture>) =>
    onChange(value.map((item, i) => (i === index ? { ...item, ...change } : item)));
  return (
    <section aria-label="응답 캡처 편집">
      <h3>캡처</h3>
      {value.map((item, index) => {
        let error = VARIABLE_NAME.test(item.variable) ? null : "변수 이름이 올바르지 않습니다";
        if (!error && item.source === "jsonPath") {
          try {
            evaluateJsonPath(null, item.target);
          } catch {
            error = "지원하지 않는 JSONPath입니다";
          }
        }
        const errorId = `${prefix}-${index}`;
        return (
          <fieldset key={item.id} disabled={disabled}>
            <legend>캡처 {index + 1}</legend>
            <label>
              <input
                type="checkbox"
                checked={item.enabled}
                onChange={(e) => patch(index, { enabled: e.currentTarget.checked })}
              />
              사용
            </label>
            <input
              aria-label={`변수 이름 ${index + 1}`}
              aria-describedby={error ? errorId : undefined}
              maxLength={64}
              value={item.variable}
              onChange={(e) => patch(index, { variable: e.currentTarget.value })}
            />
            <select
              aria-label={`캡처 출처 ${index + 1}`}
              value={item.source}
              onChange={(e) => patch(index, { source: e.currentTarget.value as Capture["source"] })}
            >
              {(["jsonPath", "header", "status"] as const).map((source) => (
                <option key={source} value={source}>
                  {{ jsonPath: "JSONPath", header: "헤더", status: "상태 코드" }[source]}
                </option>
              ))}
            </select>
            {item.source !== "status" && (
              <input
                aria-label={`캡처 대상 ${index + 1}`}
                maxLength={256}
                value={item.target}
                onChange={(e) => patch(index, { target: e.currentTarget.value })}
              />
            )}
            {error && <p id={errorId}>{error}</p>}
            <button type="button" onClick={() => onChange(value.filter((_, i) => i !== index))}>
              캡처 {index + 1} 삭제
            </button>
          </fieldset>
        );
      })}
      <button
        type="button"
        disabled={disabled || value.length >= 20}
        onClick={() =>
          onChange([
            ...value,
            { id: crypto.randomUUID(), enabled: true, variable: "", source: "jsonPath", target: "$" },
          ])
        }
      >
        캡처 추가
      </button>
    </section>
  );
}
