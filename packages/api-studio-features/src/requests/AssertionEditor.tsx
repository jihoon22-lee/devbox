import { useId } from "react";
import { ASSERTION_OPERATORS, ASSERTION_SOURCES, validateAssertion, type Assertion } from "./lib/assertions";
interface Props {
  value: Assertion[];
  onChange: (value: Assertion[]) => void;
  disabled?: boolean;
}
export function AssertionEditor({ value, onChange, disabled = false }: Props) {
  const prefix = useId();
  const patch = (index: number, change: Partial<Assertion>) =>
    onChange(value.map((item, i) => (i === index ? { ...item, ...change } : item)));
  return (
    <section aria-label="응답 검증 편집">
      <h3>검증</h3>
      {value.map((item, index) => {
        const error = validateAssertion(item);
        const errorId = `${prefix}-${index}`;
        return (
          <fieldset key={item.id} disabled={disabled}>
            <legend>검증 {index + 1}</legend>
            <label>
              <input
                type="checkbox"
                checked={item.enabled}
                onChange={(e) => patch(index, { enabled: e.currentTarget.checked })}
              />
              사용
            </label>
            <select
              aria-label={`출처 ${index + 1}`}
              value={item.source}
              onChange={(e) => patch(index, { source: e.currentTarget.value as Assertion["source"] })}
            >
              {ASSERTION_SOURCES.map((source) => (
                <option key={source} value={source}>
                  {
                    { status: "상태 코드", header: "헤더", jsonPath: "JSONPath", body: "본문", duration: "응답 시간" }[
                      source
                    ]
                  }
                </option>
              ))}
            </select>
            {["header", "jsonPath"].includes(item.source) && (
              <input
                aria-label={`대상 ${index + 1}`}
                aria-describedby={error ? errorId : undefined}
                maxLength={256}
                value={item.target}
                onChange={(e) => patch(index, { target: e.currentTarget.value })}
              />
            )}
            <select
              aria-label={`연산 ${index + 1}`}
              value={item.operator}
              onChange={(e) => patch(index, { operator: e.currentTarget.value as Assertion["operator"] })}
            >
              {ASSERTION_OPERATORS.map((operator) => (
                <option key={operator} value={operator}>
                  {
                    {
                      equals: "같음",
                      notEquals: "다름",
                      contains: "포함",
                      notContains: "포함하지 않음",
                      matches: "정규식 일치",
                      exists: "존재",
                      notExists: "없음",
                      lessThan: "미만",
                      greaterThan: "초과",
                    }[operator]
                  }
                </option>
              ))}
            </select>
            {!["exists", "notExists"].includes(item.operator) && (
              <input
                aria-label={`기대값 ${index + 1}`}
                aria-describedby={error ? errorId : undefined}
                maxLength={65536}
                value={item.expected}
                onChange={(e) => patch(index, { expected: e.currentTarget.value })}
              />
            )}
            {error && <p id={errorId}>{error}</p>}
            <button type="button" onClick={() => onChange(value.filter((_, i) => i !== index))}>
              검증 {index + 1} 삭제
            </button>
          </fieldset>
        );
      })}
      <button
        type="button"
        disabled={disabled || value.length >= 50}
        onClick={() =>
          onChange([
            ...value,
            {
              id: crypto.randomUUID(),
              enabled: true,
              source: "status",
              target: "",
              operator: "equals",
              expected: "200",
            },
          ])
        }
      >
        검증 추가
      </button>
    </section>
  );
}
