/** 같은 요청 파일에서 보낸 이전 기록(`api.history{ origin }`). 고르면 지금 응답과 비교한다. */
import { useEffect, useState } from "react";
import type { HistorySummary, RequestOrigin } from "../../../rpc/gen/rpc";
import { Button } from "../../../ui";
import s from "./Api.module.css";
import { useApiBackend } from "./backend";
import { ResponseCompare } from "./ResponseCompare";

export function HistoryList({
  origin,
  current,
  revision,
}: {
  origin: RequestOrigin | null;
  current?: string;
  revision: number;
}) {
  const backend = useApiBackend();
  const [items, setItems] = useState<HistorySummary[]>([]);
  const [error, setError] = useState(false);
  const [selected, setSelected] = useState<string>();
  const key = origin ? `${JSON.stringify(origin.root)}:${origin.path}` : "";
  // biome-ignore lint/correctness/useExhaustiveDependencies: the origin key identifies the origin object; revision reloads after a send.
  useEffect(() => {
    if (!origin) return;
    let live = true;
    backend.history(origin, 20).then(
      (page) => live && (setItems(page.items), setError(false)),
      () => live && setError(true),
    );
    return () => {
      live = false;
    };
  }, [backend, key, revision]);
  if (!origin) return <p className={s.muted}>저장한 요청만 기록을 모아 볼 수 있습니다.</p>;
  if (error) return <p role="alert">기록을 읽지 못했습니다.</p>;
  if (!items.length) return <p className={s.muted}>아직 이 요청의 기록이 없습니다.</p>;
  return (
    <div className={s.table}>
      <table className={s.results} aria-label="요청 기록">
        <thead>
          <tr>
            <th scope="col">보낸 시각</th>
            <th scope="col">상태</th>
            <th scope="col">시간</th>
            <th scope="col" aria-label="비교" />
          </tr>
        </thead>
        <tbody>
          {items.map((item) => (
            <tr key={item.id} aria-selected={selected === item.id}>
              <td>
                <time dateTime={new Date(item.sentAtMs).toISOString()}>{new Date(item.sentAtMs).toLocaleString()}</time>
              </td>
              <td>{item.status ?? (item.outcome === "cancelled" ? "취소" : "실패")}</td>
              <td>{`${item.elapsedMs} ms`}</td>
              <td>
                <Button size="sm" variant="ghost" disabled={current === undefined} onClick={() => setSelected(item.id)}>
                  비교
                </Button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {selected && current !== undefined && <ResponseCompare historyId={selected} current={current} />}
    </div>
  );
}
