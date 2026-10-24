import { messageFor } from "../../rpc/messages";
import { useRpcQuery, useTopicInvalidation } from "../../rpc/query";
import { Button } from "../../ui";
import { ActivityBars } from "./ActivityBars";
import { ActivityTables } from "./ActivityTables";
import { shiftDate } from "./dates";
export function ActivityWeek({ date }: { date: string }) {
  const params = { from: shiftDate(date, -6), to: date };
  useTopicInvalidation("activity.changed", [["activity.range", params]]);
  const query = useRpcQuery("activity.range", params);
  if (query.error)
    return (
      <p role="alert">
        {messageFor(query.error.code, query.error.detail).title}{" "}
        <Button onClick={() => void query.refetch()}>다시 시도</Button>
      </p>
    );
  if (!query.data) return <p role="status">활동을 불러오는 중…</p>;
  return (
    <section aria-label="주간 활동">
      <p>
        {params.from} ~ {params.to}
      </p>
      <ActivityBars
        label="날짜별 활동"
        items={query.data.days.map((item) => ({ label: item.date.slice(5), ms: item.totalMs }))}
      />
      <ActivityTables apps={query.data.apps} projects={query.data.projects} />
    </section>
  );
}
