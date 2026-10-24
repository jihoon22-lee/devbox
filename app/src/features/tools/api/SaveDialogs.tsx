/** 저장 대화상자 두 개: 새 요청 이름, 직접 입력한 비밀 안내. */
import { useEffect, useState } from "react";
import { Button, Dialog } from "../../../ui";
import s from "./Api.module.css";

export function SaveAsNewDialog({
  open,
  initial,
  onCancel,
  onSave,
}: {
  open: boolean;
  initial: string;
  onCancel: () => void;
  onSave: (name: string) => void;
}) {
  const [name, setName] = useState(initial);
  useEffect(() => {
    if (open) setName(initial);
  }, [open, initial]);
  const valid = name.trim().length > 0;
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => !next && onCancel()}
      title="새 요청으로 저장"
      footer={
        <>
          <Button onClick={onCancel}>취소</Button>
          <Button variant="primary" disabled={!valid} onClick={() => onSave(name.trim())}>
            저장
          </Button>
        </>
      }
    >
      <label className={s.table}>
        요청 이름
        <input
          className={s.input}
          value={name}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && valid && !event.nativeEvent.isComposing) onSave(name.trim());
          }}
        />
      </label>
    </Dialog>
  );
}

export function MaskedDialog({
  count,
  onCancel,
  onMask,
  onMove,
}: {
  count: number | null;
  onCancel: () => void;
  onMask: () => void;
  onMove: () => void;
}) {
  return (
    <Dialog
      open={count !== null}
      onOpenChange={(next) => !next && onCancel()}
      title={`직접 입력한 비밀 ${count ?? 0}개는 파일에 저장하지 않습니다`}
      description="비밀로 옮기면 이 PC 비밀에 저장하고 필드는 {{secret:이름}} 참조로 바꿉니다. 가리고 저장하면 파일에는 [REDACTED]가 남고 다시 입력해야 보낼 수 있습니다."
      footer={
        <>
          <Button onClick={onCancel}>취소</Button>
          <Button onClick={onMask}>가리고 저장</Button>
          <Button variant="primary" onClick={onMove}>
            비밀로 옮기기
          </Button>
        </>
      }
    />
  );
}
