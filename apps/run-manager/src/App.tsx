import { useCallback, useEffect, useMemo, useState } from "react";
import {
  createJob,
  deleteJob,
  hideMainWindow,
  listJobs,
  loadRuntimeStatus,
  quitApp,
  updateJob,
} from "./api";
import JobEditor from "./components/JobEditor";
import RunHistory from "./components/RunHistory";
import type { Job, JobInput, RuntimeStatus } from "./types";
import "./App.css";

type Screen = "jobs" | "editor" | "history";

function targetLabel(job: Job): string {
  return job.targetKind === "wsl" ? `WSL · ${job.targetDistro ?? "배포판 없음"}` : "Windows";
}

function scheduleLabel(job: Job): string {
  return job.cronExpr ?? "일정 없음";
}

export default function App() {
  const [status, setStatus] = useState<RuntimeStatus | null>(null);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [screen, setScreen] = useState<Screen>("jobs");
  const [editingJobId, setEditingJobId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshJobs = useCallback(async () => {
    setJobs(await listJobs());
  }, []);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await loadRuntimeStatus());
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }, []);

  useEffect(() => {
    let active = true;
    void Promise.all([loadRuntimeStatus(), listJobs()])
      .then(([nextStatus, nextJobs]) => {
        if (!active) return;
        setStatus(nextStatus);
        setJobs(nextJobs);
        setError(null);
      })
      .catch((cause: unknown) => {
        if (active) setError(cause instanceof Error ? cause.message : String(cause));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => void refreshStatus(), 1_000);
    return () => window.clearInterval(timer);
  }, [refreshStatus]);

  const editingJob = useMemo(
    () => (editingJobId ? jobs.find((job) => job.id === editingJobId) ?? null : null),
    [editingJobId, jobs],
  );

  const openCreate = () => {
    setEditingJobId(null);
    setError(null);
    setScreen("editor");
  };

  const openEdit = (job: Job) => {
    setEditingJobId(job.id);
    setError(null);
    setScreen("editor");
  };

  const closeEditor = () => {
    setScreen("jobs");
    setEditingJobId(null);
    setError(null);
  };

  const handleSave = async (input: JobInput) => {
    setBusy(true);
    try {
      if (editingJobId) {
        await updateJob(editingJobId, input);
      } else {
        await createJob(input);
      }
      await refreshJobs();
      closeEditor();
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async (job: Job) => {
    if (!window.confirm(`'${job.name}' 작업을 삭제할까요? 실행 기록도 함께 삭제됩니다.`)) return;
    setBusy(true);
    try {
      await deleteJob(job.id);
      await refreshJobs();
      if (editingJobId === job.id) closeEditor();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand-mark" aria-hidden="true">RM</div>
        <div>
          <h1>Run Manager</h1>
          <p>작업과 서비스를 한곳에서 관리합니다.</p>
        </div>
        <nav aria-label="주요 화면">
          <button className={`nav-item ${screen === "jobs" || screen === "editor" ? "active" : ""}`} type="button" onClick={() => setScreen("jobs")}>
            작업 <span>{jobs.length}</span>
          </button>
          <button className="nav-item" type="button" disabled>
            서비스 <span>Phase 2</span>
          </button>
          <button className={`nav-item ${screen === "history" ? "active" : ""}`} type="button" onClick={() => setScreen("history")}>
            실행 기록
          </button>
        </nav>
        <div className="sidebar-actions">
          <button type="button" onClick={() => void hideMainWindow()}>트레이로 숨기기</button>
          <button className="danger" type="button" onClick={() => void quitApp()}>안전하게 종료</button>
        </div>
      </aside>

      <section className="content">
        <header>
          <div>
            <span className="eyebrow">LOCAL SCHEDULER</span>
            <h2>{screen === "editor" ? (editingJob ? "작업 편집" : "새 작업") : screen === "history" ? "실행 기록" : "작업"}</h2>
          </div>
          <span className={status?.schedulerRunning ? "status ready" : "status waiting"}>
            {status?.schedulerRunning ? "스케줄러 준비됨" : "스케줄러 시작 중"}
          </span>
        </header>

        {error ? <div className="error-banner" role="alert">오류: {error}</div> : null}

        {screen === "editor" ? (
          <JobEditor job={editingJob} onSave={handleSave} onCancel={closeEditor} />
        ) : screen === "history" ? (
          <RunHistory jobs={jobs.filter((job) => job.kind === "job")} />
        ) : (
          <section className="jobs-section" aria-labelledby="jobs-title">
            <div className="section-toolbar">
              <div>
                <p className="subtitle">예약된 작업을 활성화하고 실행 정책을 관리합니다.</p>
                <h3 id="jobs-title" className="visually-hidden">작업 목록</h3>
              </div>
              <button type="button" className="button-primary" onClick={openCreate}>+ 새 작업</button>
            </div>
            {loading ? <div className="empty-card compact"><div className="pulse" /><p>작업을 불러오는 중…</p></div> : null}
            {!loading && jobs.length === 0 ? (
              <section className="empty-card" aria-labelledby="empty-title">
                <div className="pulse" aria-hidden="true" />
                <h3 id="empty-title">실행할 작업이 아직 없습니다</h3>
                <p>명령과 cron 일정을 정의하면 로컬 스케줄러가 다음 실행 시각을 미리 보여줍니다.</p>
                <button type="button" className="button-primary" onClick={openCreate}>첫 작업 만들기</button>
                <dl>
                  <div><dt>시작 방식</dt><dd>{status?.backgroundLaunch ? "백그라운드" : "일반"}</dd></div>
                  <div><dt>데이터베이스</dt><dd title={status?.databasePath}>{status?.databasePath ?? "준비 중"}</dd></div>
                </dl>
              </section>
            ) : null}
            {!loading && jobs.length > 0 ? (
              <div className="job-list">
                {jobs.map((job) => (
                  <article className="job-card" key={job.id}>
                    <div className="job-card-main">
                      <div className="job-title-row">
                        <h3>{job.name}</h3>
                        <span className={`job-state ${job.enabled ? "enabled" : "disabled"}`}>
                          {job.enabled ? "활성" : "비활성"}
                        </span>
                      </div>
                      <code title={job.command}>{job.command}</code>
                      <div className="job-meta">
                        <span>{targetLabel(job)}</span>
                        <span>{scheduleLabel(job)}</span>
                        <span>{job.overlapPolicy === "skip" ? "중복 건너뛰기" : job.overlapPolicy === "queue" ? "대기열" : "이전 종료"}</span>
                        {job.envConfigured ? <span className="secret-badge">환경변수 보호됨</span> : null}
                      </div>
                    </div>
                    <div className="job-actions">
                      <button type="button" className="button-secondary" onClick={() => openEdit(job)}>편집</button>
                      <button type="button" className="button-danger" disabled={busy} onClick={() => void handleDelete(job)}>삭제</button>
                    </div>
                  </article>
                ))}
              </div>
            ) : null}
          </section>
        )}
      </section>
    </main>
  );
}
