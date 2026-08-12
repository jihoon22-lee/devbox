import { useCallback, useEffect, useState } from "react";
import { hideMainWindow, loadRuntimeStatus, quitApp } from "./api";
import type { RuntimeStatus } from "./types";
import "./App.css";

export default function App() {
  const [status, setStatus] = useState<RuntimeStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await loadRuntimeStatus());
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand-mark" aria-hidden="true">
          RM
        </div>
        <div>
          <h1>Run Manager</h1>
          <p>작업과 서비스를 한곳에서 관리합니다.</p>
        </div>
        <nav aria-label="주요 화면">
          <button className="nav-item active" type="button">
            작업
          </button>
          <button className="nav-item" type="button" disabled>
            서비스 <span>Phase 2</span>
          </button>
          <button className="nav-item" type="button" disabled>
            실행 기록
          </button>
        </nav>
        <div className="sidebar-actions">
          <button type="button" onClick={() => void hideMainWindow()}>
            트레이로 숨기기
          </button>
          <button className="danger" type="button" onClick={() => void quitApp()}>
            안전하게 종료
          </button>
        </div>
      </aside>

      <section className="content">
        <header>
          <div>
            <span className="eyebrow">LOCAL SCHEDULER</span>
            <h2>작업</h2>
          </div>
          <span className={status?.schedulerRunning ? "status ready" : "status waiting"}>
            {status?.schedulerRunning ? "스케줄러 준비됨" : "스케줄러 시작 중"}
          </span>
        </header>

        {error ? <div className="error-banner">상태를 불러오지 못했습니다: {error}</div> : null}

        <section className="empty-card" aria-labelledby="empty-title">
          <div className="pulse" aria-hidden="true" />
          <h3 id="empty-title">실행할 작업이 아직 없습니다</h3>
          <p>
            앱 생명주기와 백그라운드 스케줄러가 준비되었습니다. 다음 기능에서 cron 작업과 실행 기록을
            연결합니다.
          </p>
          <dl>
            <div>
              <dt>시작 방식</dt>
              <dd>{status?.backgroundLaunch ? "백그라운드" : "일반"}</dd>
            </div>
            <div>
              <dt>데이터베이스</dt>
              <dd title={status?.databasePath}>{status?.databasePath ?? "준비 중"}</dd>
            </div>
          </dl>
        </section>
      </section>
    </main>
  );
}
