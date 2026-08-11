import { useCallback, useEffect, useMemo, useState } from "react";
import { getAppStats, getTimeline, isTracking, startTracking, stopTracking } from "./api";
import type { AppTotal, Session } from "./types";
import "./App.css";

function dayRange(date: Date): [number, number] {
  const start = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  return [start, start + 86_400_000];
}

function fmtDuration(ms: number): string {
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

function fmtTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export default function App() {
  const [date, setDate] = useState(new Date());
  const [sessions, setSessions] = useState<Session[]>([]);
  const [stats, setStats] = useState<AppTotal[]>([]);
  const [tracking, setTracking] = useState(false);
  const [tab, setTab] = useState<"timeline" | "stats">("timeline");
  const [error, setError] = useState<string | null>(null);

  const range = useMemo(() => dayRange(date), [date]);

  const load = useCallback(async () => {
    setError(null);
    try {
      const [ts, st, tr] = await Promise.all([
        getTimeline(range[0], range[1]),
        getAppStats(range[0], range[1]),
        isTracking(),
      ]);
      setSessions(ts);
      setStats(st);
      setTracking(tr);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [range[0], range[1]]);

  useEffect(() => {
    void load();
  }, [load]);

  const totals = useMemo(() => {
    const map = new Map<string, number>();
    for (const s of sessions) map.set(s.app, (map.get(s.app) ?? 0) + s.duration_ms);
    return [...map.entries()].sort((a, b) => b[1] - a[1]);
  }, [sessions]);

  const dayUsage = sessions.reduce((acc, s) => acc + s.duration_ms, 0);

  const toggleTracking = async () => {
    try {
      if (tracking) {
        await stopTracking();
        setTracking(false);
      } else {
        await startTracking();
        setTracking(true);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const shiftDay = (delta: number) => {
    const d = new Date(date);
    d.setDate(d.getDate() + delta);
    setDate(d);
  };

  return (
    <div className="app">
      <header className="toolbar">
        <button className="btn" onClick={() => shiftDay(-1)}>
          ◀
        </button>
        <h1 className="title">{date.toLocaleDateString()}</h1>
        <button className="btn" onClick={() => shiftDay(1)}>
          ▶
        </button>
        <button className="btn" onClick={() => setDate(new Date())}>
          Today
        </button>
        <span className="spacer" />
        <span className={tracking ? "status-on" : "status-off"}>● {tracking ? "Tracking" : "Stopped"}</span>
        <button className={`btn ${tracking ? "danger" : ""}`} onClick={() => void toggleTracking()}>
          {tracking ? "Stop" : "Start"} tracking
        </button>
        <button className="btn refresh" onClick={() => void load()}>
          Refresh
        </button>
      </header>

      {error && <div className="error">{error}</div>}

      <div className="tabs">
        <button className={`tab ${tab === "timeline" ? "active" : ""}`} onClick={() => setTab("timeline")}>
          Timeline
        </button>
        <button className={`tab ${tab === "stats" ? "active" : ""}`} onClick={() => setTab("stats")}>
          Stats
        </button>
      </div>

      {tab === "timeline" ? (
        <div className="timeline">
          <div className="day-summary">
            <span>
              Total: <strong>{fmtDuration(dayUsage)}</strong>
            </span>
            {totals.slice(0, 3).map(([app, ms]) => (
              <span key={app} className="chip">
                {shortApp(app)} {fmtDuration(ms)}
              </span>
            ))}
          </div>
          {sessions.map((s) => (
            <div key={s.id} className="session">
              <span className="time">{fmtTime(s.start_ts)}</span>
              <span className="app">{shortApp(s.app)}</span>
              <span className="title dim">{s.title || "-"}</span>
              <span className="dur dim">{fmtDuration(s.duration_ms)}</span>
            </div>
          ))}
          {sessions.length === 0 && <div className="empty">No activity recorded this day</div>}
        </div>
      ) : (
        <div className="stats">
          {stats.map((a) => (
            <div key={a.app} className="stat-row">
              <span className="stat-app">{shortApp(a.app)}</span>
              <div className="stat-bar">
                <div
                  className="stat-fill"
                  style={{ width: `${Math.min(100, (a.duration_ms / (stats[0]?.duration_ms || 1)) * 100)}%` }}
                />
              </div>
              <span className="stat-dur">{fmtDuration(a.duration_ms)}</span>
              <span className="dim">{a.sessions} sessions</span>
            </div>
          ))}
          {stats.length === 0 && <div className="empty">No stats for this period</div>}
        </div>
      )}
    </div>
  );
}

function shortApp(app: string): string {
  return app.replace(/\.exe$/i, "").slice(0, 24);
}
