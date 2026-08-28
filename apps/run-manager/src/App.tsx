import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  createService,
  createJob,
  deleteService,
  deleteJob,
  getServiceInstance,
  getServiceObservability,
  exportDefinitions,
  type ServiceObservability,
  hideMainWindow,
  listServices,
  listJobs,
  listActiveRuns,
  loadStartupShortcutStatus,
  loadRuntimeStatus,
  quitApp,
  restartService,
  runJobNow,
  setJobEnabled,
  setStartupShortcutEnabled,
  startService,
  stopActiveRun,
  stopService,
  onOpenRequest,
  takePendingOpen,
  updateService,
  updateJob,
} from "./api";
import JobEditor from "./components/JobEditor";
import ImportDialog from "./components/ImportDialog";
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

interface ServiceSnapshot {
  services: Job[];
  instances: Record<string, ServiceInstance>;
}

async function loadServiceSnapshot(): Promise<ServiceSnapshot> {
  const services = await listServices();
  const entries = await Promise.all(
    services.map(async (service): Promise<[string, ServiceInstance] | null> => {
      const instance = await getServiceInstance(service.id);
      return instance ? [service.id, instance] : null;
    }),
  );
  return {
    services,
    instances: Object.fromEntries(
      entries.filter((entry): entry is [string, ServiceInstance] => entry !== null),
    ),
  };
}

export default function App() {
  const [status, setStatus] = useState<RuntimeStatus | null>(null);
  const [startupStatus, setStartupStatus] = useState<StartupShortcutStatus | null>(null);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [services, setServices] = useState<Job[]>([]);
  const [serviceInstances, setServiceInstances] = useState<Record<string, ServiceInstance>>({});
  const [obsMap, setObsMap] = useState<Record<string, ServiceObservability | null>>({});
  const [obsOpen, setObsOpen] = useState<Record<string, boolean>>({});
  const [importOpen, setImportOpen] = useState(false);
  const importTriggerRef = useRef<HTMLButtonElement>(null);
  const [activeRuns, setActiveRuns] = useState<Record<string, Run | null>>({});
  const [screen, setScreen] = useState<Screen>("jobs");
  const [editingJobId, setEditingJobId] = useState<string | null>(null);
  const [editingServiceId, setEditingServiceId] = useState<string | null>(null);
  const [historyJobId, setHistoryJobId] = useState<string | null>(null);
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [selectedServiceId, setSelectedServiceId] = useState<string | null>(null);
  const [contextJob, setContextJob] = useState<Job | null>(null);
  const [contextService, setContextService] = useState<Job | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [activeSnapshotError, setActiveSnapshotError] = useState<string | null>(null);
  const [activeSnapshotFresh, setActiveSnapshotFresh] = useState(false);
  const [launcherTask, setLauncherTask] = useState<{ id: string; kind: "job" | "service" } | null>(null);
  const historyDefinitions = useMemo(() => [...jobs, ...services], [jobs, services]);

  const closeImport = useCallback(() => {
    setImportOpen(false);
    window.setTimeout(() => importTriggerRef.current?.focus(), 0);
  }, []);
  const activeRefresh = useRef<{ promise: Promise<void> | null; pending: boolean; generation: number }>({
    promise: null,
    pending: false,
    generation: 0,
  });
  const openRequestRef = useRef<(id: string) => void>(() => undefined);
  const launcherCancelRef = useRef<HTMLButtonElement>(null);

  const prepareJobContext = useCallback((target: HTMLElement) => {
    const id = target.dataset.jobId;
    const job = jobs.find((candidate) => candidate.id === id);
    if (!job) return;
    setSelectedJobId(job.id);
    setContextJob(job);
  }, [jobs]);
  const jobContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareJobContext(target),
  });

  const prepareServiceContext = useCallback((target: HTMLElement) => {
    const id = target.dataset.serviceId;
    const service = services.find((candidate) => candidate.id === id);
    if (!service) return;
    setSelectedServiceId(service.id);
    setContextService(service);
  }, [services]);
  const serviceContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareServiceContext(target),
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
    const snapshot = await loadServiceSnapshot();
    setServices(snapshot.services);
    setServiceInstances(snapshot.instances);
  }, []);

  const handleLauncherTask = useCallback((id: string) => {
    const job = jobs.find((candidate) => candidate.id === id);
    const service = services.find((candidate) => candidate.id === id);
    const task = job ?? service;
    if (!task) {
      setError("Launcher가 요청한 작업을 찾지 못했습니다.");
      return;
    }
    setError(null);
    setScreen(task.kind === "job" ? "jobs" : "services");
    if (task.kind === "job") setSelectedJobId(task.id);
    else setSelectedServiceId(task.id);
    setLauncherTask({ id: task.id, kind: task.kind });
  }, [jobs, services]);

  const confirmLauncherTask = async () => {
    if (!launcherTask || busy) return;
    const task = launcherTask.kind === "job"
      ? jobs.find((candidate) => candidate.id === launcherTask.id && candidate.kind === "job")
      : services.find((candidate) => candidate.id === launcherTask.id && candidate.kind === "service");
    if (!task) {
      setLauncherTask(null);
      setError("Launcher가 요청한 작업을 찾지 못했습니다.");
      return;
    }
    setBusy(true);
    try {
      if (task.kind === "job") {
        await runJobNow(task.id);
        await refreshActiveRuns();
      } else {
        await startService(task.id);
        await refreshServices();
      }
    } catch {
      setError("Launcher가 요청한 작업을 실행하지 못했습니다.");
    } finally {
      setLauncherTask(null);
      setBusy(false);
    }
  };

  openRequestRef.current = (id) => { void handleLauncherTask(id); };

  useEffect(() => {
    if (launcherTask) launcherCancelRef.current?.focus();
    else document.querySelector<HTMLElement>(".job-card.selected")?.focus();
  }, [launcherTask]);

  const onLauncherDialogKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      if (!busy) setLauncherTask(null);
      return;
    }
    if (event.key !== "Tab") return;
    const controls = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not([disabled])"));
    if (controls.length === 0) return;
    const first = controls[0];
    const last = controls[controls.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const onToggleObs = async (id: string) => {
    setObsOpen((prev) => ({ ...prev, [id]: !prev[id] }));
    if (!obsMap[id]) {
      try {
        const obs = await getServiceObservability(id);
        setObsMap((prev) => ({ ...prev, [id]: obs }));
      } catch {
        setObsMap((prev) => ({ ...prev, [id]: null }));
      }
    }
  };

  const fmtUptime = (startedAt: number | null): string => {
    if (!startedAt) return "-";
    const s = Math.floor(Math.max(0, Date.now() - startedAt) / 1000);
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    return h > 0 ? `${h}h ${m}m` : `${m}m`;
  };

  const onExportDefs = async () => {
    try {
      const doc = await exportDefinitions();
      if (!doc) return;
      const blob = new Blob([JSON.stringify(doc, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `run-manager-definitions-v${doc.schemaVersion}.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (cause) {
      setStatusError(cause instanceof Error ? cause.message : String(cause));
    }
  };

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
    void Promise.all([loadRuntimeStatus(), listJobs(), loadServiceSnapshot(), loadStartupShortcutStatus()])
      .then(([nextStatus, nextJobs, serviceSnapshot, nextStartupStatus]) => {
        if (!active) return;
        setStatus(nextStatus);
        setJobs(nextJobs);
        setServices(serviceSnapshot.services);
        setServiceInstances(serviceSnapshot.instances);
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

  // AppLink events are only a wake-up signal. The authoritative request is
  // pulled from the native one-shot slot, then the current job/service list is
  // checked before any run or service action is invoked.
  useEffect(() => {
    if (loading) return;
    const consumePendingOpen = () => {
      void takePendingOpen()
        .then((request) => {
          if (request?.target.kind === "task") openRequestRef.current(request.target.id);
        })
        .catch(() => setError("Launcher 요청을 처리하지 못했습니다."));
    };
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onOpenRequest(() => {
      if (!disposed) consumePendingOpen();
    })
      .then((stop) => {
        if (disposed) stop();
        else {
          unlisten = stop;
          consumePendingOpen();
        }
      })
      .catch(() => consumePendingOpen());
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [loading]);

  useEffect(() => {
    if (screen !== "jobs") {
      jobContextMenu.close();
      setContextJob(null);
    }
    if (screen !== "services") {
      serviceContextMenu.close();
      setContextService(null);
    }
  }, [jobContextMenu.close, screen, serviceContextMenu.close]);

  useEffect(() => {
    const id = contextJob?.id;
    if (!id) return;
    const current = jobs.find((job) => job.id === id) ?? null;
    if (current) setContextJob(current);
    else {
      jobContextMenu.close();
      setContextJob(null);
      setSelectedJobId((selected) => (selected === id ? null : selected));
    }
  }, [contextJob?.id, jobContextMenu.close, jobs]);

  useEffect(() => {
    const id = contextService?.id;
    if (!id) return;
    const current = services.find((service) => service.id === id) ?? null;
    if (current) setContextService(current);
    else {
      serviceContextMenu.close();
      setContextService(null);
      setSelectedServiceId((selected) => (selected === id ? null : selected));
    }
  }, [contextService?.id, serviceContextMenu.close, services]);

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
    if (!window.confirm(`'${job.name}' 작업의 활성 실행을 중지할까요?`)) return;
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
    if (!window.confirm(`'${service.name}' 서비스를 정지할까요?`)) return;
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

  const openHistory = (job: Job) => {
    setHistoryJobId(job.id);
    setError(null);
    setScreen("history");
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

  const handleToggleJob = async (job: Job) => {
    setBusy(true);
    try {
      await setJobEnabled(job.id, !job.enabled);
      await refreshJobs();
      setError(null);
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

  const jobContextItems = useMemo<readonly ContextMenuEntry[]>(() => {
    if (!contextJob) return [];
    return [
      { type: "item", id: "run-now", label: "지금 실행", disabled: busy },
      {
        type: "item",
        id: "toggle-enabled",
        label: contextJob.enabled ? "비활성화" : "활성화",
        disabled: busy,
      },
      { type: "item", id: "edit", label: "편집", disabled: busy },
      { type: "item", id: "open-logs", label: "로그 열기" },
      { type: "separator", id: "job-danger-separator" },
      {
        type: "item",
        id: "delete",
        label: "삭제",
        disabled: busy || !activeSnapshotFresh || Boolean(activeRuns[contextJob.id]),
        danger: true,
      },
    ];
  }, [activeRuns, activeSnapshotFresh, busy, contextJob]);

  const onJobContextSelect = (id: string) => {
    const job = contextJob;
    if (!job) return;
    if (id === "run-now") void handleRunNow(job);
    else if (id === "toggle-enabled") void handleToggleJob(job);
    else if (id === "edit") openEdit(job);
    else if (id === "open-logs") openHistory(job);
    else if (id === "delete") void handleDelete(job);
  };

  const contextServiceState = contextService
    ? serviceInstances[contextService.id]?.state ?? null
    : null;
  const serviceCanStart = contextServiceState === "stopped";
  const serviceCanStop = contextServiceState !== null
    && ["starting", "running", "retry_waiting"].includes(contextServiceState);
  const serviceCanRestart = contextServiceState !== null
    && ["starting", "running", "retry_waiting"].includes(contextServiceState);
  const serviceContextItems = useMemo<readonly ContextMenuEntry[]>(() => {
    if (!contextService) return [];
    return [
      { type: "item", id: "start", label: "시작", disabled: busy || !serviceCanStart },
      {
        type: "item",
        id: "stop",
        label: "정지",
        disabled: busy || !serviceCanStop,
        danger: true,
      },
      {
        type: "item",
        id: "restart",
        label: "재시작",
        disabled: busy || !serviceCanRestart,
      },
      { type: "separator", id: "service-edit-separator" },
      { type: "item", id: "edit", label: "편집", disabled: busy },
      {
        type: "item",
        id: "delete",
        label: "삭제",
        disabled: busy || contextServiceState !== "stopped",
        danger: true,
      },
    ];
  }, [busy, contextService, contextServiceState, serviceCanRestart, serviceCanStart, serviceCanStop]);

  const onServiceContextSelect = (id: string) => {
    const service = contextService;
    if (!service) return;
    if (id === "start") void handleServiceStart(service);
    else if (id === "stop") void handleServiceStop(service);
    else if (id === "restart") void handleServiceRestart(service);
    else if (id === "edit") openServiceEdit(service);
    else if (id === "delete") void handleServiceDelete(service);
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
          <RunHistory jobs={historyDefinitions} requestedJobId={historyJobId} />
        ) : screen === "services" ? (
          <section className="jobs-section" aria-labelledby="services-title">
            <div className="section-toolbar">
              <div>
                <p className="subtitle">서비스 정의와 자동 시작·재시작·로컬 헬스체크 정책을 관리합니다.</p>
                <h3 id="services-title" className="visually-hidden">서비스 목록</h3>
              </div>
              <button type="button" className="button-primary" onClick={openServiceCreate}>+ 새 서비스</button>
              <button type="button" className="button-secondary" onClick={() => void onExportDefs()}>정의 내보내기</button>
              <button ref={importTriggerRef} type="button" className="button-secondary" onClick={() => setImportOpen(true)}>정의 가져오기</button>
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
              <div className="job-list service-list" role="list" aria-label="서비스 목록">
                {services.map((service) => {
                  const instance = serviceInstances[service.id];
                  const state = instance?.state ?? null;
                  const canStart = state === "stopped";
                  const canControl = state !== null
                    && ["starting", "running", "retry_waiting"].includes(state);
                  const ready = state === "running" || state === "starting";
                  const obs = obsMap[service.id];
                  return (
                  <article
                    className={`job-card service-card ${selectedServiceId === service.id ? "selected" : ""}`}
                    key={service.id}
                    role="listitem"
                    tabIndex={0}
                    aria-current={selectedServiceId === service.id ? "true" : undefined}
                    data-service-id={service.id}
                    onClick={() => setSelectedServiceId(service.id)}
                    {...serviceContextMenu.triggerProps}
                  >
                    <div className="job-card-main">
                      <div className="job-title-row">
                        <h3>{service.name}</h3>
                        <span className={`job-state ${ready ? "ready" : "disabled"}`}>
                          {state ? serviceStateLabel(state) : "상태 확인 불가"}
                        </span>
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
                      {canControl ? (
                        <>
                          <button type="button" className="button-secondary" disabled={busy} onClick={() => void handleServiceRestart(service)}>재시작</button>
                          <button type="button" className="button-danger" disabled={busy} onClick={() => void handleServiceStop(service)}>정지</button>
                        </>
                      ) : (
                        <button type="button" className="button-secondary" disabled={busy || !canStart} onClick={() => void handleServiceStart(service)}>시작</button>
                      )}
                      <button type="button" className="button-secondary" onClick={() => void onToggleObs(service.id)}>
                        {obsOpen[service.id] ? "상세 닫기" : "상세"}
                      </button>
                      <button type="button" className="button-secondary" onClick={() => openServiceEdit(service)}>편집</button>
                      <button type="button" className="button-danger" disabled={busy || state !== "stopped"} onClick={() => void handleServiceDelete(service)}>삭제</button>
                    </div>
                    {obsOpen[service.id] && obs && (
                      <div className="obs-panel">
                        <div className="obs-row">
                          <span className="obs-label">정의</span>
                          <span>{obs.definition.enabled ? "활성" : "비활성"} · {obs.definition.autoStart ? "자동 시작" : "수동 시작"}</span>
                        </div>
                        <div className="obs-row">
                          <span className="obs-label">인스턴스 (DB 상태)</span>
                          <span>{obs.instance ? serviceStateLabel(obs.instance.state) : "없음"} · 재시작 {obs.restartCount}회</span>
                        </div>
                        {obs.current && (
                          <div className="obs-row">
                            <span className="obs-label">현재 실행</span>
                            <span>
                              {obs.current.status} · {fmtUptime(obs.current.startedAt)}
                              {obs.currentPid != null && ` · PID ${obs.currentPid} (DB 기록)`}
                            </span>
                          </div>
                        )}
                        {obs.nextRetryAt != null && (
                          <div className="obs-row">
                            <span className="obs-label">다음 재시도</span>
                            <span>{new Date(obs.nextRetryAt).toLocaleTimeString()}</span>
                          </div>
                        )}
                        {obs.recent.length > 0 && (
                          <div className="obs-row">
                            <span className="obs-label">최근 실행</span>
                            <span className="obs-recent">
                              {obs.recent.slice(0, 5).map((r) => (
                                <span key={r.id} className={`obs-run ${r.status === "failed" ? "obs-fail" : ""}`}>
                                  {r.status}{r.exitCode != null ? `(${r.exitCode})` : ""}
                                </span>
                              ))}
                            </span>
                          </div>
                        )}
                        <div className="obs-note">인스턴스 상태는 DB 기록 기준입니다. PID는 실제 프로세스 생존과 다를 수 있습니다.</div>
                      </div>
                    )}
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
              <div className="job-list" role="list" aria-label="작업 목록">
                {jobs.map((job) => (
                  <article
                    className={`job-card ${selectedJobId === job.id ? "selected" : ""}`}
                    key={job.id}
                    role="listitem"
                    tabIndex={0}
                    aria-current={selectedJobId === job.id ? "true" : undefined}
                    data-job-id={job.id}
                    onClick={() => setSelectedJobId(job.id)}
                    {...jobContextMenu.triggerProps}
                  >
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
                      <button type="button" className="button-danger" disabled={busy || !activeSnapshotFresh || !activeRuns[job.id]} onClick={() => void handleStopRun(job)}>중지</button>
                      <button type="button" className="button-secondary" onClick={() => openEdit(job)}>편집</button>
                      <button type="button" className="button-danger" disabled={busy || !activeSnapshotFresh || Boolean(activeRuns[job.id])} onClick={() => void handleDelete(job)}>삭제</button>
                    </div>
                  </article>
                ))}
              </div>
            ) : null}
          </section>
        )}
      </section>
      {importOpen && (
        <ImportDialog
          onDone={(_created) => {
            closeImport();
            void refreshServices();
            void refreshJobs();
          }}
          onClose={closeImport}
        />
      )}
      {launcherTask && (
        <div className="modal-backdrop" role="presentation">
          <section
            className="launcher-task-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="launcher-task-title"
            aria-describedby="launcher-task-description"
            onKeyDown={onLauncherDialogKeyDown}
          >
            <h2 id="launcher-task-title">Launcher 요청 확인</h2>
            <p id="launcher-task-description">
              {launcherTask.kind === "job"
                ? "현재 저장된 작업을 한 번 실행합니다."
                : "현재 저장된 서비스를 시작합니다."}
            </p>
            <div className="launcher-task-actions">
              <button
                ref={launcherCancelRef}
                type="button"
                className="button-secondary"
                disabled={busy}
                onClick={() => setLauncherTask(null)}
              >
                취소
              </button>
              <button
                type="button"
                className="button-primary"
                disabled={busy}
                onClick={() => void confirmLauncherTask()}
              >
                {launcherTask.kind === "job" ? "실행" : "시작"}
              </button>
            </div>
          </section>
        </div>
      )}
      <ContextMenu
        open={jobContextMenu.open}
        anchor={jobContextMenu.anchor}
        restoreFocusTo={jobContextMenu.restoreFocusTo}
        items={jobContextItems}
        onSelect={onJobContextSelect}
        onClose={jobContextMenu.close}
        ariaLabel="작업 메뉴"
      />
      <ContextMenu
        open={serviceContextMenu.open}
        anchor={serviceContextMenu.anchor}
        restoreFocusTo={serviceContextMenu.restoreFocusTo}
        items={serviceContextItems}
        onSelect={onServiceContextSelect}
        onClose={serviceContextMenu.close}
        ariaLabel="서비스 메뉴"
      />
    </main>
  );
}
