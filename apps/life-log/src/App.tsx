import { useCallback, useEffect, useMemo, useState } from "react";
import { getActivityDb, getDay, getProjects, getRange, setActivityDb, setProjects } from "./api";
import type { DaySummary, RangeSummary } from "./types";
import "./App.css";

const DAY_MS = 86_400_000;

function fmtDuration(ms: number): string {
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

function shortApp(app: string): string {
  return app.replace(/\.exe$/i, "").slice(0, 22);
}

export function toDateStr(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function fmtDay(dayMs: number): string {
  const d = new Date(dayMs);
  return `${d.getMonth() + 1}/${d.getDate()}`;
}

type ViewTab = "day" | "week" | "month" | "settings";

export function weekRange(date: Date): { start: number; end: number } {
  const d = new Date(date);
  const day = (d.getDay() + 6) % 7; // 월요일 시작
  d.setDate(d.getDate() - day);
  const start = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  return { start, end: start + 7 * DAY_MS };
}

export function monthRange(date: Date): { start: number; end: number } {
  const start = new Date(date.getFullYear(), date.getMonth(), 1).getTime();
  const end = new Date(date.getFullYear(), date.getMonth() + 1, 1).getTime();
  return { start, end };
}

export default function App() {
  const [date, setDate] = useState(new Date());
  const [view, setView] = useState<ViewTab>("day");
  const [day, setDay] = useState<DaySummary | null>(null);
  const [range, setRange] = useState<RangeSummary | null>(null);
  const [loading, setLoading] = useState(false);
  const [activityDb, setActivityDbState] = useState("");
  const [projects, setProjectsState] = useState<string[]>([]);
  const [projectInput, setProjectInput] = useState("");
  const [error, setError] = useState<string | null>(null);

  const dateStr = useMemo(() => toDateStr(date), [date]);

  const loadSettings = useCallback(async () => {
    try {
      const [db, pr] = await Promise.all([getActivityDb(), getProjects()]);
      setActivityDbState(db);
      setProjectsState(pr);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      if (view === "day") {
        const start = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
        setDay(await getDay(dateStr, start, start + DAY_MS));
      } else if (view === "week") {
        const { start, end } = weekRange(date);
        setRange(await getRange(`${fmtDay(start)} ~ ${fmtDay(end - DAY_MS)}`, start, end));
      } else if (view === "month") {
        const { start, end } = monthRange(date);
        setRange(await getRange(`${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}`, start, end));
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [view, date, dateStr]);

  useEffect(() => {
    void load();
    void loadSettings();
  }, [load, loadSettings]);

  const saveActivityDb = async () => {
    setError(null);
    try {
      await setActivityDb(activityDb);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const addProject = () => {
    const p = projectInput.trim();
    if (!p) return;
    const next = projects.includes(p) ? projects : [...projects, p];
    setProjectsState(next);
    setProjectInput("");
    void setProjects(next);
  };

  const removeProject = (p: string) => {
    const next = projects.filter((x) => x !== p);
    setProjectsState(next);
    void setProjects(next);
  };

  const shift = (delta: number) => {
    const d = new Date(date);
    if (view === "day") d.setDate(d.getDate() + delta);
    else if (view === "week") d.setDate(d.getDate() + delta * 7);
    else if (view === "month") d.setMonth(d.getMonth() + delta);
    setDate(d);
  };

  const topApp = day?.app_totals[0];
  const maxDaily = Math.max(1, ...(range?.daily.map((d) => d.pc_usage_ms) ?? []));
  const summary = view === "day" ? day : range;

  return (
    <div className="app">
      <header className="toolbar">
        <button className="btn" onClick={() => shift(-1)}>
          ◀
        </button>
        <input
          type="date"
          className="date-input"
          value={dateStr}
          onChange={(e) => e.currentTarget.value && setDate(new Date(e.currentTarget.value + "T00:00:00"))}
        />
        <button className="btn" onClick={() => shift(1)}>
          ▶
        </button>
        <button className="btn" onClick={() => setDate(new Date())}>
          Today
        </button>
        <span className="spacer" />
        {loading && <span className="loading">Loading...</span>}
        {(["day", "week", "month", "settings"] as const).map((t) => (
          <button key={t} className={`btn ${view === t ? "active" : ""}`} onClick={() => setView(t)}>
            {t[0].toUpperCase() + t.slice(1)}
          </button>
        ))}
        <button className="btn refresh" onClick={() => void load()}>
          Refresh
        </button>
      </header>

      {error && <div className="error">{error}</div>}

      {view === "settings" ? (
        <div className="settings">
          <section className="panel">
            <h2>Activity data source</h2>
            <div className="row">
              <input value={activityDb} onChange={(e) => setActivityDbState(e.currentTarget.value)} />
              <button className="btn" onClick={() => void saveActivityDb()}>
                Save
              </button>
            </div>
            <div className="dim">Other apps store data under %LOCALAPPDATA%\&lt;identifier&gt;\data.db (e.g. com.workbench.activitytimeline)</div>
          </section>

          <section className="panel">
            <h2>Git project paths</h2>
            {projects.map((p) => (
              <div key={p} className="git-row">
                <span className="mono">{p}</span>
                <button className="mini" onClick={() => removeProject(p)}>
                  ✕
                </button>
              </div>
            ))}
            <div className="row">
              <input placeholder="C:\projects\devbox" value={projectInput} onChange={(e) => setProjectInput(e.currentTarget.value)} onKeyDown={(e) => e.key === "Enter" && addProject()} />
              <button className="btn" onClick={addProject}>
                Add
              </button>
            </div>
          </section>
        </div>
      ) : (
        <div className="day">
          {summary && (
            <>
              <div className="cards">
                <div className="card">
                  <div className="card-label">PC 사용</div>
                  <div className="card-value">{fmtDuration(summary.pc_usage_ms)}</div>
                </div>
                <div className="card">
                  <div className="card-label">Git commits</div>
                  <div className="card-value">{summary.git.total_commits}</div>
                </div>
                <div className="card">
                  <div className="card-label">Most active</div>
                  <div className="card-value">{topApp ? shortApp(topApp.app) : summary.app_totals[0] ? shortApp(summary.app_totals[0].app) : "-"}</div>
                </div>
              </div>

              {view !== "day" && range && (
                <section className="panel">
                  <h2>{range.label} — daily usage</h2>
                  <div className="daily-chart">
                    {range.daily.map((p) => (
                      <div key={p.day_ms} className="daily-col" title={`${fmtDay(p.day_ms)}: ${fmtDuration(p.pc_usage_ms)}`}>
                        <div className="daily-bar" style={{ height: `${Math.max(2, (p.pc_usage_ms / maxDaily) * 100)}%` }} />
                        <div className="daily-label">{fmtDay(p.day_ms)}</div>
                      </div>
                    ))}
                    {range.daily.length === 0 && <div className="empty">No activity in this period</div>}
                  </div>
                </section>
              )}

              {summary.app_totals.length > 0 && (
                <section className="panel">
                  <h2>App usage</h2>
                  {summary.app_totals.map((a) => (
                    <div key={a.app} className="stat-row">
                      <span className="stat-app">{shortApp(a.app)}</span>
                      <div className="stat-bar">
                        <div className="stat-fill" style={{ width: `${Math.min(100, (a.duration_ms / summary.app_totals[0].duration_ms) * 100)}%` }} />
                      </div>
                      <span className="stat-dur">{fmtDuration(a.duration_ms)}</span>
                    </div>
                  ))}
                </section>
              )}

              {summary.git.projects.length > 0 && (
                <section className="panel">
                  <h2>Git</h2>
                  {summary.git.projects.map((p) => (
                    <div key={p.path} className="git-row">
                      <span className="mono dim">{p.path}</span>
                      <span className="git-count">{p.commits} commits</span>
                    </div>
                  ))}
                </section>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
