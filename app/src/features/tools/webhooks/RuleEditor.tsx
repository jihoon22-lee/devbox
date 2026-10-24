import { useRef, useState } from "react";
import type { ResponseRule } from "../../../rpc/gen/rpc";
import { Button } from "../../../ui";

const empty = (): ResponseRule => ({
  id: "",
  priority: 0,
  method: null,
  path: "/*",
  status: 200,
  headers: [],
  body: "",
  delayMs: 0,
  sequence: [],
});
export function RuleEditor({
  initialRule,
  items,
  onSave,
  onDelete,
}: {
  initialRule?: ResponseRule;
  items: ResponseRule[];
  onSave: (rule: ResponseRule) => Promise<ResponseRule>;
  onDelete: (id: string) => void;
}) {
  const [rule, setRule] = useState<ResponseRule>(initialRule ?? empty);
  const [headerText, setHeaders] = useState("[]");
  const [sequenceText, setSequence] = useState("[]");
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const pending = useRef(false);
  const selection = useRef(0);
  function edit(next: ResponseRule) {
    selection.current += 1;
    setRule(next);
    setHeaders(JSON.stringify(next.headers));
    setSequence(JSON.stringify(next.sequence ?? []));
  }
  return (
    <section aria-label="응답 규칙">
      <h3>응답 규칙</h3>
      <ul>
        {items.map((r) => (
          <li key={r.id}>
            <button type="button" onClick={() => edit(r)}>
              {r.method ?? "모든 메서드"} {r.path} → {r.status}
            </button>
            <Button onClick={() => onDelete(r.id)}>삭제</Button>
          </li>
        ))}
      </ul>
      <Button onClick={() => edit(empty())}>새 규칙</Button>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (pending.current) return;
          let snapshot: ResponseRule;
          try {
            snapshot = { ...rule, headers: JSON.parse(headerText), sequence: JSON.parse(sequenceText) };
          } catch {
            setError("헤더와 순서 응답을 JSON 배열로 입력해 주세요.");
            return;
          }
          const selected = selection.current;
          pending.current = true;
          setSaving(true);
          setError("");
          try {
            const saved = await onSave(snapshot);
            // The daemon allocates new IDs. Only adopt identity; edits made while awaiting it stay intact.
            if (selection.current === selected) setRule((current) => ({ ...current, id: saved.id }));
          } catch {
            if (selection.current === selected) setError("규칙을 저장하지 못했습니다. 다시 시도해 주세요.");
          } finally {
            pending.current = false;
            setSaving(false);
          }
        }}
      >
        <label>
          메서드{" "}
          <input
            value={rule.method ?? ""}
            placeholder="모든 메서드"
            onChange={(e) => setRule({ ...rule, method: e.target.value || null })}
          />
        </label>
        <label>
          경로 <input value={rule.path} onChange={(e) => setRule({ ...rule, path: e.target.value })} />
        </label>
        <label>
          우선순위{" "}
          <input
            type="number"
            min={-1000}
            max={1000}
            value={rule.priority}
            onChange={(e) => setRule({ ...rule, priority: Number(e.target.value) })}
          />
        </label>
        <label>
          응답 상태{" "}
          <input
            type="number"
            min={200}
            max={599}
            value={rule.status}
            onChange={(e) => setRule({ ...rule, status: Number(e.target.value) })}
          />
        </label>
        <label>
          지연(ms){" "}
          <input
            type="number"
            min={0}
            max={60000}
            value={Number(rule.delayMs)}
            onChange={(e) => setRule({ ...rule, delayMs: Number(e.target.value) })}
          />
        </label>
        <label>
          응답 본문 <textarea value={rule.body} onChange={(e) => setRule({ ...rule, body: e.target.value })} />
        </label>
        <label>
          응답 헤더(JSON) <textarea value={headerText} onChange={(e) => setHeaders(e.target.value)} />
        </label>
        <label>
          추가 순서 응답(JSON) <textarea value={sequenceText} onChange={(e) => setSequence(e.target.value)} />
        </label>
        <p>
          처음에는 기본 응답을, 다음부터 순서 응답을 사용하며 마지막 응답을 유지합니다. 규칙을 저장하면 순서를 처음부터
          시작합니다.
        </p>
        {error && <p role="alert">{error}</p>}
        <Button type="submit" disabled={saving}>
          규칙 저장
        </Button>
      </form>
    </section>
  );
}
