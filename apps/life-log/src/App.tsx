import { useCallback, useEffect, useMemo, useState } from "react";
import {
  autostartStatus,
  getAppStats,
  getDay,
  getIdleThreshold,
  getPrivacyRules,
  getProjects,
  getRange,
  getTimeline,
  integrationSources,
  isTracking,
  redactExisting,
  setAutostart,
  setIdleThreshold,
  setPrivacyRules,
  setProjects,
  startTracking,
  stopTracking,
  type AutostartStatus,
  type PrivacyRules,
  type SourceStatus,
} from "./api";
import type { AppTotal, DaySummary, RangeSummary, Session } from "./types";
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

function fmtTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

type ViewTab = "day" | "week" | "month" | "timeline" | "settings";

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
  const [sessions, setSessions] = useState<Session[]>([]);
  const [stats, setStats] = useState<AppTotal[]>([]);
  const [tracking, setTracking] = useState(false);
  const [projects, setProjectsState] = useState<string[]>([]);
  const [projectInput, setProjectInput] = useState("");
  const [idleThreshold, setIdleThresholdState] = useState(300000);
  const [privacy, setPrivacy] = useState<PrivacyRules>({ excludedProcesses: [], excludedTitlePatterns: [], redactTitlePatterns: [], maskAllTitles: false });
  const [autoStart, setAutoStart] = useState<AutostartStatus | null>(null);
  const [sources, setSources] = useState<SourceStatus[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const dateStr = useMemo(() => toDateStr(date), [date]);

  const loadSettings = useCallback(async () => {
    try {
      const [pr, idle, privacyRules, ast, src] = await Promise.all([
        getProjects(),
        getIdleThreshold(),
        getPrivacyRules(),
        autostartStatus(),
        integrationSources(),
      ]);
      setProjectsState(pr);
      setIdleThresholdState(idle);
      setPrivacy(privacyRules);
      setAutoStart(ast);
      setSources(src);
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
      } else if (view === "timeline") {
        const start = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
        const [ts, st, tr] = await Promise.all([
          getTimeline(start, start + DAY_MS),
          getAppStats(start, start + DAY_MS),
          isTracking(),
        ]);
        setSessions(ts);
        setStats(st);
        setTracking(tr);
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

  // 타임라인은 추적 중에는 주기적으로 갱신한다 (세션 자동 반영).
  useEffect(() => {
    if (view !== "timeline") return;
    const id = setInterval(() => void load(), 30_000);
    return () => clearInterval(id);
  }, [view, load]);

  const toggleTracking = async () => {
    setError(null);
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
        {(["day", "week", "month", "timeline", "settings"] as const).map((t) => (
          <button key={t} className={`btn ${view === t ? "active" : ""}`} onClick={() => setView(t)}>
            {t[0].toUpperCase() + t.slice(1)}
          </button>
        ))}
        <button className="btn refresh" onClick={() => void load()}>
          Refresh
        </button>
      </header>

      {error && <div className="error">{error}</div>}
      {notice && <div className="notice">{notice}</div>}

      {view === "settings" ? (
        <div className="settings">
          <section className="panel">
            <h2>Data sources</h2>
            {sources.length === 0 && <div className="dim">등록된 source가 없습니다.</div>}
            {sources.map((s) => (
              <div key={s.producer} className="git-row">
                <span className="mono">{s.producer}</span>
                {s.available ? (
                  <span className="dim">
                    v{s.schemaVersion} · {s.producerVersion}
                    {s.freshnessMs != null && ` · ${fmtDuration(s.freshnessMs)} 전 갱신`}
                  </span>
                ) : (
                  <span className="tag-dirty">{s.error ?? "사용할 수 없음"}</span>
                )}
              </div>
            ))}
            <div className="dim">source는 devbox 공용 루트의 read-only snapshot을 통해 읽습니다 (다른 앱의 DB를 직접 읽지 않음).</div>
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
            <div className="dim">활동 추적은 Life Log에 통합되어 있으며, 세션은 자동으로 기록됩니다.</div>
          </section>

          <section className="panel">
            <h2>Idle detection</h2>
            <div className="row">
              <span className="dim">자리를 비운 지 (분):</span>
              <input
                type="number"
                min={1}
                value={Math.round(idleThreshold / 60000)}
                onChange={(e) => {
                  const minutes = Number(e.currentTarget.value);
                  if (Number.isFinite(minutes) && minutes >= 1) {
                    setIdleThresholdState(minutes * 60000);
                    void setIdleThreshold(minutes * 60000);
                  }
                }}
              />
            </div>
            <div className="dim">이 시간 이상 입력이 없으면 해당 구간을 사용 시간에서 제외합니다.</div>
          </section>

          <section className="panel">
            <h2>Auto start</h2>
            {autoStart?.supported ? (
              <label className="row">
                <input
                  type="checkbox"
                  checked={autoStart.enabled}
                  onChange={(e) => {
                    setError(null);
                    setNotice(null);
                    void (async () => {
                      try {
                        const next = await setAutostart(e.currentTarget.checked);
                        setAutoStart(next);
                      } catch (err) {
                        setError(err instanceof Error ? err.message : String(err));
                      }
                    })();
                  }}
                />
                Windows 로그인 시 자동 시작
              </label>
            ) : (
              <div className="dim">이 플랫폼에서는 자동 시작을 지원하지 않습니다.</div>
            )}
          </section>

          <section className="panel">
            <h2>Privacy rules</h2>
            <div className="privacy-row">
              <span className="dim">제외할 프로세스 (쉼표 구분, 정확 일치):</span>
              <input
                value={privacy.excludedProcesses.join(", ")}
                onChange={(e) => {
                  const next = { ...privacy, excludedProcesses: e.currentTarget.value.split(",").map((s) => s.trim()).filter(Boolean) };
                  setPrivacy(next);
                  void setPrivacyRules(next);
                }}
              />
            </div>
            <div className="privacy-row">
              <span className="dim">제목 미저장 정규식 (쉼표 구분):</span>
              <input
                value={privacy.excludedTitlePatterns.join(", ")}
                onChange={(e) => {
                  const next = { ...privacy, excludedTitlePatterns: e.currentTarget.value.split(",").map((s) => s.trim()).filter(Boolean) };
                  setPrivacy(next);
                  void setPrivacyRules(next);
                }}
              />
            </div>
            <div className="privacy-row">
              <span className="dim">제목 치환 정규식 → [redacted] (쉼표 구분):</span>
              <input
                value={privacy.redactTitlePatterns.join(", ")}
                onChange={(e) => {
                  const next = { ...privacy, redactTitlePatterns: e.currentTarget.value.split(",").map((s) => s.trim()).filter(Boolean) };
                  setPrivacy(next);
                  void setPrivacyRules(next);
                }}
              />
            </div>
            <label className="row">
              <input type="checkbox" checked={privacy.maskAllTitles} onChange={(e) => {
                const next = { ...privacy, maskAllTitles: e.currentTarget.checked };
                setPrivacy(next);
                void setPrivacyRules(next);
              }} />
              모든 제목을 저장하지 않음
            </label>
            <div className="row">
              <button className="btn" onClick={() => void (async () => {
                setError(null);
                try {
                  const n = await redactExisting();
                  setNotice(`기존 세션 ${n}개에 규칙을 적용했습니다.`);
                } catch (e) {
                  setError(e instanceof Error ? e.message : String(e));
                }
              })()}>
                기존 세션에 적용
              </button>
            </div>
            <div className="dim">규칙은 DB 저장 전에 적용됩니다. 제외한 원문은 어디에도 남지 않습니다.</div>
          </section>
        </div>
      ) : view === "timeline" ? (
        <div className="timeline">
          <div className="timeline-head">
            <span className={tracking ? "status-on" : "status-off"}>● {tracking ? "Tracking" : "Stopped"}</span>
            <button className={`btn ${tracking ? "danger" : ""}`} onClick={() => void toggleTracking()}>
              {tracking ? "Stop" : "Start"} tracking
            </button>
            <span className="dim">Total: {fmtDuration(sessions.reduce((acc, s) => acc + s.duration_ms, 0))}</span>
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

          {stats.length > 0 && (
            <section className="panel">
              <h2>App usage</h2>
              {stats.map((a) => (
                <div key={a.app} className="stat-row">
                  <span className="stat-app">{shortApp(a.app)}</span>
                  <div className="stat-bar">
                    <div className="stat-fill" style={{ width: `${Math.min(100, (a.duration_ms / stats[0].duration_ms) * 100)}%` }} />
                  </div>
                  <span className="stat-dur">{fmtDuration(a.duration_ms)}</span>
                  <span className="dim">{a.sessions} sessions</span>
                </div>
              ))}
            </section>
          )}
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
