import { useState } from "react";
export default function CloseReview({
  reasons,
  filesDirty,
  onSave,
  onDiscard,
  onCancel,
  onReturn,
}: {
  reasons: string[];
  filesDirty: boolean;
  onSave(): Promise<void>;
  onDiscard(): Promise<void>;
  onCancel(): Promise<void>;
  onReturn(): void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "종료 준비를 완료하지 못했습니다.");
    } finally {
      setBusy(false);
    }
  };
  return (
    <section role="dialog" aria-modal="true" aria-label="Workspace 종료 검토">
      <h2>Workspace를 닫기 전에 변경을 확인하세요</h2>
      <p>{reasons.length ? reasons.join(", ") : "마지막 복구 기록을 확인한 뒤 종료할 수 있습니다."}</p>
      {error && <p role="alert">{error}</p>}
      <button type="button" disabled={busy} onClick={() => void run(onSave)}>
        {filesDirty ? "파일 저장 후 종료" : "종료"}
      </button>
      {filesDirty && (
        <button type="button" disabled={busy} onClick={() => void run(onDiscard)}>
          파일 변경 폐기 후 종료
        </button>
      )}
      <button type="button" disabled={busy} onClick={onReturn}>
        편집 화면으로 돌아가기
      </button>
      <button type="button" disabled={busy} onClick={() => void run(onCancel)}>
        종료 취소
      </button>
    </section>
  );
}
