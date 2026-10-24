import { messageFor } from "../../rpc/messages";
import { useRpcQuery, useTopicInvalidation } from "../../rpc/query";
import { Button } from "../../ui";
import { ActivityBars } from "./ActivityBars";
import { ActivityTables } from "./ActivityTables";
import { duration } from "./dates";
export function ActivityDay({ date }: { date: string }) {
  useTopicInvalidation("activity.changed", [["activity.day", { date }]]);
  const query = useRpcQuery("activity.day", { date });
  if (query.error)
    return (
      <p role="alert">
        {messageFor(query.error.code, query.error.detail).title}{" "}
        <Button onClick={() => void query.refetch()}>다시 시도</Button>
      </p>
    );
  if (!query.data) return <p role="status">활동을 불러오는 중…</p>;
  const day = query.data;
  return (
    <section aria-label="하루 활동">
      <p>총 {duration(day.totalMs)}</p>
      <ActivityBars label="시간대별 활동" items={day.hours.map((item) => ({ label: `${item.hour}시`, ms: item.ms }))} />
      <ActivityTables apps={day.apps} projects={day.projects} />
      {day.titles.length > 0 && (
        <details>
          <summary>창 제목별 활동</summary>
          <table>
            <tbody>
              {day.titles.map((item) => (
                <tr key={`${item.app}:${item.title}`}>
                  <td>{item.app}</td>
                  <td>{item.title}</td>
                  <td>{duration(item.ms)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </details>
      )}
    </section>
  );
}
