import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { RpcError } from "../../../rpc/client";
import { messageFor } from "../../../rpc/messages";
import { useRpcMutation, useRpcQuery, useTopicInvalidation } from "../../../rpc/query";
import { useRpcSession } from "../../../rpc/RpcProvider";
import { openFileLocation } from "../../../shell/fileLocation";
import { Button, EmptyState, Tooltip } from "../../../ui";
import { useProjectTrust } from "../../projects/useProjectTrust";
import { LogView } from "../../runs/LogView";
import { resultLabel } from "../../runs/runPresentation";
import type { AgentTask } from "../model";
import s from "./Detail.module.css";
import { useAgentResult } from "./data";
import { readFailureLogs } from "./failureLogs";
import { QueryError } from "./QueryError";
export const useAgentRuns = create<{
  runs: Record<string, string>;
  set: (id: string, runId: string) => void;
}>((set, get) => ({
  runs: {},
  set: (id, runId) => set({ runs: { ...get().runs, [id]: runId } }),
}));
export function RunTab({ task }: { task: AgentTask }) {
  const id = useAgentRuns((state) => state.runs[task.id]);
  const test = useRpcMutation("agents.test");
  const send = useRpcMutation("agents.send");
  const updateAgent = useAgentResult();
  const trust = useProjectTrust(`${task.id}:${task.projectId}:${task.state}`);
  const pending = useRef(false);
  const sending = useRef<AbortController | null>(null);
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<string | null>(null);
  const [sent, setSent] = useState<string | null>(null);
  const { client, state } = useRpcSession();
  const config = useRpcQuery("projects.config", { id: task.projectId });
  const detail = useRpcQuery("runtime.get", { runId: id ?? "" }, { enabled: !!id });
  useTopicInvalidation("runtime.changed", [["runtime.get", { runId: id ?? "" }]]);
  const lifetime = `${task.id}:${task.state === "closed"}:${id ?? ""}`;
  useEffect(() => {
    void lifetime;
    sending.current?.abort();
    sending.current = null;
    setBusy(false);
    setFeedback(null);
    return () => sending.current?.abort();
  }, [lifetime]);
  const hasTest = !!config.data?.config.agent.test && !config.data.error;
  const run = async () => {
    if (pending.current || task.state === "closed") return;
    pending.current = true;
    try {
      if (
        !(await trust.ensure({
          id: task.projectId,
          operation: "test",
          agentId: task.id,
          worktrees: [],
          worktree: null,
        }))
      )
        return;
      const result = await test.mutateAsync({ id: task.id });
      useAgentRuns.getState().set(task.id, result.runId);
    } catch {
      /* Typed error remains visible; ambiguous retries preserve request identity. */
    } finally {
      pending.current = false;
    }
  };
  const sendLogs = async () => {
    if (!id || task.state === "closed" || sending.current || sent === id) return;
    const controller = new AbortController();
    sending.current = controller;
    setBusy(true);
    setFeedback(null);
    try {
      const text = await readFailureLogs(client, id, controller.signal);
      controller.signal.throwIfAborted();
      updateAgent(await send.mutateAsync({ id: task.id, text }));
      setSent(id);
      setFeedback("실패 로그를 보냈습니다.");
    } catch (error) {
      if (controller.signal.aborted) return;
      if (error instanceof RpcError) {
        if (error.code === "agents.delivery_unknown") setSent(id);
        const message = messageFor(error.code, error.detail);
        setFeedback(`${message.title} ${message.action}`);
      } else setFeedback(error instanceof Error ? error.message : "실패 로그를 보내지 못했습니다.");
    } finally {
      if (sending.current === controller) {
        sending.current = null;
        setBusy(false);
      }
    }
  };
  const button = task.state !== "closed" && (
    <Tooltip content={hasTest ? "설정한 테스트를 실행합니다" : ".devbox/devbox.toml의 [agent] test를 설정하세요"}>
      <span>
        <Button
          disabled={!hasTest || state !== "ready" || test.isPending || trust.pending || busy}
          onClick={() => void run()}
        >
          {id ? "테스트 다시 실행" : "테스트 실행"}
        </Button>
      </span>
    </Tooltip>
  );
  return (
    <div className={s.runTab}>
      {trust.dialog}
      {trust.notice && <p role="alert">{trust.notice}</p>}
      <QueryError query={config} />
      {test.error && (
        <p role="alert">
          {messageFor(test.error.code, test.error.detail).title} {messageFor(test.error.code, test.error.detail).action}
          {test.error.diagnosticId && ` · 진단 ${test.error.diagnosticId}`}
        </p>
      )}
      {id ? (
        <>
          <p>
            실행 ID {id} · {detail.data ? resultLabel(detail.data.run) : "실행 확인 중"}
          </p>
          <QueryError query={detail} />
          {button}
          {detail.data?.run.state === "failed" && task.state !== "closed" && (
            <Button
              disabled={state !== "ready" || busy || send.isPending || test.isPending || sent === id}
              onClick={() => void sendLogs()}
            >
              에이전트에게 실패 로그 보내기
            </Button>
          )}
          {feedback && <p role="status">{feedback}</p>}
          <div className={s.runLogs}>
            <LogView
              sources={[{ kind: "run", runId: id }]}
              projectId={task.projectId}
              onOpenLocation={(location) => openFileLocation(client, task.projectId, location)}
            />
          </div>
        </>
      ) : (
        <EmptyState title="아직 테스트를 실행하지 않았습니다" action={button} />
      )}
    </div>
  );
}
