import { useEffect, useRef } from "react";
import { Button, Dialog } from "../ui";

/** What a full quit would end or drop. */
export interface QuitReasons {
  /** Open agent tasks; `null` while the count is not known. */
  tasks: number | null;
  /** The hidden quick capture window holds text the inbox does not have. */
  captureDraft: boolean;
}

/** Nothing would be lost, so the app quits without asking. */
export const quitsDirectly = (reasons: QuitReasons) => reasons.tasks === 0 && !reasons.captureDraft;

function sentences({ tasks, captureDraft }: QuitReasons): string[] {
  const out: string[] = [];
  if (tasks !== 0)
    out.push(
      tasks === null ? "실행 중인 에이전트 작업을 확인하지 못했습니다." : `에이전트 ${tasks}개가 실행 중입니다.`,
      "앱을 끄면 WSL이 약 15초 뒤 멈추면서 함께 종료될 수 있습니다.",
    );
  if (captureDraft) out.push("빠른 캡처에 저장하지 않은 초안이 있습니다. 종료하면 사라집니다.");
  return out;
}

/** The tray quit's confirmation; [취소] has the first focus. [캡처 열기] shows the draft instead of quitting. */
export function QuitConfirmation({
  reasons,
  busy,
  onCancel,
  onOpenCapture,
  onConfirm,
}: {
  reasons: QuitReasons | undefined;
  busy: boolean;
  onCancel(): void;
  onOpenCapture(): void;
  onConfirm(): void;
}) {
  const cancel = useRef<HTMLButtonElement>(null);
  const draft = reasons?.captureDraft === true;
  // A draft found only when [종료] is pressed moves the focus back to [취소], as on opening with one.
  useEffect(() => {
    if (draft) cancel.current?.focus({ preventScroll: true });
  }, [draft]);
  return (
    <Dialog
      open={reasons !== undefined}
      onOpenAutoFocus={(event) => {
        event.preventDefault();
        cancel.current?.focus({ preventScroll: true });
      }}
      onOpenChange={(open) => !open && onCancel()}
      title="devbox 종료"
      description={reasons && sentences(reasons).join(" ")}
      footer={
        <>
          <Button ref={cancel} onClick={onCancel}>
            취소
          </Button>
          {draft && (
            <Button disabled={busy} onClick={onOpenCapture}>
              캡처 열기
            </Button>
          )}
          <Button variant="danger" disabled={busy} onClick={onConfirm}>
            {draft ? "버리고 종료" : "종료"}
          </Button>
        </>
      }
    />
  );
}
