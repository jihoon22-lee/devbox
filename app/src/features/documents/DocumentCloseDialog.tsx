import { useRef } from "react";
import { Button, Dialog } from "../../ui";
import { useClosePrompt } from "./closePrompt";

/** Outside the closing editor's inert subtree so a native close can still be cancelled. */
export function DocumentCloseDialog() {
  const request = useClosePrompt((s) => s.request);
  const cancel = useRef<HTMLButtonElement>(null);
  if (!request) return null;
  const recovery = request.docs.some((doc) => doc.state === "Recovering");
  const readOnly = request.docs.some((doc) => doc.state === "ReadOnly");
  const canSave = request.docs.every(
    (doc) => ["Dirty", "Saving", "Clean", "Orphaned"].includes(doc.state) && !!doc.leaseId,
  );
  return (
    <Dialog
      open
      onOpenChange={(open) => !open && request.answer()}
      title="저장하지 않은 파일"
      description="변경을 어떻게 남길지 선택하세요. 초안을 유지하면 다음에 파일을 열 때 복구할 수 있습니다."
      onOpenAutoFocus={(event) => {
        event.preventDefault();
        cancel.current?.focus();
      }}
      footer={
        <>
          <Button ref={cancel} onClick={() => request.answer()}>
            취소
          </Button>
          <Button disabled={recovery || readOnly} onClick={() => request.answer("keep")}>
            초안 유지
          </Button>
          <Button variant="danger" onClick={() => request.answer(recovery ? "discardAll" : "discard")}>
            {recovery ? "복구본까지 버리기" : "버림"}
          </Button>
          <Button variant="primary" disabled={!canSave} onClick={() => request.answer("save")}>
            저장
          </Button>
        </>
      }
    >
      <ul>
        {request.docs.map((doc) => (
          <li key={doc.id}>{doc.key.path}</li>
        ))}
      </ul>
      {readOnly && <p>다른 창의 초안은 바꾸지 않습니다. 편집기로 돌아가 여기로 가져온 뒤 저장할 수 있습니다.</p>}
      {recovery && <p>복구할 초안이 있습니다. 편집기로 돌아가 먼저 복원하거나 비교할 수 있습니다.</p>}
    </Dialog>
  );
}
