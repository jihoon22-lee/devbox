import { useEffect, useId, useRef, useState } from "react";
import type { InvalidRule, PrivacyRules, RuleField, RuleProblem } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation, useRpcQuery } from "../../rpc/query";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Button } from "../../ui";

const FIELDS: { key: RuleField; label: string }[] = [
  { key: "excludedProcesses", label: "제외할 앱 이름" },
  { key: "excludedTitlePatterns", label: "제목 제외 정규식" },
  { key: "redactTitlePatterns", label: "제목 가림 정규식" },
];
const PROBLEMS: Record<RuleProblem, string> = {
  empty: "내용을 입력하세요.",
  too_long: "512자 이하로 입력하세요.",
  too_many: "규칙은 목록마다 64개까지 넣을 수 있습니다.",
  syntax: "정규식 문법을 확인하세요.",
};
export function RulesEditor() {
  const uid = useId();
  const { state } = useRpcSession();
  const query = useRpcQuery("activity.rules", {});
  const save = useRpcMutation("activity.set_rules");
  const [rules, setRules] = useState<PrivacyRules>();
  const [idle, setIdle] = useState("5");
  const [validation, setValidation] = useState<string>();
  const dirty = useRef(false);
  useEffect(() => {
    if (query.data && !dirty.current) {
      setRules(query.data.rules);
      setIdle(String(query.data.idleThresholdMs / 60000));
    }
  }, [query.data]);
  const edit = (next: PrivacyRules) => {
    dirty.current = true;
    setRules(next);
    save.reset();
  };
  const detail = save.error?.detail as { problems?: InvalidRule[] } | undefined;
  const problems =
    save.error?.code === "activity.invalid_rules" && Array.isArray(detail?.problems) ? detail.problems : [];
  const submit = () => {
    if (!rules) return;
    const minutes = Number(idle);
    if (!Number.isFinite(minutes) || minutes < 1 || minutes > 1440) {
      setValidation("유휴 시간은 1분부터 1440분까지 입력하세요.");
      return;
    }
    setValidation(undefined);
    save.mutate(
      { rules, idleThresholdMs: Math.round(minutes * 60000) },
      {
        onSuccess: () => {
          dirty.current = false;
          void query.refetch();
        },
      },
    );
  };
  const error = save.error ?? query.error;
  return (
    <section aria-label="활동 개인정보 규칙">
      <h3>개인정보 규칙</h3>
      <p>제외할 앱은 기록하지 않습니다. 제목 제외는 제목만 비우고, 제목 가림은 일치한 부분을 가립니다.</p>
      {!rules && !query.error && <p role="status">규칙을 불러오는 중…</p>}
      {rules && (
        <>
          <label>
            <input
              type="checkbox"
              checked={rules.maskAllTitles}
              disabled={save.isPending}
              onChange={(event) => edit({ ...rules, maskAllTitles: event.target.checked })}
            />
            모든 창 제목 숨기기
          </label>
          {FIELDS.map(({ key, label }) => (
            <fieldset key={key} disabled={save.isPending}>
              <legend>{label}</legend>
              {rules[key].map((value, index) => {
                const problem = problems.find((p) => p.field === key && p.index === index);
                const id = `${uid}-${key}-${index}`;
                return (
                  <div key={id}>
                    <label htmlFor={id}>
                      {label} {index + 1}
                    </label>
                    <input
                      id={id}
                      value={value}
                      aria-invalid={!!problem}
                      aria-describedby={problem ? `${id}-error` : undefined}
                      onChange={(event) =>
                        edit({
                          ...rules,
                          [key]: rules[key].map((old, at) => (at === index ? event.target.value : old)),
                        })
                      }
                    />
                    <Button onClick={() => edit({ ...rules, [key]: rules[key].filter((_, at) => at !== index) })}>
                      삭제: {label} {index + 1}
                    </Button>
                    {problem && (
                      <p id={`${id}-error`} role="alert">
                        {PROBLEMS[problem.problem]}
                      </p>
                    )}
                  </div>
                );
              })}
              <Button disabled={rules[key].length >= 64} onClick={() => edit({ ...rules, [key]: [...rules[key], ""] })}>
                {label} 추가
              </Button>
            </fieldset>
          ))}
          <label>
            유휴로 전환할 시간(분){" "}
            <input
              type="number"
              min={1}
              max={1440}
              value={idle}
              disabled={save.isPending}
              onChange={(event) => {
                dirty.current = true;
                setIdle(event.target.value);
              }}
            />
          </label>
          <Button disabled={save.isPending || state !== "ready"} onClick={submit}>
            {save.isPending ? "저장 중…" : "개인정보 규칙 저장"}
          </Button>
          {save.isSuccess && <p role="status">개인정보 규칙을 저장했습니다.</p>}
        </>
      )}
      {validation && <p role="alert">{validation}</p>}
      {error && !problems.length && (
        <p role="alert">
          {messageFor(error.code, error.detail).title}{" "}
          {!rules && <Button onClick={() => void query.refetch()}>다시 시도</Button>}
        </p>
      )}
    </section>
  );
}
