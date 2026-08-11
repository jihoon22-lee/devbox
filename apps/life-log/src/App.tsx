import { useCallback, useEffect, useMemo, useState } from "react";
import { getActivityDb, getDay, getProjects, setActivityDb, setProjects } from "./api";
import type { DaySummary } from "./types";
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

function shortApp(app: string): string {
  return app.replace(/\.exe$/i, "").slice(0, 22);
}

export default function App() {
  const [date, setDate] = useState(new Date());
  const [day, setDay] = useState<DaySummary | null>(null);
  const [activityDb, setActivityDbState] = useState("");
  const [projects, setProjectsState] = useState<string[]>([]);
  const [projectInput, setProjectInput] = useState("");
  const [tab, setTab] = useState<"day" | "settings">("day");
  const [error, setError] = useState<string | null>(null);

  const range = useMemo(() => dayRange(date), [date]);
  const dateStr = useMemo(
    () => `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`,
    [date],
  );

  const loadDay = useCallback(async () => {
    setError(null);
    try {
      setDay(await getDay(dateStr, range[0], range[1]));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [dateStr, range[0], range[1]]);

  const loadSettings = useCallback(async () => {
    try {
      const [db, pr] = await Promise.all([getActivityDb(), getProjects()]);
      setActivityDbState(db);
      setProjectsState(pr);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void loadDay();
    void loadSettings();
  }, [loadDay, loadSettings]);

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
    setProjectsState((prev) => [...prev, p]);
    setProjectInput("");
    void setProjects([...projects, p]);
  };

  const removeProject = (p: string) => {
    const next = projects.filter((x) => x !== p);
    setProjectsState(next);
    void setProjects(next);
  };

  const shiftDay = (delta: number) => {
    const d = new Date(date);
    d.setDate(d.getDate() + delta);
    setDate(d);
  };

  const topApp = day?.app_totals[0];

  return (
    <div className="app">
      <header className="toolbar">
        <button className="btn" onClick={() => shiftDay(-1)}>
          ◀
        </button>
        <h1 className="title">{dateStr}</h1>
        <button className="btn" onClick={() => shiftDay(1)}>
          ▶
        </button>
        <button className="btn" onClick={() => setDate(new Date())}>
          Today
        </button>
        <span className="spacer" />
        <button className={`btn ${tab === "day" ? "active" : ""}`} onClick={() => setTab("day")}>
          Day
        </button>
        <button className={`btn ${tab === "settings" ? "active" : ""}`} onClick={() => setTab("settings")}>
          Settings
        </button>
        <button className="btn refresh" onClick={() => void loadDay()}>
          Refresh
        </button>
      </header>

      {error && <div className="error">{error}</div>}

      {tab === "day" ? (
        <div className="day">
          {day && (
            <>
              <div className="cards">
                <div className="card">
                  <div className="card-label">PC 사용</div>
                  <div className="card-value">{fmtDuration(day.pc_usage_ms)}</div>
                </div>
                <div className="card">
                  <div className="card-label">Git commits</div>
                  <div className="card-value">{day.git.total_commits}</div>
                </div>
                <div className="card">
                  <div className="card-label">Most active</div>
                  <div className="card-value">{topApp ? shortApp(topApp.app) : "-"}</div>
                </div>
              </div>

              {day.app_totals.length > 0 && (
                <section className="panel">
                  <h2>App usage</h2>
                  {day.app_totals.map((a) => (
                    <div key={a.app} className="stat-row">
                      <span className="stat-app">{shortApp(a.app)}</span>
                      <div className="stat-bar">
                        <div className="stat-fill" style={{ width: `${Math.min(100, (a.duration_ms / day.app_totals[0].duration_ms) * 100)}%` }} />
                      </div>
                      <span className="stat-dur">{fmtDuration(a.duration_ms)}</span>
                    </div>
                  ))}
                </section>
              )}

              {day.git.projects.length > 0 && (
                <section className="panel">
                  <h2>Git</h2>
                  {day.git.projects.map((p) => (
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
      ) : (
        <div className="settings">
          <section className="panel">
            <h2>Activity data source</h2>
            <div className="row">
              <input value={activityDb} onChange={(e) => setActivityDbState(e.currentTarget.value)} />
              <button className="btn" onClick={() => void saveActivityDb()}>
                Save
              </button>
            </div>
            <div className="dim">Other apps store data under %LOCALAPPDATA%\\Workbench\\&lt;app&gt;\\data.db</div>
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
      )}
    </div>
  );
}
