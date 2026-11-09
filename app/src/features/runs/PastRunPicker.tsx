import { useId } from "react";
import type { Run } from "../../rpc/gen/rpc";
import { useRpcQuery, useTopicInvalidation } from "../../rpc/query";
import s from "./RunsPage.module.css";
import { resultLabel } from "./runPresentation";

/** The runs of the shown run's definition among the project's latest 100, newest first; hidden when it is the only one. */
export function PastRunPicker({ run, onPick }: { run: Run; onPick: (runId: string) => void }) {
  const hint = useId();
  const params = { projectId: run.projectId, limit: 100 };
  const recent = useRpcQuery("runtime.runs", params);
  useTopicInvalidation("runtime.changed", [["runtime.runs", params]]);
  // The shown run is always an option, so the value stays valid when it is older than the latest 100.
  const runs = [run, ...(recent.data?.items ?? []).filter((item) => item.defId === run.defId && item.id !== run.id)];
  runs.sort((a, b) => b.startedMs - a.startedMs);
  if (runs.length < 2) return null;
  return (
    <label className={s.editorChoice}>
      지난 실행
      <select
        aria-label="지난 실행"
        aria-describedby={hint}
        value={run.id}
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
  );
}
