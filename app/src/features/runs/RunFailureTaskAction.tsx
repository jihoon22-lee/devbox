import { useEffect, useRef, useState } from "react";
import { RpcError } from "../../rpc/client";
import type { Run } from "../../rpc/gen/rpc";
import { messageText } from "../../rpc/messages";
import { useRpc } from "../../rpc/RpcProvider";
import { Button } from "../../ui";
import { readFailureLogs } from "../agents/detail/failureLogs";
import { useAgentsUi } from "../agents/ui";
import s from "./RunsPage.module.css";
import { resultLabel } from "./runPresentation";

// What the failure was, never the definition's command: the masked log tail below it says the rest.
const summary = (run: Run) =>
  `실행 ${run.defId}이(가) 실패했습니다(${resultLabel(run)}, 실행 ID ${run.id}). 아래 가려진 실행 로그 끝을 보고 원인을 고쳐 주세요.\n`;
function reason(error: unknown) {
  if (error instanceof RpcError) return messageText(error.code, error.detail, error.diagnosticId);
  return error instanceof Error ? error.message : "실행 로그를 읽지 못했습니다.";
}
/**
 * [에이전트로 고치기]: opens the new task form with the failed run's summary and log tail. Nothing starts until [시작].
 * The page keys this by run id and shows it only while the run is failed, so either change unmounts it and drops a
 * read still on its way.
 */
export function RunFailureTaskAction({ run }: { run: Run }) {
  const client = useRpc();
  const reading = useRef<AbortController | null>(null);
  const shown = useRef(run.id);
  shown.current = run.id;
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string>();
  useEffect(() => () => reading.current?.abort(), []);
  const draft = async () => {
    if (reading.current) return;
    const controller = new AbortController();
    reading.current = controller;
    setBusy(true);
    setProblem(undefined);
    try {
      const prompt = await readFailureLogs(client, run.id, controller.signal, summary(run));
      // A cancel only asks the daemon to stop, so a read for a run no longer shown can still complete.
      if (controller.signal.aborted || shown.current !== run.id) return;
      useAgentsUi
        .getState()
        .setCreating(true, { title: `Fix failed run ${run.defId}`, projectId: run.projectId, prompt });
    } catch (error) {
      if (!controller.signal.aborted) setProblem(reason(error));
    } finally {
      if (reading.current === controller) {
        reading.current = null;
        setBusy(false);
      }
    }
  };
  return (
    <div className={s.actions}>
      <Button size="sm" disabled={busy} onClick={() => void draft()}>
        에이전트로 고치기
      </Button>
      {problem && <span role="alert">{problem}</span>}
    </div>
  );
}
