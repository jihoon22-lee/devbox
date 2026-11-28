import { useRef } from "react";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation } from "../../rpc/query";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Button } from "../../ui";
import { useAgentResult } from "./detail/data";
import type { AgentTask } from "./model";

export function ConversationRecovery({
  task,
  onStart,
  disabled,
}: {
  task: AgentTask;
  onStart?: () => void;
  disabled?: boolean;
}) {
  const recover = useRpcMutation("agents.recover_conversation");
  const { state } = useRpcSession();
  const apply = useAgentResult();
  const pending = useRef(false);
  const blocked = disabled || pending.current || recover.isPending || state !== "ready";
  const choose = async (action: "startNew" | "archive") => {
    if (pending.current || blocked) return;
    pending.current = true;
    try {
      apply(await recover.mutateAsync({ id: task.id, updatedMs: task.updatedMs, action }));
      if (action === "startNew") onStart?.();
    } catch {
      /* The typed failure stays next to the available choices. */
    } finally {
      pending.current = false;
    }
  };
  const error = recover.error && messageFor(recover.error.code, recover.error.detail);
  return (
    <>
      <Button size="sm" disabled={blocked} onClick={() => void choose("startNew")}>
        새 대화로 시작
      </Button>
      <Button
        size="sm"
        disabled={blocked}
        title="작업 폴더와 브랜치를 보존하고 목록에서 숨깁니다."
        onClick={() => void choose("archive")}
      >
        목록에서 정리
      </Button>
      {error && (
        <p role="alert">
          {error.title} {error.action}
        </p>
      )}
    </>
  );
}
