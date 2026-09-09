import { useEffect, useRef, useState } from "react";
import {
  applyLspRecovery, cancelLspRecovery, listLspRecovery, previewLspRecovery,
  type LspRecoveryListing, type LspRecoveryPreview, type LspRecoveryResult,
} from "../api";

export default function LspRecoveryReview({ disabled }: { disabled: boolean }) {
  const [listing, setListing] = useState<LspRecoveryListing | null>(null);
  const [preview, setPreview] = useState<LspRecoveryPreview | null>(null);
  const [result, setResult] = useState<LspRecoveryResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const pending = useRef<string | null>(null);
  const generation = useRef(0);
  const working = useRef(false);
  useEffect(() => {
    generation.current += 1;
    return () => {
      generation.current += 1;
      const id = pending.current;
      pending.current = null;
      if (id) void cancelLspRecovery(id).catch(() => undefined);
    };
  }, []);
  const run = (operation: (current: () => boolean) => Promise<void>) => {
    if (working.current) return;
    working.current = true;
    const token = generation.current;
    const current = () => token === generation.current;
    setBusy(true); setError(null); setResult(null);
    void operation(current).catch(cause => {
      if (current()) setError(cause instanceof Error ? cause.message : String(cause));
    }).finally(() => {
      working.current = false;
      if (current()) setBusy(false);
    });
  };
  const list = () => run(async current => {
    const next = await listLspRecovery();
    if (current()) setListing(next);
  });
  const review = (id: string) => run(async current => {
    const next = await previewLspRecovery(id);
    if (!current()) { await cancelLspRecovery(next.previewId); return; }
    pending.current = next.previewId;
    setPreview(next);
  });
  const cancel = () => run(async current => {
    const id = pending.current;
    pending.current = null;
    if (id) await cancelLspRecovery(id);
    if (current()) setPreview(null);
  });
  const apply = () => run(async current => {
    const id = pending.current;
    if (!id) return;
    pending.current = null;
    setPreview(null); setListing(null);
    const next = await applyLspRecovery(id);
    if (current()) setResult(next);
  });
  return <section className="lsp-status-section" aria-label="이름 변경 복구">
    <h3>이름 변경 복구</h3>
    <p>중단된 이름 변경의 원본 백업을 검토하고 복원합니다. 대상 파일 탭을 닫은 뒤 검토하세요. 검토와 적용 시 언어 서버를 종료합니다.</p>
    {error && <p role="alert" className="lsp-error">{error}</p>}
    {!preview && <button type="button" className="toolbar-button" disabled={disabled || busy} onClick={list}>복구 기록 확인</button>}
    {listing && !preview && <div>
      {listing.records.length === 0 && <p role="status">남은 복구 기록이 없습니다.</p>}
      {listing.truncated && <p>기록 표시 한도를 넘었습니다. 표시되지 않은 기록은 보존됩니다.</p>}
      {listing.records.map((record, index) => <p key={record.journalId}>
        {record.available ? `기록 ${index + 1} · ${record.files}개 파일` : `기록 ${index + 1} · 손상된 기록을 보존했습니다`}
        <button type="button" className="toolbar-button" disabled={disabled || busy || !record.available} onClick={() => review(record.journalId)}>기록 {index + 1} 복구 검토</button>
      </p>)}
    </div>}
    {preview && <div className="lsp-execution-review">
      {preview.files.map(file => <details key={file.path}>
        <summary>{file.path} · {file.restore ? "원본으로 복원" : "이미 원본과 일치"}</summary>
        <p>현재 파일 ({file.currentSize} bytes)</p><pre>{file.current}</pre>
        <p>원본 백업 ({file.originalSize} bytes)</p><pre>{file.original}</pre>
      </details>)}
      <p>검토 이후 파일이 바뀌면 복원을 중단하고 백업을 보존합니다.</p>
      <button type="button" className="toolbar-button selected" disabled={disabled || busy} onClick={apply}>검토한 원본 복원</button>
      <button type="button" className="toolbar-button" disabled={busy} onClick={cancel}>복구 검토 취소</button>
    </div>}
    {result && <div role="status">
      <p>{result.complete ? "원본 복원이 완료되었습니다. 파일을 다시 열어 확인하세요." : "일부 복원을 완료하지 못했습니다. 백업을 보존했습니다."}</p>
      {result.restored.length > 0 && <ul>{result.restored.map(path => <li key={path}>{path}</li>)}</ul>}
      {result.error && <p>{result.error}</p>}
      {result.cleanupPending && <p>복구 기록 정리가 남아 있습니다.</p>}
    </div>}
  </section>;
}
