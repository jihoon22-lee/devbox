import { useId } from "react";
import type { Run } from "../../rpc/gen/rpc";
import { useRpcQuery, useTopicInvalidation } from "../../rpc/query";
import { Button } from "../../ui";
import s from "./RunsPage.module.css";
import { resultLabel } from "./runPresentation";

/**
 * The runs of one definition among the project's latest 100, newest first; hidden when there is only one.
 * The page keeps this mounted while a picked run loads, so the select keeps its focus.
 */
export function PastRunPicker({
  projectId,
  defId,
  value,
  shown,
  onPick,
}: {
  projectId: string;
  defId: string;
  /** The selected run, which may still be loading. */
  value: string;
  /** The run on screen once its detail has loaded. */
  shown?: Run;
  onPick: (runId: string) => void;
}) {
  const hint = useId();
  const params = { projectId, limit: 100 };
  const recent = useRpcQuery("runtime.runs", params);
  useTopicInvalidation("runtime.changed", [["runtime.runs", params]]);
  const runs = (recent.data?.items ?? []).filter((item) => item.defId === defId);
  // The shown run stays an option when it is older than the latest 100, so the value stays valid.
  if (shown && !runs.some((item) => item.id === shown.id)) runs.push(shown);
  runs.sort((a, b) => b.startedMs - a.startedMs);
  const failed = recent.isError && shown !== undefined;
  if (runs.length < 2 && !failed) return null;
  return (
    <>
      {runs.length > 1 && (
        <label className={s.editorChoice}>
          지난 실행
          <select
            aria-label="지난 실행"
            aria-describedby={hint}
            value={value}
            onChange={(event) => onPick(event.target.value)}
          >
            {runs.map((item) => (
              <option key={item.id} value={item.id}>
                {`${new Date(item.startedMs).toLocaleString("ko-KR")} · ${resultLabel(item)}`}
              </option>
            ))}
          </select>
          <span id={hint}>최근 100개 실행 중</span>
        </label>
      )}
      {failed && (
        <div role="alert" className={s.editorChoice}>
          지난 실행 목록을 불러오지 못했습니다.
          <Button size="sm" onClick={() => void recent.refetch()}>
            다시 불러오기
          </Button>
        </div>
      )}
    </>
  );
}
