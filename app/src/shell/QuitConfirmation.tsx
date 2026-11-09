import { useEffect, useRef } from "react";
import { Button, Dialog } from "../ui";

/** What a full quit would end or drop. */
export interface QuitReasons {
  /** Open agent tasks; `null` while the count is not known. */
  tasks: number | null;
  /** The hidden quick capture window holds text the inbox does not have. */
  captureDraft: boolean;
}

function sentences({ tasks, captureDraft }: QuitReasons): string[] {
  const out: string[] = [];
  if (tasks === null) out.push("실행 중인 작업을 확인하지 못했습니다.");
  else if (tasks > 0) out.push(`에이전트 ${tasks}개가 실행 중입니다.`);
  out.push("앱을 끄면 에이전트 작업·서비스·터미널이 WSL과 함께 종료될 수 있습니다.");
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
          <Button
            variant="danger"
            disabled={busy}
            onClick={(event) => {
              // A double click's second click may land after [종료] turned into [버리고 종료].
              // Key presses (Enter, Space) have detail 0 and still confirm.
              if (event.detail <= 1) onConfirm();
            }}
          >
            {draft ? "버리고 종료" : "종료"}
          </Button>
        </>
      }
    />
  );
}
