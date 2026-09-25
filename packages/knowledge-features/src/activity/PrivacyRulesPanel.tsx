import { useEffect, useMemo, useState } from "react";
import {
  redactExisting, setPrivacyRules,
  type InvalidPrivacyRule, type PrivacyRuleField, type PrivacyRuleProblem, type PrivacyRules,
} from "./api";

export function toLines(values: string[]): string {
  return values.join("\n");
}

export function fromLines(text: string): string[] {
  return text.split(/\r?\n/).map((line) => line.trim()).filter((line) => line.length > 0);
}

const PROBLEMS: Record<PrivacyRuleProblem, string> = {
  empty: "빈 규칙입니다.",
  too_long: "512자를 넘습니다.",
  too_many: "규칙은 64개까지 입력할 수 있습니다.",
  syntax: "정규식 문법 오류입니다.",
};

interface FieldProps {
  id: PrivacyRuleField;
  label: string;
  hint: string;
  value: string;
  onChange: (value: string) => void;
  problems: InvalidPrivacyRule[];
}

function RuleField({ id, label, hint, value, onChange, problems }: FieldProps) {
  const errorId = `${id}-errors`;
  return (
    <div className="privacy-row">
      <label htmlFor={id}>{label}</label>
      <span className="dim" id={`${id}-hint`}>{hint}</span>
      <textarea
        id={id}
        rows={3}
        value={value}
        spellCheck={false}
        aria-invalid={problems.length > 0}
        aria-describedby={problems.length > 0 ? `${id}-hint ${errorId}` : `${id}-hint`}
        onChange={(event) => onChange(event.currentTarget.value)}
      />
      {problems.length > 0 && (
        <ul id={errorId} className="field-error">
          {problems.map((problem) => (
            <li key={`${problem.index}-${problem.problem}`}>{`${problem.index + 1}번째 규칙: ${PROBLEMS[problem.problem]}`}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default function PrivacyRulesPanel({ initial, healthy, onSaved }: {
  initial: PrivacyRules;
  healthy: boolean;
  onSaved: (rules: PrivacyRules) => void;
}) {
  const [processes, setProcesses] = useState(toLines(initial.excludedProcesses));
  const [excluded, setExcluded] = useState(toLines(initial.excludedTitlePatterns));
  const [redact, setRedact] = useState(toLines(initial.redactTitlePatterns));
  const [maskAll, setMaskAll] = useState(initial.maskAllTitles);
  const [invalid, setInvalid] = useState<InvalidPrivacyRule[]>([]);
  const [busy, setBusy] = useState(false);
  const [confirmApply, setConfirmApply] = useState(false);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    setProcesses(toLines(initial.excludedProcesses));
    setExcluded(toLines(initial.excludedTitlePatterns));
    setRedact(toLines(initial.redactTitlePatterns));
    setMaskAll(initial.maskAllTitles);
    setInvalid([]);
    setConfirmApply(false);
  }, [initial]);

  const draft = useMemo<PrivacyRules>(() => ({
    excludedProcesses: fromLines(processes),
    excludedTitlePatterns: fromLines(excluded),
    redactTitlePatterns: fromLines(redact),
    maskAllTitles: maskAll,
  }), [processes, excluded, redact, maskAll]);
  const dirty = JSON.stringify(draft) !== JSON.stringify(initial);
  const problemsFor = (field: PrivacyRuleField) => invalid.filter((problem) => problem.field === field);

  const save = async () => {
    setBusy(true); setError(""); setNotice("");
    try {
      const result = await setPrivacyRules(draft);
      if (result.saved) {
        setInvalid([]);
        setNotice("규칙을 저장했습니다. 다음 기록부터 적용됩니다.");
        onSaved(draft);
      } else {
        setInvalid(result.invalid);
      }
    } catch {
      setError("작업을 완료하지 못했습니다. 규칙과 저장 상태를 확인한 뒤 다시 시도해 주세요.");
    } finally {
      setBusy(false);
    }
  };

  const applyToStored = async () => {
    if (busy || dirty || !healthy) return;
    setConfirmApply(false);
    setBusy(true); setError(""); setNotice("");
    try {
      const count = await redactExisting();
      setNotice(`기존 세션 ${count}개에 규칙을 적용했습니다.`);
    } catch {
      setError("작업을 완료하지 못했습니다. 규칙과 저장 상태를 확인한 뒤 다시 시도해 주세요.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="panel" aria-labelledby="privacy-rules-heading">
      <h2 id="privacy-rules-heading">개인정보 보호 규칙</h2>
      {!healthy && (
        <p role="alert">저장된 규칙을 읽지 못해 창 제목 저장을 멈췄습니다. 규칙을 확인하고 다시 저장해 주세요.</p>
      )}
      <fieldset disabled={busy}>
      <RuleField id="excludedProcesses" label="제외할 프로세스" hint="한 줄에 하나, 대소문자 구분 없이 정확히 일치하면 세션 전체를 저장하지 않습니다."
        value={processes} onChange={setProcesses} problems={problemsFor("excludedProcesses")} />
      <RuleField id="excludedTitlePatterns" label="제목을 저장하지 않을 정규식" hint="한 줄에 하나. 일치하면 세션은 남기고 제목만 비웁니다."
        value={excluded} onChange={setExcluded} problems={problemsFor("excludedTitlePatterns")} />
      <RuleField id="redactTitlePatterns" label="제목 치환 정규식" hint="한 줄에 하나. 일치한 부분을 [redacted]로 바꿉니다."
        value={redact} onChange={setRedact} problems={problemsFor("redactTitlePatterns")} />
      <label className="row">
        <input type="checkbox" checked={maskAll} onChange={(event) => setMaskAll(event.currentTarget.checked)} />
        모든 제목을 저장하지 않음
      </label>
      </fieldset>
      <div className="row">
        <button className="btn" disabled={busy || (!dirty && healthy)} onClick={() => void save()}>규칙 저장</button>
        <button className="btn" disabled={busy || dirty || !healthy} onClick={() => setConfirmApply(true)}>기존 세션에 적용</button>
      </div>
      {confirmApply && !dirty && healthy && <div>
        <p>제외된 세션을 삭제하고 제목을 변경합니다. 이 작업은 되돌릴 수 없습니다.</p>
        <button disabled={busy} onClick={() => void applyToStored()}>기록 변경 확인</button>
        <button disabled={busy} onClick={() => setConfirmApply(false)}>취소</button>
      </div>}
      {notice && <p role="status">{notice}</p>}
      {error && <p role="alert">{error}</p>}
      <div className="dim">규칙은 DB 저장 전에 적용됩니다. 제외한 원문은 어디에도 남지 않습니다.</div>
    </section>
  );
}
