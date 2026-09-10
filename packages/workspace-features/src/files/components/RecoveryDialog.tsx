// 비정상 종료 후 미저장 버퍼 복구 다이얼로그 (§12.1).
// ChangeSetPreview(§12.5)를 사용해 항목별 복구/폐기를 선택한다.

import { useEffect, useRef, useState } from "react";
import { applyRecovery, applyRecoveryPreview, cancelRecoveryPreview, prepareRecovery, discardRecovery, loadRecoveryState, openFile, type RecoveryEntry } from "../api";
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
  const nativeRevision = useRef<string | undefined>(undefined);
  const [unavailable,setUnavailable] = useState<{path:string;issue:string}[]>([]);

  useEffect(() => {
    let active = true;
    const createdPreviews: string[] = [];
    setItems(null); setError(null);
    void (async () => {
      const loaded = await loadRecoveryState();
      const entries = loaded.entries;
      if (!active) return;
      nativeRevision.current = loaded.nativeRevision;
      setPaths(entries);
      const failed: {path:string;issue:string}[] = [];
      const built: ChangeSetItem[] = [];
      const approvals: Record<string,string> = {};
      for (const e of entries) {
        try {
        const preview = isProductHosted() ? await prepareRecovery(e.path) : null;
        if (preview) {
          if (!active) {void cancelRecoveryPreview(preview.previewId).catch(() => {}); return;}
          createdPreviews.push(preview.previewId);
        }
        const current = preview ? preview.before : await readCurrentText(e.path);
        if (preview) approvals[e.path] = preview.previewId;
        if (current === e.content) {
          if (preview) await cancelRecoveryPreview(preview.previewId);
          continue;
        }
        built.push({ path: e.path, before: current, after: e.content });
        } catch(cause) { failed.push({path:e.path,issue:cause instanceof Error?cause.message:String(cause)}); }
      }
      if (active) {setPreviews(approvals); setItems(built); setUnavailable(failed);}
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
          nativeRevision.current = await discardRecovery(e.path,nativeRevision.current);
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
        nativeRevision.current = await discardRecovery(p,nativeRevision.current);
      }
      onDone([]);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="recovery-dialog">
      <h2>복구할 미저장 변경</h2>
      <p className="recovery-note">
        이전에 저장하지 못한 버퍼입니다. 적용하려면 복구, 버리려면 폐기를 선택하세요.
      </p>
      {error && <div className="error" role="alert">{error}<button type="button" disabled={busy} onClick={() => setRevision(value => value + 1)}>미리보기 다시 확인</button></div>}
      {unavailable.length > 0 && <section aria-label="확인하지 못한 복구 문서"><p>다음 버퍼는 보존했습니다. 파일 접근을 확인한 뒤 다시 검토해 주세요.</p><ul>{unavailable.map(item => <li key={item.path}>{item.path} · {item.issue}</li>)}</ul><button type="button" disabled={busy} onClick={() => setRevision(value => value + 1)}>복구 문서 다시 확인</button></section>}
      {items.length === 0 ? <><p>현재 적용할 수 있는 변경이 없습니다. 보관된 버퍼를 유지합니다.</p><button type="button" onClick={() => onDone([])}>닫기</button></> : <ChangeSetPreview
        items={items}
        disabled={busy}
        title="미저장 버퍼"
        approveLabel="복구"
        onApprove={(p) => void approve(p)}
        onReject={(p) => void reject(p)}
        onCancel={busy ? undefined : () => onDone([])}
      />}
    </div>
  );
}
