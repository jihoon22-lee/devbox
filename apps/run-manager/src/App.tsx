import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  createService,
  createJob,
  deleteService,
  deleteJob,
  getServiceInstance,
  hideMainWindow,
  listServices,
  listJobs,
  listActiveRuns,
  loadStartupShortcutStatus,
  loadRuntimeStatus,
  quitApp,
  restartService,
  runJobNow,
  setStartupShortcutEnabled,
  startService,
  stopActiveRun,
  stopService,
  updateService,
  updateJob,
} from "./api";
import JobEditor from "./components/JobEditor";
import RunHistory from "./components/RunHistory";
import ServiceEditor from "./components/ServiceEditor";
import type {
  Job,
  JobInput,
  Run,
  RuntimeStatus,
  ServiceInput,
  ServiceInstance,
  StartupShortcutStatus,
} from "./types";
import "./App.css";

type Screen = "jobs" | "editor" | "services" | "service-editor" | "history";

function targetLabel(job: Job): string {
  return job.targetKind === "wsl" ? `WSL · ${job.targetDistro ?? "배포판 없음"}` : "Windows";
}

function scheduleLabel(job: Job): string {
  return job.cronExpr ?? "일정 없음";
}

function restartLabel(job: Job): string {
  return job.restartPolicy === "on-failure"
    ? "실패 시 재시작"
    : job.restartPolicy === "always"
      ? "항상 재시작"
      : "재시작 안 함";
}

function serviceStateLabel(state: ServiceInstance["state"]): string {
  switch (state) {
    case "running": return "실행 중";
    case "starting": return "시작 중";
    case "stopping": return "정지 중";
    case "retry_waiting": return "재시작 대기";
    case "stopped": return "정지됨";
  }
}

export default function App() {
  const [status, setStatus] = useState<RuntimeStatus | null>(null);
  const [startupStatus, setStartupStatus] = useState<StartupShortcutStatus | null>(null);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [services, setServices] = useState<Job[]>([]);
  const [serviceInstances, setServiceInstances] = useState<Record<string, ServiceInstance>>({});
  const [activeRuns, setActiveRuns] = useState<Record<string, Run | null>>({});
  const [screen, setScreen] = useState<Screen>("jobs");
  const [editingJobId, setEditingJobId] = useState<string | null>(null);
  const [editingServiceId, setEditingServiceId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [activeSnapshotError, setActiveSnapshotError] = useState<string | null>(null);
  const [activeSnapshotFresh, setActiveSnapshotFresh] = useState(false);
  const activeRefresh = useRef<{ promise: Promise<void> | null; pending: boolean; generation: number }>({
    promise: null,
    pending: false,
    generation: 0,
  });

  const refreshActiveRuns = useCallback(async () => {
    const existing = activeRefresh.current.promise;
    if (existing) {
      activeRefresh.current.pending = true;
      await existing;
      return;
    }
    const operation = (async () => {
      do {
        activeRefresh.current.pending = false;
        const generation = ++activeRefresh.current.generation;
        try {
          const runs = await listActiveRuns();
          if (generation !== activeRefresh.current.generation) continue;
          setActiveRuns(Object.fromEntries(runs.map((run) => [run.jobId, run])));
          setActiveSnapshotFresh(true);
          setActiveSnapshotError(null);
        } catch (cause) {
          if (generation === activeRefresh.current.generation) {
            setActiveRuns({});
            setActiveSnapshotFresh(false);
            setActiveSnapshotError(cause instanceof Error ? cause.message : String(cause));
          }
        }
      } while (activeRefresh.current.pending);
    })();
    activeRefresh.current.promise = operation;
    try {
      await operation;
    } finally {
      if (activeRefresh.current.promise === operation) {
        activeRefresh.current.promise = null;
        if (activeRefresh.current.pending) {
          activeRefresh.current.pending = false;
          await refreshActiveRuns();
        }
      }
    }
  }, []);

  const refreshJobs = useCallback(async () => {
    const nextJobs = await listJobs();
    setJobs(nextJobs);
    await refreshActiveRuns();
  }, [refreshActiveRuns]);

  const refreshServices = useCallback(async () => {
    const nextServices = await listServices();
    setServices(nextServices);
    const entries = await Promise.all(
      nextServices.map(async (service): Promise<[string, ServiceInstance] | null> => {
        const instance = await getServiceInstance(service.id);
        return instance ? [service.id, instance] : null;
      }),
    );
    setServiceInstances(Object.fromEntries(entries.filter((entry): entry is [string, ServiceInstance] => entry !== null)));
  }, []);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await loadRuntimeStatus());
      setStatusError(null);
    } catch (cause) {
      setStatusError(cause instanceof Error ? cause.message : String(cause));
    }
  }, []);

  useEffect(() => {
    let active = true;
    void Promise.all([loadRuntimeStatus(), listJobs(), listServices(), loadStartupShortcutStatus()])
      .then(([nextStatus, nextJobs, nextServices, nextStartupStatus]) => {
        if (!active) return;
        setStatus(nextStatus);
        setJobs(nextJobs);
        setServices(nextServices);
        setStartupStatus(nextStartupStatus);
        void refreshActiveRuns();
        setStatusError(null);
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
  }, [refreshActiveRuns]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      void refreshStatus();
      void refreshActiveRuns();
    }, 1_000);
    return () => window.clearInterval(timer);
  }, [refreshActiveRuns, refreshStatus]);

  const handleRunNow = async (job: Job) => {
    setBusy(true);
    try {
      await runJobNow(job.id);
      // The overlap policy may return a queued/skipped row. Refresh the
      // process-only snapshot so the card is never marked running by guess.
      await refreshActiveRuns();
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const handleStopRun = async (job: Job) => {
    setBusy(true);
    try {
      await stopActiveRun(job.id);
      await refreshActiveRuns();
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const handleServiceStart = async (service: Job) => {
    setBusy(true);
    try {
      await startService(service.id);
      await refreshServices();
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const handleServiceStop = async (service: Job) => {
    setBusy(true);
    try {
      await stopService(service.id);
      await refreshServices();
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const handleServiceRestart = async (service: Job) => {
    setBusy(true);
    try {
      await restartService(service.id);
      await refreshServices();
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const editingJob = useMemo(
    () => (editingJobId ? jobs.find((job) => job.id === editingJobId) ?? null : null),
    [editingJobId, jobs],
  );

  const editingService = useMemo(
    () => (editingServiceId ? services.find((service) => service.id === editingServiceId) ?? null : null),
    [editingServiceId, services],
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

  const openServiceCreate = () => {
    setEditingServiceId(null);
    setError(null);
    setScreen("service-editor");
  };

  const openServiceEdit = (service: Job) => {
    setEditingServiceId(service.id);
    setError(null);
    setScreen("service-editor");
  };

  const closeEditor = () => {
    setScreen("jobs");
    setEditingJobId(null);
    setError(null);
  };

  const closeServiceEditor = () => {
    setScreen("services");
    setEditingServiceId(null);
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

  const toggleStartup = async () => {
    if (!startupStatus?.supported) return;
    setBusy(true);
    try {
      setStartupStatus(await setStartupShortcutEnabled(!startupStatus.enabled));
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const handleServiceSave = async (input: ServiceInput) => {
    setBusy(true);
    try {
      if (editingServiceId) {
        await updateService(editingServiceId, input);
      } else {
        await createService(input);
      }
      await refreshServices();
      closeServiceEditor();
    } finally {
      setBusy(false);
    }
  };

  const handleServiceDelete = async (service: Job) => {
    if (!window.confirm(`'${service.name}' 서비스를 삭제할까요? 저장된 정의와 실행 기록도 함께 삭제됩니다.`)) return;
    setBusy(true);
    try {
      await deleteService(service.id);
      await refreshServices();
      if (editingServiceId === service.id) closeServiceEditor();
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
          <button className={`nav-item ${screen === "services" || screen === "service-editor" ? "active" : ""}`} type="button" onClick={() => setScreen("services")}>
            서비스 <span>{services.length}</span>
          </button>
          <button className={`nav-item ${screen === "history" ? "active" : ""}`} type="button" onClick={() => setScreen("history")}>
            실행 기록
          </button>
        </nav>
        <div className="sidebar-actions">
          <button
            type="button"
            disabled={busy || !startupStatus?.supported}
            title={startupStatus?.shortcutPath}
            onClick={() => void toggleStartup()}
          >
            {startupStatus?.supported
              ? `로그인 시 자동 시작: ${startupStatus.enabled ? "켜짐" : "꺼짐"}`
              : "자동 시작: Windows 전용"}
          </button>
          <button type="button" onClick={() => void hideMainWindow()}>트레이로 숨기기</button>
          <button className="danger" type="button" onClick={() => void quitApp()}>안전하게 종료</button>
        </div>
      </aside>

      <section className="content">
        <header>
          <div>
            <span className="eyebrow">LOCAL SCHEDULER</span>
            <h2>
              {screen === "editor"
                ? editingJob
                  ? "작업 편집"
                  : "새 작업"
                : screen === "service-editor"
                  ? editingService
                    ? "서비스 편집"
                    : "새 서비스"
                  : screen === "history"
                    ? "실행 기록"
                    : screen === "services"
                      ? "서비스"
                      : "작업"}
            </h2>
          </div>
          <span className={status?.schedulerRunning ? "status ready" : "status waiting"}>
            {status?.schedulerRunning ? "스케줄러 준비됨" : "스케줄러 시작 중"}
          </span>
        </header>

        {(error ?? statusError ?? activeSnapshotError) ? <div className="error-banner" role="alert">오류: {error ?? statusError ?? activeSnapshotError}</div> : null}

        {screen === "editor" ? (
          <JobEditor job={editingJob} onSave={handleSave} onCancel={closeEditor} />
        ) : screen === "service-editor" ? (
          <ServiceEditor service={editingService} onSave={handleServiceSave} onCancel={closeServiceEditor} />
        ) : screen === "history" ? (
          <RunHistory jobs={jobs.filter((job) => job.kind === "job")} />
        ) : screen === "services" ? (
          <section className="jobs-section" aria-labelledby="services-title">
            <div className="section-toolbar">
              <div>
                <p className="subtitle">서비스 정의와 자동 시작·재시작·로컬 헬스체크 정책을 관리합니다.</p>
                <h3 id="services-title" className="visually-hidden">서비스 목록</h3>
              </div>
              <button type="button" className="button-primary" onClick={openServiceCreate}>+ 새 서비스</button>
            </div>
            {loading ? <div className="empty-card compact"><div className="pulse" /><p>서비스를 불러오는 중…</p></div> : null}
            {!loading && services.length === 0 ? (
              <section className="empty-card" aria-labelledby="empty-service-title">
                <div className="pulse" aria-hidden="true" />
                <h3 id="empty-service-title">등록된 서비스가 아직 없습니다</h3>
                <p>계속 실행할 명령을 서비스로 저장하고 자동 시작·재시작 정책을 준비할 수 있습니다.</p>
                <button type="button" className="button-primary" onClick={openServiceCreate}>첫 서비스 만들기</button>
              </section>
            ) : null}
            {!loading && services.length > 0 ? (
              <div className="job-list service-list">
                {services.map((service) => {
                  const instance = serviceInstances[service.id];
                  const state = instance?.state ?? "stopped";
                  const running = state === "running" || state === "starting";
                  return (
                  <article className="job-card service-card" key={service.id}>
                    <div className="job-card-main">
                      <div className="job-title-row">
                        <h3>{service.name}</h3>
                        <span className={`job-state ${running ? "ready" : "disabled"}`}>{serviceStateLabel(state)}</span>
                      </div>
                      <code title={service.command}>{service.command}</code>
                      <div className="job-meta">
                        <span>{targetLabel(service)}</span>
                        <span>{restartLabel(service)}</span>
                        <span>{service.autoStart ? "자동 시작" : "수동 시작"}</span>
                        {service.healthTcpAddress && service.healthTcpPort ? (
                          <span>TCP {service.healthTcpAddress}:{service.healthTcpPort}</span>
                        ) : <span>TCP probe 없음</span>}
                        {instance && instance.consecutiveFailures > 0 ? (
                          <span>연속 실패 {instance.consecutiveFailures}회</span>
                        ) : null}
                        {service.envConfigured ? <span className="secret-badge">환경변수 보호됨</span> : null}
                      </div>
                    </div>
                    <div className="job-actions">
                      {running ? (
                        <>
                          <button type="button" className="button-secondary" disabled={busy} onClick={() => void handleServiceRestart(service)}>재시작</button>
                          <button type="button" className="button-danger" disabled={busy} onClick={() => void handleServiceStop(service)}>정지</button>
                        </>
                      ) : (
                        <button type="button" className="button-secondary" disabled={busy} onClick={() => void handleServiceStart(service)}>시작</button>
                      )}
                      <button type="button" className="button-secondary" onClick={() => openServiceEdit(service)}>편집</button>
                      <button type="button" className="button-danger" disabled={busy || running} onClick={() => void handleServiceDelete(service)}>삭제</button>
                    </div>
                  </article>
                  );
                })}
              </div>
            ) : null}
          </section>
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
                        <span className={`job-state ${activeRuns[job.id] ? "running" : job.enabled ? "enabled" : "disabled"}`}>
                          {activeRuns[job.id] ? "실행 중" : job.enabled ? "활성" : "비활성"}
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
                      <button type="button" className="button-primary" disabled={busy} onClick={() => void handleRunNow(job)}>지금 실행</button>
      <button type="button" className="button-secondary" disabled={busy || !activeSnapshotFresh || !activeRuns[job.id]} onClick={() => void handleStopRun(job)}>중지</button>
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
