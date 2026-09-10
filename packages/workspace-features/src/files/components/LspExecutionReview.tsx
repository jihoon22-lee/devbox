import { useEffect, useRef, useState } from "react";
import { approveLspExecution, cancelLspExecutionReview, previewLspExecution, revokeLspExecution, type LspExecutionPreview } from "../api";

export default function LspExecutionReview({ disabled, nativeRevision }: { disabled: boolean; nativeRevision: string | null }) {
  const [preview, setPreview] = useState<LspExecutionPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  const pending = useRef<string | null>(null);
  const working = useRef(false);

  useEffect(() => {
    generation.current += 1;
    return () => {
      generation.current += 1;
      const id = pending.current;
      pending.current = null;
      if (id) void cancelLspExecutionReview(id).catch(() => undefined);
    };
  }, []);

  const run = (operation: (current: () => boolean) => Promise<void>) => {
    if (working.current) return;
    working.current = true;
    setBusy(true);
    setError(null);
    setMessage(null);
    const token = generation.current;
    const current = () => token === generation.current;
    void operation(current).catch(cause => {
      if (current()) setError(cause instanceof Error ? cause.message : String(cause));
    }).finally(() => {
      working.current = false;
      if (current()) setBusy(false);
    });
  };
  const review = () => run(async current => {
    const next = await previewLspExecution();
    if (!current() || next.configRevision !== nativeRevision) {
      await cancelLspExecutionReview(next.previewId);
      if (current()) throw new Error("설정이 변경되었습니다. 저장한 설정을 다시 검토하세요.");
      return;
    }
    pending.current = next.previewId;
    setPreview(next);
  });
  const cancel = () => run(async current => {
    const id = pending.current;
    pending.current = null;
    if (id) await cancelLspExecutionReview(id);
    if (current()) setPreview(null);
  });
  const approve = () => run(async current => {
    const id = pending.current;
    if (!id) return;
    pending.current = null;
    // Approval consumes the review even when the native evidence changed.
    setPreview(null);
    await approveLspExecution(id);
    if (current()) setMessage("검토한 설정의 실행을 승인했습니다. 언어별 시작 버튼으로 실행하세요.");
  });
  const revoke = () => run(async current => {
    const id = pending.current;
    pending.current = null;
    if (id) await cancelLspExecutionReview(id);
    await revokeLspExecution();
    if (current()) { setPreview(null); setMessage("언어 서버를 종료하고 실행 승인을 해제했습니다."); }
  });

  return <section className="lsp-status-section" aria-label="언어 서버 실행 승인">
    <h3>언어 서버 실행 승인</h3>
    <p>설정을 저장한 뒤 실행 파일, 인자와 작업 폴더를 검토하세요. 실행 근거가 바뀌면 다시 승인이 필요합니다.</p>
    {error && <p role="alert" className="lsp-error">{error}</p>}
    {message && <p role="status">{message}</p>}
    {!preview && <button type="button" className="toolbar-button" disabled={disabled || busy || !nativeRevision} onClick={review}>실행 설정 검토</button>}
    <button type="button" className="toolbar-button" disabled={busy || !nativeRevision} onClick={revoke}>실행 승인 해제</button>
    {preview && <div className="lsp-execution-review">
      <p>작업 폴더: <code>{preview.workspaceRoot}</code></p>
      {preview.commands.map(command => <article key={command.languageId}>
        <h4>{command.languageId}</h4>
        <p>실행 파일: <code>{command.executable}</code></p>
        {command.runtime && <p>런타임: <code>{command.runtime}</code></p>}
        <p>인자</p><pre>{command.args.length ? command.args.join("\n") : "없음"}</pre>
      </article>)}
      <details><summary>프로세스에 전달할 환경 변수</summary>
        <dl>{Object.entries(preview.environment ?? {}).map(([name, value]) => <div key={name}><dt>{name}</dt><dd><code>{value}</code></dd></div>)}</dl>
        {preview.environmentKeys && <p>변수 이름: {preview.environmentKeys.join(", ")}</p>}
      </details>
      <button type="button" className="toolbar-button selected" disabled={disabled || busy} onClick={approve}>이 설정의 실행 승인</button>
      <button type="button" className="toolbar-button" disabled={busy} onClick={cancel}>검토 취소</button>
    </div>}
  </section>;
}
