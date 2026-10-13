import { useRef, useState } from "react";
import { Button, Dialog } from "../ui";

export function FirstRun({
  onDone,
  onCancel,
}: {
  onDone: (choice: { autostart: boolean }) => Promise<void>;
  onCancel: () => void;
}) {
  const [autostart, setAutostart] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const cancel = useRef<HTMLButtonElement>(null);
  const pending = useRef(false);
  const submit = async () => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError(false);
    try {
      await onDone({ autostart });
    } catch {
      setError(true);
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };
  return (
    <Dialog
      open
      title="devbox 시작하기"
      onOpenChange={(open) => !open && !pending.current && onCancel()}
      onOpenAutoFocus={(event) => {
        event.preventDefault();
        cancel.current?.focus();
      }}
      footer={
        <>
          <Button ref={cancel} disabled={busy} onClick={onCancel}>
            나중에
          </Button>
          <Button variant="primary" disabled={busy} onClick={() => void submit()}>
            시작하기
          </Button>
        </>
      }
    >
      <p>트레이에서 실행되는 동안 devbox는 WSL 연결을 유지하여 작업이 유휴 상태에서도 계속 실행되도록 합니다.</p>
      <p>
        앱을 완전히 종료하면 WSL의 instanceIdleTimeout 설정에 따라 약 15초 뒤 배포판과 작업이 멈출 수 있습니다. 계속
        실행하려면 이 값을 -1로 설정할 수 있습니다.
      </p>
      <label>
        <input
          type="checkbox"
          checked={autostart}
          disabled={busy}
          onChange={(event) => setAutostart(event.target.checked)}
        />{" "}
        Windows 시작 시 devbox 실행
      </label>
      {error && <p role="alert">설정을 저장하지 못했습니다. 다시 시도하세요.</p>}
    </Dialog>
  );
}
