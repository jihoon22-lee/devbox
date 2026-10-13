import { useEffect, useRef } from "react";
import { messageFor } from "../../../rpc/messages";
import { useRpcMutation } from "../../../rpc/query";
import { useRpcSession } from "../../../rpc/RpcProvider";
import { Button, Kbd, TextArea } from "../../../ui";
import s from "./Detail.module.css";
import { useAgentResult } from "./data";
import { useDrafts } from "./drafts";

export { useDrafts } from "./drafts";
export function Compose({
  agentId,
  disabled,
  focusRequest,
}: {
  agentId: string;
  disabled?: boolean;
  focusRequest?: number;
}) {
  const text = useDrafts((state) => state.text[agentId] ?? "");
  const send = useRpcMutation("agents.send");
  const result = useAgentResult();
  const { state } = useRpcSession();
  const box = useRef<HTMLTextAreaElement>(null);
  const composing = useRef(false),
    queued = useRef(false),
    pending = useRef(false),
    enterDown = useRef(false);
  const lifetime = useRef(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: Changing the target cancels its queued composition intent.
  useEffect(() => {
    lifetime.current++;
    composing.current = false;
    queued.current = false;
    enterDown.current = false;
    return () => {
      lifetime.current++;
      queued.current = false;
    };
  }, [agentId]);
  useEffect(() => {
    if (focusRequest !== undefined) box.current?.focus();
  }, [focusRequest]);
  const submit = (value = box.current?.value ?? "") => {
    if (composing.current) {
      queued.current = true;
      return;
    }
    if (disabled || state !== "ready" || pending.current || !value.trim()) return;
    const capturedId = agentId;
    const drafts = useDrafts.getState();
    if ((drafts.text[agentId] ?? "") !== value) drafts.set(agentId, value);
    const revision = useDrafts.getState().revisions[agentId];
    pending.current = true;
    void (async () => {
      try {
        const task = await send.mutateAsync({ id: capturedId, text: value.trim() });
        result(task);
        if (useDrafts.getState().revisions[capturedId] === revision) useDrafts.getState().set(capturedId, "");
      } catch {
        /* The typed mutation error remains beside the draft. */
      } finally {
        pending.current = false;
      }
    })();
  };
  const error = send.error && messageFor(send.error.code, send.error.detail);
  return (
    <form
      className={s.compose}
      onSubmit={(event) => {
        event.preventDefault();
        if (queued.current) return;
        submit();
      }}
    >
      <TextArea
        ref={box}
        aria-label="에이전트에게 보낼 지시"
        placeholder="다음 지시를 쓰세요. Enter 줄바꿈, Ctrl+Enter 보내기"
        value={text}
        disabled={disabled}
        maxRows={10}
        onChange={(event) => useDrafts.getState().set(agentId, event.target.value)}
        onCompositionStart={() => {
          composing.current = true;
        }}
        onCompositionEnd={(event) => {
          composing.current = false;
          const value = event.currentTarget.value;
          useDrafts.getState().set(agentId, value);
          if (!queued.current) return;
          const version = lifetime.current;
          queueMicrotask(() => {
            if (version !== lifetime.current || !queued.current) return;
            queued.current = false;
            // Some browsers emit input immediately after compositionend.
            submit(box.current?.value ?? value);
          });
        }}
        onKeyUp={(event) => {
          if (event.key === "Enter") enterDown.current = false;
        }}
        onKeyDown={(event) => {
          if (event.key !== "Enter" || !event.ctrlKey) return;
          event.preventDefault();
          if (event.repeat || enterDown.current) return;
          enterDown.current = true;
          if (composing.current || event.nativeEvent.isComposing) queued.current = true;
          else if (!queued.current) submit(event.currentTarget.value);
        }}
      />
      <Button
        type="submit"
        variant="primary"
        disabled={disabled || state !== "ready" || !text.trim() || send.isPending}
      >
        보내기 <Kbd keys={["Ctrl", "Enter"]} />
      </Button>
      {error && (
        <p role="alert">
          {error.title} {error.action}
          {send.error?.diagnosticId && ` · 진단 ${send.error.diagnosticId}`}
        </p>
      )}
    </form>
  );
}
