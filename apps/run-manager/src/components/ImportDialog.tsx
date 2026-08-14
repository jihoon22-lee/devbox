// 정의 import 다이얼로그 (§13.2, §12.5 diff-view 사용).
// JSON 붙여넣기 → 계획(충돌 표시) → 항목별 선택 → 적용.

import { useState } from "react";
import ChangeSetPreview, { type ChangeSetItem } from "@devbox/diff-view";
import { applyImport, importDefinitions, type ImportPlan } from "../api";

interface Props {
  onDone: (created: number) => void;
  onClose: () => void;
}

export default function ImportDialog({ onDone, onClose }: Props) {
  const [json, setJson] = useState("");
  const [plan, setPlan] = useState<ImportPlan | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const preview = async () => {
    setBusy(true);
    setError(null);
    try {
      setPlan(await importDefinitions(json));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const items: ChangeSetItem[] = (plan?.items ?? []).map((item) => ({
    path: `${item.kind}:${item.name} (${item.id})`,
    before: item.status === "conflict" ? "(기존 정의 존재)" : "(신규)",
    after: item.status === "conflict" ? "(기존 정의 유지 — 건너뜀)" : item.detail,
    meta: item.status === "conflict" ? "충돌 — 건너뜀" : "신규",
  }));

  const apply = async (selectedPaths: string[]) => {
    if (!plan) return;
    setBusy(true);
    setError(null);
    try {
      // ChangeSetPreview의 path는 "kind:name (id)" — id 추출
      const selectedIds = plan.items
        .filter((item) => selectedPaths.some((p) => p.endsWith(`(${item.id})`)))
        .map((item) => item.id);
      const created = await applyImport(json, selectedIds);
      onDone(created);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" role="presentation">
      <div className="import-dialog" role="dialog" aria-modal="true">
        <h2>정의 가져오기</h2>
        {error && <div className="error">{error}</div>}
        {!plan ? (
          <>
            <textarea
              className="import-textarea"
              placeholder='여기에 export한 JSON을 붙여넣으세요 ({"schemaVersion":1,"jobs":[...],"services":[...]})'
              value={json}
              onChange={(e) => setJson(e.currentTarget.value)}
              spellCheck={false}
            />
            <div className="import-actions">
              <button className="button-primary" disabled={busy || !json.trim()} onClick={() => void preview()}>
                미리보기
              </button>
              <button className="button-secondary" disabled={busy} onClick={onClose}>
                취소
              </button>
            </div>
          </>
        ) : (
          <>
            <ChangeSetPreview
              items={items}
              title="import 계획"
              approveLabel="선택 항목 import"
              onApprove={(paths) => void apply(paths)}
              onReject={(_paths) => {
                setPlan(null);
              }}
              onCancel={() => setPlan(null)}
            />
          </>
        )}
      </div>
    </div>
  );
}
