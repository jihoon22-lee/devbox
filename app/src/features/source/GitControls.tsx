import { useState } from "react";
import { RpcError } from "../../rpc/client";
import type { SourceTarget } from "../../rpc/gen/rpc";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Banner, Button } from "../../ui";
import {
  gitError,
  inspectUncertain,
  operationMissing,
  prepareRemote,
  retryUncertain,
  runGit,
  sourceTargetKey,
  updateGit,
  useSourceGit,
} from "./gitState";
import { receiveOperation } from "./SourceGitSession";
export function GitControls({ target }: { target: SourceTarget }) {
  const { client, state } = useRpcSession();
  const [force, setForce] = useState(false),
    [rebase, setRebase] = useState(false);
  const entry = useSourceGit((state) => state.entries[sourceTargetKey(target)]);
  const operation = entry?.operation;
  const cancel = async () => {
    if (!operation || operation.state !== "running") return;
    const daemonId = entry?.operationDaemonId;
    try {
      const result = await client.call(
        "git.operation_cancel",
        { target: operation.target, operationId: operation.id },
        { clientRequestId: crypto.randomUUID() },
      );
      receiveOperation(result, client.daemonId);
    } catch (error) {
      if (error instanceof RpcError && error.code === "git.invalid_target") operationMissing(operation, daemonId);
      else {
        const current = useSourceGit.getState().entries[sourceTargetKey(target)];
        if (
          current?.operation?.id === operation.id &&
          current.operationDaemonId === daemonId &&
          current.operation.state === "running" &&
          !current.lostOperation
        )
          updateGit(target, { error: gitError(error) });
      }
    }
  };
  return (
    <section aria-label="Git 작업">
      <div>
        <Button
          disabled={state !== "ready" || entry?.busy || !!entry?.uncertain}
          onClick={() => void runGit(client, { method: "git.fetch", params: { target } })}
        >
          fetch
        </Button>
        <Button
          disabled={state !== "ready" || entry?.busy || !!entry?.uncertain}
          onClick={() => void prepareRemote(client, target, "git.push", force)}
        >
          push…
        </Button>
        <Button
          disabled={state !== "ready" || entry?.busy || !!entry?.uncertain}
          onClick={() => void prepareRemote(client, target, "git.pull", rebase)}
        >
          pull…
        </Button>
        <label>
          <input type="checkbox" checked={force} onChange={(event) => setForce(event.target.checked)} />
          force-with-lease
        </label>
        <label>
          <input type="checkbox" checked={rebase} onChange={(event) => setRebase(event.target.checked)} />
          pull 시 rebase
        </label>
      </div>
      {entry?.notice && <p role="status">{entry.notice}</p>}
      {entry?.error && <Banner tone="danger">{entry.error}</Banner>}
      {(entry?.uncertain || entry?.lostOperation) && (
        <Banner tone="warn">
          완료 여부를 확인할 때까지 새 Git 작업을 보류합니다.
          {entry.uncertain && (
            <Button disabled={entry.busy} onClick={() => void retryUncertain(client, target)}>
              같은 요청 결과 다시 확인
            </Button>
          )}
          <Button onClick={() => void inspectUncertain(client, target)}>현재 Git 상태 읽기</Button>
          {entry.observed && (
            <div>
              <p>
                현재 {entry.observed.summary.branch ?? "분리된 HEAD"} ·{" "}
                {entry.observed.expect.state.headOid ?? "커밋 없음"} · 변경 {entry.observed.files.length}개
              </p>
              <Button
                onClick={() => {
                  updateGit(target, {
                    uncertain: undefined,
                    ...(entry.lostOperation
                      ? { busy: false, operation: undefined, operationDaemonId: undefined, lostOperation: undefined }
                      : {}),
                    observed: undefined,
                    error: undefined,
                    notice: "현재 상태를 확인했습니다. 새 작업을 진행할 수 있습니다.",
                  });
                  const confirmation = useSourceGit.getState().confirmation;
                  if (confirmation && sourceTargetKey(confirmation.action.params.target) === sourceTargetKey(target))
                    useSourceGit.setState({ confirmation: undefined });
                }}
              >
                이 상태를 확인하고 새 작업 허용
              </Button>
            </div>
          )}
        </Banner>
      )}
      {operation && (
        <p role="status">
          {entry?.lostOperation && operation.state === "running"
            ? "원격 작업 완료 여부 불명"
            : operation.state === "running"
              ? "원격 작업 실행 중"
              : operation.state === "succeeded"
                ? "원격 작업 완료"
                : operation.state === "cancelled"
                  ? "원격 작업 취소됨"
                  : "원격 작업 실패"}
          {operation.completionUnknown
            ? entry?.lostOperation
              ? " · 원격 반영 여부를 알 수 없습니다. 현재 Git 상태를 읽고 확인하세요."
              : " · 원격 반영 여부를 알 수 없습니다. fetch 후 확인하세요."
            : ""}
        </p>
      )}
      {operation?.state === "running" && !entry?.lostOperation && (
        <Button onClick={() => void cancel()}>원격 작업 취소</Button>
      )}
    </section>
  );
}
