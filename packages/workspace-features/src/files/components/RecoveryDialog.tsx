// 비정상 종료 후 미저장 버퍼 복구 다이얼로그 (§12.1).
// ChangeSetPreview(§12.5)를 사용해 항목별 복구/폐기를 선택한다.

import { useEffect, useState } from "react";
import { applyRecovery, applyRecoveryPreview, cancelRecoveryPreview, prepareRecovery, discardRecovery, loadRecovery, openFile, type RecoveryEntry } from "../api";
import { isProductHosted } from "../../transport";
import ChangeSetPreview, { type ChangeSetItem } from "./ChangeSetPreview";

interface Props {
  onDone: (recovered: string[]) => void;
}

function readCurrentText(path: string): Promise<string> {
  return openFile(path, null)
    .then((f) => f.text)
    .catch(() => "");
}

export default function RecoveryDialog({ onDone }: Props) {
  const [items, setItems] = useState<ChangeSetItem[] | null>(null);
  const [paths, setPaths] = useState<RecoveryEntry[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [previews, setPreviews] = useState<Record<string, string>>({});
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    let active = true;
    const createdPreviews: string[] = [];
    setItems(null); setError(null);
    void (async () => {
      const entries = await loadRecovery();
      if (!active) return;
      setPaths(entries);
      const built: ChangeSetItem[] = [];
      const approvals: Record<string,string> = {};
      for (const e of entries) {
        const preview = isProductHosted() ? await prepareRecovery(e.path) : null;
        if (preview) {
          if (!active) {void cancelRecoveryPreview(preview.previewId).catch(() => {}); return;}
          createdPreviews.push(preview.previewId);
        }
        const current = preview ? preview.before : await readCurrentText(e.path);
        if (preview) approvals[e.path] = preview.previewId;
        if (current === e.content) continue; // 원본과 동일하면 복구할 내용 없음
        built.push({ path: e.path, before: current, after: e.content });
      }
      if (active) {setPreviews(approvals); setItems(built);}
    })().catch(cause => {if (active) setError(cause instanceof Error ? cause.message : "복구 내용을 확인하지 못했습니다.");});
    return () => {
      active = false;
      for (const id of createdPreviews) void cancelRecoveryPreview(id).catch(() => {});
    };
  }, [revision]);

  if (items === null) {
    return (
      <div className="recovery-dialog">
        {error ? <><p role="alert">{error}</p><button type="button" onClick={() => setRevision(value => value + 1)}>다시 확인</button><button type="button" onClick={() => onDone([])}>닫기</button></> : <div className="recovery-message">복구 가능한 미저장 변경을 확인 중…</div>}
      </div>
    );
  }

  const approve = async (selectedPaths: string[]) => {
    setBusy(true);
    setError(null);
    try {
      const recovered: string[] = [];
      for (const e of paths) {
        if (selectedPaths.includes(e.path)) {
          if (isProductHosted()) {
            if (!previews[e.path]) throw new Error("복구 미리보기를 다시 확인해 주세요.");
            await applyRecoveryPreview(previews[e.path]);
          } else { await applyRecovery(e.path, e.content); }
          await discardRecovery(e.path);
          recovered.push(e.path);
        }
      }
      onDone(recovered);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const reject = async (selectedPaths: string[]) => {
    setBusy(true);
    setError(null);
    try {
      for (const p of selectedPaths) {
        await discardRecovery(p);
      }
      onDone([]);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const rejectAll = async () => {
    setBusy(true);
    setError(null);
    try {
      await discardRecovery(null);
      onDone([]);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  if (items.length === 0) {
    return (
      <div className="recovery-dialog">
        <div className="recovery-message">복구할 미저장 변경이 없습니다.</div>
        <div className="changeset-actions">
          <button className="btn" disabled={busy} onClick={() => void rejectAll()}>
            확인
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="recovery-dialog">
      <h2>복구할 미저장 변경</h2>
      <p className="recovery-note">
        이전에 저장하지 못한 버퍼입니다. 적용하려면 복구, 버리려면 폐기를 선택하세요.
      </p>
      {error && <div className="error" role="alert">{error}<button type="button" disabled={busy} onClick={() => setRevision(value => value + 1)}>미리보기 다시 확인</button></div>}
      <ChangeSetPreview
        items={items}
        disabled={busy}
        title="미저장 버퍼"
        approveLabel="복구"
        onApprove={(p) => void approve(p)}
        onReject={(p) => void reject(p)}
        onCancel={busy ? undefined : () => onDone([])}
      />
    </div>
  );
}
