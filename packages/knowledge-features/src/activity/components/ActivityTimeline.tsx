import { fmtDuration, shortApp, fmtTime } from "../lib/activityPresentation";

interface Props {
  tracking: boolean;
  toggleTracking: () => Promise<void>;
  sessions: import("../../generated/Session").Session[];
  stats: import("../../generated/AppTotal").AppTotal[];
  maxStatDuration: number;
}

export function ActivityTimeline({ tracking, toggleTracking, sessions, stats, maxStatDuration }: Props) {
  return (
    <div className="timeline">
      <div className="timeline-head">
        <span className={tracking ? "status-on" : "status-off"}>● {tracking ? "추적 중" : "중지됨"}</span>
        <button className={`btn ${tracking ? "danger" : ""}`} onClick={() => void toggleTracking()}>
          {tracking ? "추적 중지" : "추적 시작"}
        </button>
        <span className="dim">합계: {fmtDuration(sessions.reduce((acc, s) => acc + s.duration_ms, 0))}</span>
      </div>
      {sessions.map((s) => (
        <div key={s.id} className="session">
          <span className="time">{fmtTime(s.start_ts)}</span>
          <span className="app">{shortApp(s.app)}</span>
          <span className="title dim">{s.title || "-"}</span>
          <span className="dur dim">{fmtDuration(s.duration_ms)}</span>
        </div>
      ))}
      {sessions.length === 0 && <div className="empty">오늘 기록된 활동이 없습니다</div>}

      {stats.length > 0 && (
        <section className="panel">
          <h2>앱 사용량</h2>
          {stats.map((a) => (
            <div key={a.app} className="stat-row">
              <span className="stat-app">{shortApp(a.app)}</span>
              <div className="stat-bar">
                <div
                  className="stat-fill"
                  style={{ width: `${Math.min(100, (a.duration_ms / maxStatDuration) * 100)}%` }}
                />
              </div>
              <span className="stat-dur">{fmtDuration(a.duration_ms)}</span>
              <span className="dim">세션 {a.sessions}개</span>
            </div>
          ))}
        </section>
      )}
    </div>
  );
}
