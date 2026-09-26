interface Props {
  historyReady: boolean;
  historyBusy: boolean;
  history: import("../lib/grpc").GrpcHistoryStore;
  onClearHistory: () => Promise<void>;
  native: boolean;
  onExport: (summary: import("../../generated/GrpcExchangeSummary").GrpcExchangeSummary) => Promise<void>;
}

export function GrpcHistory({ historyReady, historyBusy, history, onClearHistory, native, onExport }: Props) {
  return (
    <section className="grpc-panel" aria-labelledby="grpc-history-heading">
      <div className="grpc-history-head">
        <div>
          <h3 id="grpc-history-heading">요약 기록</h3>
          <p className="dim">최대 50개 · 본문/엔드포인트/경로/자격 증명 ID 미저장</p>
        </div>
        <button
          className="btn"
          type="button"
          disabled={!historyReady || historyBusy || history.entries.length === 0}
          onClick={onClearHistory}
        >
          기록 지우기
        </button>
      </div>
      <ol className="grpc-history-list">
        {history.entries.map((entry, index) => (
          <li key={`${entry.startedAtMs}-${entry.service}-${entry.method}-${index}`}>
            <div>
              <strong>
                {entry.service}/{entry.method}
              </strong>
              <code>
                {entry.rpcKind} · {entry.status}
              </code>
              <span>
                {entry.requestMessageCount} → {entry.responseMessageCount}개 메시지 · {entry.elapsedMs}ms
              </span>
              <span>
                {entry.sourceKind} · {entry.tlsMode}
                {entry.credentialUsed ? " · 자격 증명 사용" : ""}
              </span>
              <time dateTime={new Date(entry.startedAtMs).toISOString()}>
                {new Date(entry.startedAtMs).toLocaleString()}
              </time>
            </div>
            <button className="btn" type="button" disabled={!native} onClick={() => void onExport(entry)}>
              요약 내보내기
            </button>
          </li>
        ))}
        {history.entries.length === 0 && <li className="dim">아직 저장된 gRPC 요약이 없습니다.</li>}
      </ol>
    </section>
  );
}
