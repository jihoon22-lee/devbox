import { bindTypedCall } from "@devbox/workspace-features/typed";
import type { WorkspaceRuntimeCall } from "@devbox/workspace-features/generated/WorkspaceRuntimeCall";
import type { RuntimeResults } from "@devbox/workspace-features/generated/runtime-results";
import { useEffect, useRef, useState } from "react";
import { useIncomingReview } from "@devbox/product-shell/incoming";
import type { Description } from "@devbox/product-shell/api";
import { componentCall } from "./native";
const runtimeCall = (description: Description) =>
  bindTypedCall<WorkspaceRuntimeCall, RuntimeResults>((method, args) =>
    componentCall(description, "workspace.runtime", method, args, "tasks"),
  );
interface Run {
  id: string;
  jobId: string;
  status: string;
  logsAvailable: boolean;
  startedAt: number | null;
  createdAt: number;
}
export default function IncomingRuntimeReview({ description }: { description: Description }) {
  const { review, clear } = useIncomingReview();
  const actionGeneration = useRef(0);
  const operationId = review?.operationId;
  const [run, setRun] = useState<Run | null>(null),
    [issue, setIssue] = useState(""),
    [busy, setBusy] = useState(false);
  const id =
    review?.route === "tasks" && review.target.kind === "entity" && review.target.entity === "run"
      ? review.target.id
      : null;
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new operation for the same run must invalidate the previous review's pending actions.
  useEffect(() => {
    let current = true;
    actionGeneration.current += 1;
    setRun(null);
    setIssue("");
    setBusy(false);
    if (id)
      void runtimeCall(description)("get_run", { id })
        .then((value) => {
          if (current) {
            if (value?.id === id) setRun(value);
            else setIssue("실행 기록이 더 이상 없습니다.");
          }
        })
        .catch(() => {
          if (current) setIssue("실행 기록을 확인하지 못했습니다.");
        });
    return () => {
      current = false;
      actionGeneration.current += 1;
    };
  }, [description, id, operationId]);
  if (!id) return null;
  const openLog = async (stream: "stdout" | "stderr") => {
    const generation = actionGeneration.current;
    setBusy(true);
    setIssue("");
    try {
      await runtimeCall(description)("open_run_log_in_log_lens", { runId: id, stream });
      if (actionGeneration.current === generation) clear();
    } catch {
      if (actionGeneration.current === generation) setIssue("현재 실행 로그를 열지 못했습니다.");
    } finally {
      if (actionGeneration.current === generation) setBusy(false);
    }
  };
  const labels: Record<string, string> = {
    queued: "대기",
    starting: "시작 중",
    running: "실행 중",
    stopping: "종료 중",
    succeeded: "성공",
    failed: "실패",
    cancelled: "취소",
    skipped: "건너뜀",
  };
  return (
    <section aria-label="받은 실행 기록">
      <h2>{review?.label}</h2>
      {run ? (
        <>
          <p>
            {labels[run.status] ?? "상태 확인 필요"} · {new Date(run.startedAt ?? run.createdAt).toLocaleString()}
          </p>
          <button disabled={busy || !run.logsAvailable} onClick={() => void openLog("stdout")}>
            출력 로그 열기
          </button>{" "}
          <button disabled={busy || !run.logsAvailable} onClick={() => void openLog("stderr")}>
            오류 로그 열기
          </button>
        </>
      ) : (
        !issue && <p role="status">실행 기록을 확인하고 있습니다…</p>
      )}
      {issue && <p role="alert">{issue}</p>}{" "}
      <button disabled={busy} onClick={clear}>
        닫기
      </button>
    </section>
  );
}
