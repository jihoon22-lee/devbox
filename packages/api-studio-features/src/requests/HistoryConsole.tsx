import { useEffect, useMemo, useState } from "react";
import type { HistoryItem } from "./types";
import { filterHistory, historyDisplayLabel, historyMethod, projectHistoryForReplay } from "./lib/history";
import { GRPC_HISTORY_KEY, parseGrpcHistory, type GrpcHistoryStore } from "./lib/grpc";
import "./HistoryConsole.css";
interface Props {
  history: HistoryItem[];
  activity: { sending: boolean; sse: string; sseEvents: number; websocket: string; websocketMessages: number };
  canApply: boolean;
  onApply: (item: HistoryItem) => void;
}
export function HistoryConsole({ history, activity, canApply, onApply }: Props) {
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [grpc, setGrpc] = useState<GrpcHistoryStore | null>(() => parseGrpcHistory(localStorage.getItem(GRPC_HISTORY_KEY)));
  const [grpcInvalid, setGrpcInvalid] = useState(false);
  useEffect(() => {
    let last: string | null | undefined;
    const refresh = () => {
      const raw = localStorage.getItem(GRPC_HISTORY_KEY);
      if (raw === last) return;
      last = raw;
      const parsed = parseGrpcHistory(raw);
      setGrpc(parsed); setGrpcInvalid(raw !== null && !parsed);
    };
    refresh();
    // Protocol sessions stay mounted elsewhere. Read only their bounded summary
    // store while this screen is visible; never collect message/timeline payloads.
    const timer = setInterval(refresh, 1000);
    return () => clearInterval(timer);
  }, []);
  const visible = useMemo(() => filterHistory(history, { query, method: "", status: "all" }), [history, query]);
  const selected = history.find(item => item.id === selectedId);
  const safe = selected ? projectHistoryForReplay(selected) : null;
  const preview = safe ? JSON.stringify(safe.request, null, 2) : "";
  return <section className="studio-history" aria-labelledby="studio-history-title">
    <header><h1 id="studio-history-title">History & Console</h1><p>저장된 요청과 현재 연결 상태를 확인합니다. 기록 선택은 열린 초안을 바꾸지 않습니다.</p></header>
    <section className="studio-console" aria-label="현재 연결 상태">
      <h2>현재 세션</h2>
      <dl><div><dt>HTTP</dt><dd>{activity.sending ? "전송 중" : "대기"}</dd></div>
        <div><dt>SSE</dt><dd>{activity.sse} · 보관 이벤트 {activity.sseEvents}개</dd></div>
        <div><dt>WebSocket</dt><dd>{activity.websocket} · 보관 메시지 {activity.websocketMessages}개</dd></div></dl>
      <p>연결 메시지는 해당 요청·프로토콜 화면에서 확인합니다. 이 화면에 원문을 모으거나 추가 저장하지 않습니다.</p>
    </section>
    <div className="studio-history-columns">
      <section aria-labelledby="studio-http-history"><h2 id="studio-http-history">HTTP 요청 기록</h2>
        <input aria-label="저장된 요청 검색" placeholder="이름·메서드·주소 검색" maxLength={128} value={query} onChange={event => setQuery(event.target.value)}/>
        <div className="studio-history-list">{visible.map(item => <button key={item.id} aria-pressed={selectedId === item.id} onClick={() => setSelectedId(item.id)}>
          <strong>{historyMethod(item)}</strong><span>{historyDisplayLabel(item)}</span><small>{item.status ?? "응답 없음"}</small>
        </button>)}</div>
        {visible.length === 0 && <p>{history.length ? "일치하는 기록이 없습니다." : "저장된 HTTP 요청이 없습니다."}</p>}
      </section>
      <section aria-label="기록 미리보기"><h2>요청 미리보기</h2>
        {safe ? <><p>{historyDisplayLabel(safe)}</p><pre>{preview.slice(0, 32768)}</pre>
          {preview.length > 32768 && <p>긴 요청은 앞부분만 표시합니다.</p>}
          <p>마스킹된 값을 확인하고 필요한 환경·인증을 다시 연결하세요.</p>
          <button className="btn" disabled={!canApply} onClick={() => onApply(safe)}>현재 초안 대신 열기</button>
        </> : <p>기록을 선택하면 안전하게 저장된 요청을 미리 볼 수 있습니다.</p>}
      </section>
    </div>
    <section aria-labelledby="studio-grpc-history"><h2 id="studio-grpc-history">gRPC 실행 요약</h2>
      <p>메시지·인증 값은 포함하지 않습니다. 이 요약에서 호출을 다시 실행하지 않습니다.</p>
      {grpcInvalid && <p role="alert">저장된 gRPC 요약의 형식을 확인하지 못했습니다. 원본은 유지됩니다.</p>}
      {!!grpc?.entries.length && <div className="studio-history-table"><table><thead><tr><th>메서드</th><th>상태</th><th>메시지 수</th><th>시간</th></tr></thead><tbody>
        {grpc.entries.map((entry, index) => <tr key={`${entry.startedAtMs}-${index}`}><td>{entry.service}/{entry.method}</td><td>{entry.status}</td><td>{entry.requestMessageCount} → {entry.responseMessageCount}</td><td>{entry.elapsedMs} ms</td></tr>)}
      </tbody></table></div>}
      {!grpc?.entries.length && !grpcInvalid && <p>저장된 gRPC 실행 요약이 없습니다.</p>}
    </section>
  </section>;
}
