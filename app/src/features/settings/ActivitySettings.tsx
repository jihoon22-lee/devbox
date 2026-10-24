import { useState } from "react";
import type { ExportFormat } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation } from "../../rpc/query";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Button } from "../../ui";
import { ActivityControls } from "../activity/Consent";
import { localDate } from "../activity/dates";
import { RulesEditor } from "../activity/RulesEditor";
import { useActivityHealth, useActivityStatus } from "../activity/useActivityStatus";
export function ActivitySettings() {
  const { state } = useRpcSession();
  const control = useActivityStatus();
  const status = useActivityHealth();
  const [from, setFrom] = useState(localDate);
  const [to, setTo] = useState(localDate);
  const [format, setFormat] = useState<ExportFormat>("csv");
  const output = useRpcMutation("activity.export");
  return (
    <section aria-label="활동 설정">
      <ActivityControls control={control} />
      {status?.state === "storage_failed" && <p role="alert">저장 중단 · {status.pending}건 대기</p>}
      {!!status?.dropped && <p>저장하지 못한 관찰 {status.dropped}건</p>}
      <RulesEditor />
      <h3>활동 기록 내보내기</h3>
      <label>
        시작 날짜 <input type="date" value={from} onChange={(e) => setFrom(e.target.value)} />
      </label>
      <label>
        끝 날짜 <input type="date" value={to} onChange={(e) => setTo(e.target.value)} />
      </label>
      <label>
        파일 형식{" "}
        <select value={format} onChange={(e) => setFormat(e.target.value as ExportFormat)}>
          <option value="csv">CSV</option>
          <option value="json">JSON</option>
        </select>
      </label>
      <Button
        disabled={output.isPending || state !== "ready" || !from || !to || from > to}
        onClick={() => output.mutate({ from, to, format })}
      >
        내보내기
      </Button>
      {output.data && (
        <p role="status">
          내보낸 파일: <code>{output.data.path}</code>
        </p>
      )}
      {output.error && <p role="alert">{messageFor(output.error.code, output.error.detail).title}</p>}
    </section>
  );
}
