import {
  ContextMenu,
  useContextMenu,
  type ContextMenuEntry,
} from "@devbox/context-menu";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  createService,
  createJob,
  deleteService,
  deleteJob,
  acceptWorkspaceTaskControl,
  getServiceInstance,
  getServiceObservability,
  getWorkspaceTaskOperation,
  exportDefinitions,
  type ServiceObservability,
  friendlyErrorMessage,
  hideMainWindow,
  listWorkspaceTasks,
  listServices,
  listJobs,
  listActiveRuns,
  listWorkspaceTaskControlReceipts,
  listWorkspaceTaskDiagnostics,
  listWorkspaceTaskOperations,
  loadStartupShortcutStatus,
  loadRuntimeStatus,
  quitApp,
  restartService,
  runJobNow,
  runWorkspaceTaskOperation,
  setJobEnabled,
  setStartupShortcutEnabled,
  startService,
  stopActiveRun,
  stopService,
  stopWorkspaceTaskOperation,
  openWorkspaceTaskDiagnostic,
  onOpenRequest,
  previewWorkspaceTaskControl,
  rejectWorkspaceTaskControl,
  renewWorkspaceTaskControl,
  takePendingOpen,
  updateService,
  updateJob,
  trustWorkspaceTaskSource,
  trustWorkspaceTaskShellSource,
  type OpenRequest,
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
  WorkspaceTaskApplyResult,
  WorkspaceTaskControlPreview,
  WorkspaceTaskControlReceipt,
  WorkspaceTaskDiagnostics,
  WorkspaceTaskOperation,
  WorkspaceTaskOperationRunStatus,
  WorkspaceTaskOperationStatus,
  WorkspaceTaskState,
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

function shortRevision(revision: string): string {
  return revision ? revision.slice(0, 8) : "--------";
}

function workspaceSourceLabel(sourceRoot: string): string {
  const normalized = sourceRoot.replace(/[\\/]+$/, "");
  const parts = normalized.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? sourceRoot;
}

function canUseWorkspaceTask(
  task: WorkspaceTaskState | undefined,
  snapshotFresh: boolean,
): boolean {
  return snapshotFresh && (!task || workspaceTaskGateCode(task) === null);
}

/** Return the stable native gate code for the first failed workspace guard. */
function workspaceTaskGateCode(task: WorkspaceTaskState | undefined): string | null {
  if (!task) return null;
  if (!task.trusted) return "source-untrusted";
  if (!task.available) return "unavailable";
  if (task.taskKind === "shell" && !task.shellTrusted) return "shell-untrusted";
  return null;
}

function workspaceTaskGateHint(task: WorkspaceTaskState | undefined): string {
  switch (workspaceTaskGateCode(task)) {
    case "source-untrusted":
      return "workspace task source 승인이 필요합니다.";
    case "unavailable":
      return "workspace task 원본이 변경되어 다시 가져와야 합니다.";
    case "shell-untrusted":
      return "셸 실행 별도 승인이 필요합니다.";
    default:
      return "workspace task 상태를 다시 확인해야 합니다.";
  }
}

const WORKSPACE_OPERATION_POLL_INTERVAL_MS = 500;
const WORKSPACE_OPERATION_POLL_MAX_MS = 10 * 60 * 1000;
const TASK_CONTROL_HANDOFF_KIND = "task-control/v1";
const TASK_CONTROL_RENEW_AFTER_MS = 30 * 1000;
const TASK_CONTROL_RENEW_INTERVAL_MS = 20 * 1000;
const TASK_CONTROL_MAX_RENEWALS = 24;

function isWorkspaceOperationTerminal(status: WorkspaceTaskOperationStatus): boolean {
  return status === "succeeded" || status === "failed" || status === "cancelled";
}

function workspaceOperationStatusLabel(status: WorkspaceTaskOperationStatus): string {
  switch (status) {
    case "queued": return "대기 중";
    case "running": return "실행 중";
    case "stopping": return "중지 중";
    case "succeeded": return "완료";
    case "failed": return "실패";
    case "cancelled": return "취소됨";
  }
}

function workspaceOperationRunStatusLabel(status: WorkspaceTaskOperationRunStatus): string {
  switch (status) {
    case "pending": return "대기 중";
    case "launching": return "시작 중";
    case "running": return "실행 중";
    case "succeeded": return "완료";
    case "failed": return "실패";
    case "cancelled": return "취소됨";
    case "skipped": return "건너뜀";
  }
}

function workspaceOperationProgressLabel(operation: WorkspaceTaskOperation): string {
  const completed = operation.runs.filter((run) =>
    run.status === "succeeded"
    || run.status === "failed"
    || run.status === "cancelled"
    || run.status === "skipped",
  ).length;
  return `child ${completed}/${operation.runs.length}`;
}

function isWorkspaceOperationRunTerminal(status: WorkspaceTaskOperationRunStatus): boolean {
  return status === "succeeded"
    || status === "failed"
    || status === "cancelled"
    || status === "skipped";
}

function taskControlActionLabel(action: WorkspaceTaskControlPreview["action"]): string {
  return action === "start" ? "시작" : "중지";
}

function taskControlReceiptStatusLabel(status: WorkspaceTaskControlReceipt["status"]): string {
  switch (status) {
    case "accepted": return "승인됨";
    case "rejected": return "거절됨";
    case "started": return "시작됨";
    case "stopped": return "중지됨";
    case "failed": return "실패";
  }
}

interface WorkspaceDiagnosticState {
  status: "loading" | "ready" | "error";
  diagnostics?: WorkspaceTaskDiagnostics;
  error?: string;
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
  const [workspaceTasks, setWorkspaceTasks] = useState<WorkspaceTaskState[]>([]);
  const [workspaceSnapshotFresh, setWorkspaceSnapshotFresh] = useState(false);
  const [services, setServices] = useState<Job[]>([]);
  const [serviceInstances, setServiceInstances] = useState<Record<string, ServiceInstance>>({});
  const [obsMap, setObsMap] = useState<Record<string, ServiceObservability | null>>({});
  const [obsOpen, setObsOpen] = useState<Record<string, boolean>>({});
  const [importOpen, setImportOpen] = useState(false);
  const importTriggerRef = useRef<HTMLButtonElement>(null);
  const [activeRuns, setActiveRuns] = useState<Record<string, Run | null>>({});
  const [workspaceOperations, setWorkspaceOperations] = useState<Record<string, WorkspaceTaskOperation>>({});
  const [workspaceDiagnostics, setWorkspaceDiagnostics] = useState<Record<string, WorkspaceDiagnosticState>>({});
  const [taskControlPreview, setTaskControlPreview] = useState<WorkspaceTaskControlPreview | null>(null);
  const [taskControlReceipt, setTaskControlReceipt] = useState<WorkspaceTaskControlReceipt | null>(null);
  const [taskControlReceipts, setTaskControlReceipts] = useState<WorkspaceTaskControlReceipt[]>([]);
  const [taskControlLeaseUntil, setTaskControlLeaseUntil] = useState<number | null>(null);
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
  const [workspaceNotice, setWorkspaceNotice] = useState<string | null>(null);
  const [launcherTask, setLauncherTask] = useState<{ id: string; kind: "job" | "service" } | null>(null);
  const [shellTrustTask, setShellTrustTask] = useState<WorkspaceTaskState | null>(null);
  const historyDefinitions = useMemo(() => [...jobs, ...services], [jobs, services]);
  const workspaceTaskByJobId = useMemo(
    () => new Map(workspaceTasks.map((task) => [task.jobId, task])),
    [workspaceTasks],
  );
  const workspaceOperationByRootJobId = useMemo(() => {
    const latest = new Map<string, WorkspaceTaskOperation>();
    for (const operation of Object.values(workspaceOperations)) {
      const current = latest.get(operation.rootJobId);
      const operationActive = !isWorkspaceOperationTerminal(operation.status);
      const currentActive = current ? !isWorkspaceOperationTerminal(current.status) : false;
      if (!current
        || (operationActive && !currentActive)
        || (operationActive === currentActive
          && (operation.createdAt > current.createdAt
            || (operation.createdAt === current.createdAt && operation.id > current.id)))) {
        latest.set(operation.rootJobId, operation);
      }
    }
    return latest;
  }, [workspaceOperations]);

  const closeImport = useCallback(() => {
    setImportOpen(false);
    window.setTimeout(() => importTriggerRef.current?.focus(), 0);
  }, []);
  const activeRefresh = useRef<{ promise: Promise<void> | null; pending: boolean; generation: number }>({
    promise: null,
    pending: false,
    generation: 0,
  });
  const openRequestRef = useRef<(request: OpenRequest) => void>(() => undefined);
  const launcherCancelRef = useRef<HTMLButtonElement>(null);
  const shellTrustCancelRef = useRef<HTMLButtonElement>(null);
  const shellTrustRestoreRef = useRef<HTMLElement | null>(null);
  const taskControlCancelRef = useRef<HTMLButtonElement>(null);
  const taskControlRestoreRef = useRef<HTMLElement | null>(null);
  const taskControlRenewTimerRef = useRef<number | null>(null);
  const taskControlRenewIntervalRef = useRef<number | null>(null);
  const taskControlRenewCountRef = useRef(0);
  const workspaceDiagnosticsRequestedRef = useRef(new Set<string>());
  const workspaceOperationTimersRef = useRef(new Map<string, number>());
  const workspaceOperationPollHealthyAtRef = useRef(new Map<string, number>());
  const mountedRef = useRef(false);

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
    setWorkspaceSnapshotFresh(false);
    try {
      const [nextJobs, nextWorkspaceTasks] = await Promise.all([
        listJobs(),
        listWorkspaceTasks(),
      ]);
      setJobs(nextJobs);
      setWorkspaceTasks(nextWorkspaceTasks);
      setWorkspaceSnapshotFresh(true);
      await refreshActiveRuns();
    } catch (cause) {
      setWorkspaceSnapshotFresh(false);
      throw cause;
    }
  }, [refreshActiveRuns]);

  const refreshServices = useCallback(async () => {
    const snapshot = await loadServiceSnapshot();
    setServices(snapshot.services);
    setServiceInstances(snapshot.instances);
  }, []);

  const stopWorkspaceOperationPolling = useCallback((operationId: string) => {
    const timer = workspaceOperationTimersRef.current.get(operationId);
    if (timer !== undefined) window.clearTimeout(timer);
    workspaceOperationTimersRef.current.delete(operationId);
    workspaceOperationPollHealthyAtRef.current.delete(operationId);
  }, []);

  const pollWorkspaceTaskOperation = useCallback(async (operationId: string): Promise<void> => {
    if (!mountedRef.current || !workspaceOperationPollHealthyAtRef.current.has(operationId)) return;
    workspaceOperationTimersRef.current.delete(operationId);
    const lastHealthyAt = workspaceOperationPollHealthyAtRef.current.get(operationId) ?? Date.now();

    try {
      const operation = await getWorkspaceTaskOperation(operationId);
      if (!mountedRef.current || !workspaceOperationPollHealthyAtRef.current.has(operationId)) return;
      if (!operation) {
        stopWorkspaceOperationPolling(operationId);
        setError(friendlyErrorMessage("workspace-task-operation-not-found"));
        return;
      }
      setWorkspaceOperations((previous) => ({ ...previous, [operation.id]: operation }));
      if (isWorkspaceOperationTerminal(operation.status)) {
        stopWorkspaceOperationPolling(operationId);
        return;
      }
      // A workspace task may legitimately run longer than ten minutes. Keep
      // following it while the native DB remains readable; the bound below is
      // for a continuously broken polling channel, not operation duration.
      workspaceOperationPollHealthyAtRef.current.set(operationId, Date.now());
    } catch (cause) {
      if (!mountedRef.current || !workspaceOperationPollHealthyAtRef.current.has(operationId)) return;
      if (Date.now() - lastHealthyAt >= WORKSPACE_OPERATION_POLL_MAX_MS) {
        stopWorkspaceOperationPolling(operationId);
        setError(friendlyErrorMessage(cause));
        return;
      }
    }

    if (!mountedRef.current || !workspaceOperationPollHealthyAtRef.current.has(operationId)) return;
    const timer = window.setTimeout(() => {
      workspaceOperationTimersRef.current.delete(operationId);
      void pollWorkspaceTaskOperation(operationId);
    }, WORKSPACE_OPERATION_POLL_INTERVAL_MS);
    workspaceOperationTimersRef.current.set(operationId, timer);
  }, [stopWorkspaceOperationPolling]);

  const trackWorkspaceTaskOperation = useCallback((operation: WorkspaceTaskOperation) => {
    if (!mountedRef.current) return;
    setWorkspaceOperations((previous) => ({ ...previous, [operation.id]: operation }));
    if (isWorkspaceOperationTerminal(operation.status)) {
      stopWorkspaceOperationPolling(operation.id);
      return;
    }
    if (!workspaceOperationPollHealthyAtRef.current.has(operation.id)) {
      workspaceOperationPollHealthyAtRef.current.set(operation.id, Date.now());
    }
    if (!workspaceOperationTimersRef.current.has(operation.id)) {
      void pollWorkspaceTaskOperation(operation.id);
    }
  }, [pollWorkspaceTaskOperation, stopWorkspaceOperationPolling]);

  const refreshWorkspaceOperations = useCallback(async () => {
    const operations = await listWorkspaceTaskOperations(100);
    if (!mountedRef.current) return;
    for (const operation of operations) trackWorkspaceTaskOperation(operation);
  }, [trackWorkspaceTaskOperation]);

  const loadWorkspaceTaskDiagnostics = useCallback(async (runId: string) => {
    if (!mountedRef.current) return;
    setWorkspaceDiagnostics((previous) => ({
      ...previous,
      [runId]: { status: "loading" },
    }));
    try {
      const diagnostics = await listWorkspaceTaskDiagnostics(runId);
      if (!mountedRef.current) return;
      setWorkspaceDiagnostics((previous) => ({
        ...previous,
        [runId]: { status: "ready", diagnostics },
      }));
    } catch (cause) {
      if (!mountedRef.current) return;
      setWorkspaceDiagnostics((previous) => ({
        ...previous,
        [runId]: { status: "error", error: friendlyErrorMessage(cause) },
      }));
    }
  }, []);

  const retryWorkspaceTaskDiagnostics = useCallback((runId: string) => {
    workspaceDiagnosticsRequestedRef.current.delete(runId);
    workspaceDiagnosticsRequestedRef.current.add(runId);
    void loadWorkspaceTaskDiagnostics(runId);
  }, [loadWorkspaceTaskDiagnostics]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      for (const timer of workspaceOperationTimersRef.current.values()) window.clearTimeout(timer);
      workspaceOperationTimersRef.current.clear();
      workspaceOperationPollHealthyAtRef.current.clear();
      workspaceDiagnosticsRequestedRef.current.clear();
      if (taskControlRenewTimerRef.current !== null) {
        window.clearTimeout(taskControlRenewTimerRef.current);
        taskControlRenewTimerRef.current = null;
      }
      if (taskControlRenewIntervalRef.current !== null) {
        window.clearInterval(taskControlRenewIntervalRef.current);
        taskControlRenewIntervalRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    for (const operation of Object.values(workspaceOperations)) {
      for (const run of operation.runs) {
        if (!run.runId || !isWorkspaceOperationRunTerminal(run.status)) continue;
        const task = workspaceTaskByJobId.get(run.jobId);
        if (!task?.hasProblemMatcher || workspaceDiagnosticsRequestedRef.current.has(run.runId)) continue;
        workspaceDiagnosticsRequestedRef.current.add(run.runId);
        void loadWorkspaceTaskDiagnostics(run.runId);
      }
    }
  }, [loadWorkspaceTaskDiagnostics, workspaceOperations, workspaceTaskByJobId]);

  const refreshTaskControlReceipts = useCallback(async () => {
    const receipts = await listWorkspaceTaskControlReceipts(20);
    if (mountedRef.current) setTaskControlReceipts(receipts);
  }, []);

  const handleTaskControlHandoff = useCallback(async (handoffId: string) => {
    if (!mountedRef.current || taskControlPreview) return;
    taskControlRestoreRef.current = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    setTaskControlReceipt(null);
    setTaskControlLeaseUntil(null);
    try {
      const preview = await previewWorkspaceTaskControl(handoffId);
      if (!mountedRef.current) return;
      setTaskControlPreview(preview);
      void refreshTaskControlReceipts().catch((cause) => {
        if (mountedRef.current) setError(friendlyErrorMessage(cause));
      });
    } catch (cause) {
      if (mountedRef.current) setError(friendlyErrorMessage(cause));
    }
  }, [refreshTaskControlReceipts, taskControlPreview]);

  const closeTaskControlPreview = useCallback(() => {
    setTaskControlPreview(null);
    setTaskControlLeaseUntil(null);
  }, []);

  const handleAcceptTaskControl = useCallback(async () => {
    const preview = taskControlPreview;
    if (!preview || busy) return;
    setBusy(true);
    try {
      const receipt = await acceptWorkspaceTaskControl(preview.requestId);
      if (mountedRef.current) {
        setTaskControlReceipt(receipt);
        setError(null);
      }
      closeTaskControlPreview();
      await refreshTaskControlReceipts();
      if (receipt.operationId) void refreshWorkspaceOperations().catch((cause) => setError(friendlyErrorMessage(cause)));
      if (receipt.status === "started") void refreshActiveRuns();
    } catch (cause) {
      if (mountedRef.current) setError(friendlyErrorMessage(cause));
      closeTaskControlPreview();
      void refreshTaskControlReceipts().catch((refreshCause) => setError(friendlyErrorMessage(refreshCause)));
    } finally {
      setBusy(false);
    }
  }, [busy, closeTaskControlPreview, refreshActiveRuns, refreshTaskControlReceipts, refreshWorkspaceOperations, taskControlPreview]);

  const handleRejectTaskControl = useCallback(async () => {
    const preview = taskControlPreview;
    if (!preview || busy) return;
    setBusy(true);
    try {
      const receipt = await rejectWorkspaceTaskControl(preview.requestId);
      if (mountedRef.current) {
        setTaskControlReceipt(receipt);
        setError(null);
      }
      closeTaskControlPreview();
      await refreshTaskControlReceipts();
    } catch (cause) {
      if (mountedRef.current) setError(friendlyErrorMessage(cause));
      closeTaskControlPreview();
    } finally {
      setBusy(false);
    }
  }, [busy, closeTaskControlPreview, refreshTaskControlReceipts, taskControlPreview]);

  useEffect(() => {
    const preview = taskControlPreview;
    if (!preview) return;
    let renewCount = 0;
    let renewing = false;
    const renew = async () => {
      if (!mountedRef.current || !taskControlPreview || renewing) return;
      if (renewCount >= TASK_CONTROL_MAX_RENEWALS) {
        if (taskControlRenewIntervalRef.current !== null) {
          window.clearInterval(taskControlRenewIntervalRef.current);
          taskControlRenewIntervalRef.current = null;
        }
        setError(friendlyErrorMessage("task-control-lease-expired"));
        return;
      }
      renewing = true;
      try {
        const leaseUntil = await renewWorkspaceTaskControl(preview.requestId);
        if (mountedRef.current && taskControlPreview?.requestId === preview.requestId) {
          renewCount += 1;
          taskControlRenewCountRef.current = renewCount;
          setTaskControlLeaseUntil(leaseUntil);
        }
      } catch (cause) {
        if (mountedRef.current && taskControlPreview?.requestId === preview.requestId) {
          setError(friendlyErrorMessage(cause));
        }
        if (taskControlRenewIntervalRef.current !== null) {
          window.clearInterval(taskControlRenewIntervalRef.current);
          taskControlRenewIntervalRef.current = null;
        }
      } finally {
        renewing = false;
      }
    };
    taskControlRenewCountRef.current = 0;
    taskControlRenewTimerRef.current = window.setTimeout(() => {
      taskControlRenewTimerRef.current = null;
      void renew();
      taskControlRenewIntervalRef.current = window.setInterval(() => void renew(), TASK_CONTROL_RENEW_INTERVAL_MS);
    }, TASK_CONTROL_RENEW_AFTER_MS);
    return () => {
      if (taskControlRenewTimerRef.current !== null) {
        window.clearTimeout(taskControlRenewTimerRef.current);
        taskControlRenewTimerRef.current = null;
      }
      if (taskControlRenewIntervalRef.current !== null) {
        window.clearInterval(taskControlRenewIntervalRef.current);
        taskControlRenewIntervalRef.current = null;
      }
      taskControlRenewCountRef.current = 0;
    };
  }, [taskControlPreview]);

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
    const workspaceTask = task.kind === "job" ? workspaceTaskByJobId.get(task.id) : undefined;
    const workspaceOperation = task.kind === "job"
      ? workspaceOperationByRootJobId.get(task.id)
      : undefined;
    if (workspaceOperation && !isWorkspaceOperationTerminal(workspaceOperation.status)) {
      setLauncherTask(null);
      setError(friendlyErrorMessage("workspace-task-operation-active"));
      return;
    }
    if (!canUseWorkspaceTask(workspaceTask, workspaceSnapshotFresh)) {
      setLauncherTask(null);
      setError(
        workspaceSnapshotFresh
          ? friendlyErrorMessage(workspaceTaskGateCode(workspaceTask) ?? "unavailable")
          : "workspace task 상태를 확인하지 못해 실행을 차단했습니다. 다시 불러온 뒤 시도하세요.",
      );
      return;
    }
    setBusy(true);
    try {
      if (task.kind === "job") {
        if (workspaceTask) {
          const operation = await runWorkspaceTaskOperation(task.id, true);
          trackWorkspaceTaskOperation(operation);
        } else {
          await runJobNow(task.id);
        }
        await refreshActiveRuns();
      } else {
        await startService(task.id);
        await refreshServices();
      }
    } catch (cause) {
      setError(friendlyErrorMessage(cause));
    } finally {
      setLauncherTask(null);
      setBusy(false);
    }
  };

  openRequestRef.current = (request) => {
    if (request.target.kind === "task") {
      void handleLauncherTask(request.target.id);
    } else if (request.target.kind === "handoff" && request.target.handoffKind === TASK_CONTROL_HANDOFF_KIND) {
      void handleTaskControlHandoff(request.target.id);
    }
  };

  useLayoutEffect(() => {
    if (launcherTask) launcherCancelRef.current?.focus();
    else document.querySelector<HTMLElement>(".job-card.selected")?.focus();
  }, [launcherTask]);

  useLayoutEffect(() => {
    if (shellTrustTask) {
      shellTrustCancelRef.current?.focus();
    } else {
      shellTrustRestoreRef.current?.focus();
      shellTrustRestoreRef.current = null;
    }
  }, [shellTrustTask]);

  useLayoutEffect(() => {
    if (taskControlPreview) {
      taskControlCancelRef.current?.focus();
    } else {
      taskControlRestoreRef.current?.focus();
      taskControlRestoreRef.current = null;
    }
  }, [taskControlPreview]);

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

  const onShellTrustDialogKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      if (!busy) setShellTrustTask(null);
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

  const onTaskControlDialogKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      if (!busy) void handleRejectTaskControl();
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
      setStatusError(friendlyErrorMessage(cause));
    }
  };

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await loadRuntimeStatus());
      setStatusError(null);
    } catch (cause) {
      setStatusError(friendlyErrorMessage(cause));
    }
  }, []);

  useEffect(() => {
    let active = true;
    void refreshTaskControlReceipts().catch((cause: unknown) => {
      if (active) setError(friendlyErrorMessage(cause));
    });
    void refreshWorkspaceOperations().catch((cause: unknown) => {
      if (active) setError(friendlyErrorMessage(cause));
    });
    void Promise.all([
      loadRuntimeStatus(),
      listJobs(),
      loadServiceSnapshot(),
      loadStartupShortcutStatus(),
      listWorkspaceTasks(),
    ])
      .then(([nextStatus, nextJobs, serviceSnapshot, nextStartupStatus, nextWorkspaceTasks]) => {
        if (!active) return;
        setStatus(nextStatus);
        setJobs(nextJobs);
        setWorkspaceTasks(nextWorkspaceTasks);
        setWorkspaceSnapshotFresh(true);
        setServices(serviceSnapshot.services);
        setServiceInstances(serviceSnapshot.instances);
        setStartupStatus(nextStartupStatus);
        void refreshActiveRuns();
        setStatusError(null);
      })
      .catch((cause: unknown) => {
        if (active) {
          setWorkspaceSnapshotFresh(false);
          setError(friendlyErrorMessage(cause));
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [refreshActiveRuns, refreshTaskControlReceipts, refreshWorkspaceOperations]);

  // AppLink events are only a wake-up signal. The authoritative request is
  // pulled from the native one-shot slot, then the current job/service list is
  // checked before any run, service action, or task-control handoff is used.
  useEffect(() => {
    if (loading) return;
    const consumePendingOpen = () => {
      void takePendingOpen()
        .then((request) => {
          if (request) openRequestRef.current(request);
        })
        .catch((cause) => setError(friendlyErrorMessage(cause)));
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
    const workspaceTask = workspaceTaskByJobId.get(job.id);
    const workspaceOperation = workspaceOperationByRootJobId.get(job.id);
    if (workspaceOperation && !isWorkspaceOperationTerminal(workspaceOperation.status)) {
      setError(friendlyErrorMessage("workspace-task-operation-active"));
      return;
    }
    if (!canUseWorkspaceTask(workspaceTask, workspaceSnapshotFresh)) {
      setError(
        workspaceSnapshotFresh
          ? friendlyErrorMessage(workspaceTaskGateCode(workspaceTask) ?? "unavailable")
          : "workspace task 상태를 확인하지 못해 실행을 차단했습니다. 다시 불러온 뒤 시도하세요.",
      );
      return;
    }
    setBusy(true);
    try {
      if (workspaceTask) {
        const operation = await runWorkspaceTaskOperation(job.id, true);
        trackWorkspaceTaskOperation(operation);
      } else {
        await runJobNow(job.id);
      }
      // The overlap policy may return a queued/skipped row. Refresh the
      // process-only snapshot so the card is never marked running by guess.
      await refreshActiveRuns();
      setError(null);
    } catch (cause) {
      setError(friendlyErrorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  const handleStopRun = async (job: Job) => {
    const workspaceOperation = workspaceOperationByRootJobId.get(job.id);
    const operationActive = workspaceOperation && !isWorkspaceOperationTerminal(workspaceOperation.status);
    if (!window.confirm(
      operationActive
        ? `'${job.name}' workspace task orchestration을 중지할까요?`
        : `'${job.name}' 작업의 활성 실행을 중지할까요?`,
    )) return;
    setBusy(true);
    try {
      if (operationActive && workspaceOperation) {
        const operation = await stopWorkspaceTaskOperation(workspaceOperation.id);
        trackWorkspaceTaskOperation(operation);
      } else {
        await stopActiveRun(job.id);
      }
      await refreshActiveRuns();
      setError(null);
    } catch (cause) {
      setError(friendlyErrorMessage(cause));
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
      setError(friendlyErrorMessage(cause));
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
      setError(friendlyErrorMessage(cause));
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
      setError(friendlyErrorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  const handleOpenWorkspaceTaskDiagnostic = async (runId: string, diagnosticIndex: number) => {
    try {
      const opened = await openWorkspaceTaskDiagnostic(runId, diagnosticIndex);
      if (!opened) throw new Error("workspace-task-diagnostic-launch-failed");
      setError(null);
    } catch (cause) {
      setError(friendlyErrorMessage(cause));
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

  const editingWorkspaceTask = useMemo(
    () => (editingJobId ? workspaceTaskByJobId.get(editingJobId) ?? null : null),
    [editingJobId, workspaceTaskByJobId],
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
      setError(friendlyErrorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  const handleToggleJob = async (job: Job) => {
    const workspaceTask = workspaceTaskByJobId.get(job.id);
    if (!job.enabled && workspaceTask && workspaceTask.dependsOn.length > 0) {
      setError(friendlyErrorMessage("workspace-task-orchestration-manual-only"));
      return;
    }
    if (!job.enabled && !canUseWorkspaceTask(workspaceTask, workspaceSnapshotFresh)) {
      setError(
        workspaceSnapshotFresh
          ? friendlyErrorMessage(workspaceTaskGateCode(workspaceTask) ?? "unavailable")
          : "workspace task 상태를 확인하지 못해 활성화를 차단했습니다. 다시 불러온 뒤 시도하세요.",
      );
      return;
    }
    setBusy(true);
    try {
      await setJobEnabled(job.id, !job.enabled);
      await refreshJobs();
      setError(null);
    } catch (cause) {
      setError(friendlyErrorMessage(cause));
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
      setError(friendlyErrorMessage(cause));
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
      setError(friendlyErrorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  const handleTrustWorkspaceTask = async (task: WorkspaceTaskState) => {
    if (busy || task.trusted) return;
    const approved = window.confirm(
      `현재 source revision ${shortRevision(task.revision)}을 신뢰할까요?\n` +
      "이 승인은 이 revision을 실행 대상으로 사용할 수 있도록 권한을 부여하지만, task를 실행하거나 프로세스를 시작하지 않습니다.",
    );
    if (!approved) return;
    setBusy(true);
    setWorkspaceNotice(null);
    try {
      await trustWorkspaceTaskSource(task.sourceId, task.revision);
      await refreshJobs();
      setWorkspaceNotice(`source revision ${shortRevision(task.revision)}을 승인했습니다. 작업은 자동 실행되지 않았습니다.`);
      setError(null);
    } catch (cause) {
      setError(friendlyErrorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  const openShellTrustConfirmation = (task: WorkspaceTaskState) => {
    if (busy || task.taskKind !== "shell" || !task.trusted || task.shellTrusted || !task.available) {
      if (task.taskKind === "shell" && task.trusted && !task.available) {
        setError(friendlyErrorMessage("unavailable"));
      }
      return;
    }
    setError(null);
    shellTrustRestoreRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setShellTrustTask(task);
  };

  const handleTrustWorkspaceTaskShell = async () => {
    const task = shellTrustTask;
    if (!task || busy) return;
    setBusy(true);
    setWorkspaceNotice(null);
    try {
      await trustWorkspaceTaskShellSource(task.sourceId, task.revision);
      await refreshJobs();
      setShellTrustTask(null);
      setWorkspaceNotice(`source revision ${shortRevision(task.revision)}의 셸 실행을 승인했습니다. 작업은 자동 실행되지 않았습니다.`);
      setError(null);
    } catch (cause) {
      setError(friendlyErrorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  const jobContextItems = useMemo<readonly ContextMenuEntry[]>(() => {
    if (!contextJob) return [];
    const workspaceTask = workspaceTaskByJobId.get(contextJob.id);
    const workspaceOperation = workspaceOperationByRootJobId.get(contextJob.id);
    const operationActive = workspaceOperation !== undefined
      && !isWorkspaceOperationTerminal(workspaceOperation.status);
    const workspaceRunnable = canUseWorkspaceTask(workspaceTask, workspaceSnapshotFresh);
    return [
      {
        type: "item",
        id: "run-now",
        label: "지금 실행",
        disabled: busy || !workspaceRunnable || operationActive,
      },
      {
        type: "item",
        id: "toggle-enabled",
        label: contextJob.enabled ? "비활성화" : "활성화",
        disabled: busy || (!contextJob.enabled
          && (!workspaceRunnable || Boolean(workspaceTask?.dependsOn.length))),
      },
      { type: "item", id: "edit", label: "편집", disabled: busy },
      { type: "item", id: "open-logs", label: "로그 열기" },
      { type: "separator", id: "job-danger-separator" },
      {
        type: "item",
        id: "delete",
        label: "삭제",
        disabled: busy || !activeSnapshotFresh || Boolean(activeRuns[contextJob.id]) || operationActive,
        danger: true,
      },
    ];
  }, [
    activeRuns,
    activeSnapshotFresh,
    busy,
    contextJob,
    workspaceOperationByRootJobId,
    workspaceSnapshotFresh,
    workspaceTaskByJobId,
  ]);

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

  const launcherWorkspaceTask = launcherTask?.kind === "job"
    ? workspaceTaskByJobId.get(launcherTask.id)
    : undefined;
  const launcherWorkspaceOperation = launcherTask?.kind === "job"
    ? workspaceOperationByRootJobId.get(launcherTask.id)
    : undefined;
  const launcherOperationActive = launcherWorkspaceOperation !== undefined
    && !isWorkspaceOperationTerminal(launcherWorkspaceOperation.status);
  const visibleTaskControlReceipts = taskControlReceipt
    && !taskControlReceipts.some((receipt) => receipt.requestId === taskControlReceipt.requestId)
    ? [taskControlReceipt, ...taskControlReceipts]
    : taskControlReceipts;

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
        {workspaceNotice ? <div className="success-banner" role="status">{workspaceNotice}</div> : null}
        {visibleTaskControlReceipts.length > 0 ? (
          <section className="task-control-receipts" aria-labelledby="task-control-receipts-title">
            <h3 id="task-control-receipts-title">최근 task-control 내역</h3>
            <ul>
              {visibleTaskControlReceipts.slice(0, 5).map((receipt) => {
                const task = workspaceTaskByJobId.get(receipt.taskId);
                const definition = jobs.find((candidate) => candidate.id === receipt.taskId);
                return (
                  <li key={receipt.requestId}>
                    <span>{task?.label ?? definition?.name ?? receipt.taskId}</span>
                    <span>{taskControlActionLabel(receipt.action)} · {taskControlReceiptStatusLabel(receipt.status)}</span>
                    {receipt.failureCode ? <span className="workspace-task-unavailable">{friendlyErrorMessage(receipt.failureCode)}</span> : null}
                  </li>
                );
              })}
            </ul>
          </section>
        ) : null}

        {screen === "editor" ? (
          <JobEditor job={editingJob} workspaceTask={editingWorkspaceTask} onSave={handleSave} onCancel={closeEditor} />
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
              <button ref={importTriggerRef} type="button" className="button-secondary" onClick={() => setImportOpen(true)}>정의와 task 가져오기</button>
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
                {jobs.map((job) => {
                  const workspaceTask = workspaceTaskByJobId.get(job.id);
                  const workspaceOperation = workspaceOperationByRootJobId.get(job.id);
                  const workspaceOperationActive = workspaceOperation !== undefined
                    && !isWorkspaceOperationTerminal(workspaceOperation.status);
                  const workspaceRunnable = canUseWorkspaceTask(workspaceTask, workspaceSnapshotFresh);
                  const operationChildProgress = workspaceOperation?.runs
                    .map((run) => {
                      const childJob = jobs.find((candidate) => candidate.id === run.jobId);
                      return `${childJob?.name ?? run.jobId}: ${workspaceOperationRunStatusLabel(run.status)}`;
                    })
                    .join(" · ");
                  const diagnosticRuns = workspaceOperation?.runs.filter((run) => {
                    const task = workspaceTaskByJobId.get(run.jobId);
                    return Boolean(run.runId)
                      && isWorkspaceOperationRunTerminal(run.status)
                      && task?.hasProblemMatcher === true;
                  }) ?? [];
                  return (
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
                        <span className={`job-state ${workspaceOperationActive || activeRuns[job.id] ? "running" : job.enabled ? "enabled" : "disabled"}`}>
                          {workspaceOperationActive
                            ? `오케스트레이션 ${workspaceOperationStatusLabel(workspaceOperation!.status)}`
                            : activeRuns[job.id] ? "실행 중" : job.enabled ? "활성" : "비활성"}
                        </span>
                      </div>
                      <code title={job.command}>{job.command}</code>
                      <div className="job-meta">
                        <span>{targetLabel(job)}</span>
                        <span>{scheduleLabel(job)}</span>
                        <span>{job.overlapPolicy === "skip" ? "중복 건너뛰기" : job.overlapPolicy === "queue" ? "대기열" : "이전 종료"}</span>
                        {job.envConfigured ? <span className="secret-badge">환경변수 보호됨</span> : null}
                        {workspaceTask ? (
                          <>
                            <span className="workspace-task-badge">VS Code {workspaceTask.taskKind}</span>
                            <span title={workspaceTask.sourceRoot}>소스 {workspaceSourceLabel(workspaceTask.sourceRoot)} · rev {shortRevision(workspaceTask.revision)}</span>
                            <span className={workspaceTask.trusted ? "workspace-task-trusted" : "workspace-task-untrusted"}>
                              {workspaceTask.trusted ? "소스 승인됨" : "소스 승인 필요"}
                            </span>
                            {workspaceTask.taskKind === "shell" ? (
                              <span className={workspaceTask.shellTrusted ? "workspace-task-trusted" : "workspace-task-untrusted"}>
                                {workspaceTask.shellTrusted ? "셸 실행 승인됨" : "셸 실행 승인 필요"}
                              </span>
                            ) : null}
                            <span className={workspaceTask.available ? "workspace-task-trusted" : "workspace-task-unavailable"}>
                              {workspaceTask.available ? "원본 사용 가능" : "원본 변경됨 · 사용 불가"}
                            </span>
                            {workspaceTask.dependsOn.length > 0 ? (
                              <span>선행 task: {workspaceTask.dependsOn.join(", ")} · {workspaceTask.dependsOrder === "sequence" ? "순차" : "병렬"}</span>
                            ) : null}
                            {workspaceTask.hasProblemMatcher ? <span>problem matcher 지원됨</span> : null}
                            {workspaceTask.environmentKeys.length > 0 ? <span>환경 키: {workspaceTask.environmentKeys.join(", ")}</span> : null}
                          </>
                        ) : null}
                        {workspaceOperation ? (
                          <>
                            <span
                              className={`workspace-operation-badge workspace-operation-${workspaceOperation.status}`}
                              aria-live="polite"
                              aria-label={`workspace task operation 상태: ${workspaceOperationStatusLabel(workspaceOperation.status)}`}
                            >
                              오케스트레이션 {workspaceOperationStatusLabel(workspaceOperation.status)} · {workspaceOperationProgressLabel(workspaceOperation)}
                            </span>
                            {operationChildProgress ? (
                              <span title={operationChildProgress}>child 진행: {operationChildProgress}</span>
                            ) : null}
                            {workspaceOperation.failureCode ? (
                              <span className="workspace-task-unavailable">{friendlyErrorMessage(workspaceOperation.failureCode)}</span>
                            ) : null}
                          </>
                        ) : null}
                      </div>
                      {diagnosticRuns.length > 0 ? (
                        <div className="workspace-diagnostics" aria-label={`${job.name} diagnostics`}>
                          <strong>problem matcher diagnostics</strong>
                          {diagnosticRuns.map((run) => {
                            const runId = run.runId!;
                            const state = workspaceDiagnostics[runId];
                            const childJob = jobs.find((candidate) => candidate.id === run.jobId);
                            return (
                              <div className="workspace-diagnostic-run" key={runId}>
                                <span className="workspace-diagnostic-run-label">{childJob?.name ?? run.jobId}</span>
                                {state?.status === "loading" || !state ? <span>diagnostics 불러오는 중…</span> : null}
                                {state?.status === "error" ? (
                                  <>
                                    <span className="workspace-task-unavailable">{state.error}</span>
                                    <button
                                      type="button"
                                      className="button-secondary small"
                                      onClick={() => retryWorkspaceTaskDiagnostics(runId)}
                                    >다시 시도</button>
                                  </>
                                ) : null}
                                {state?.status === "ready" ? (
                                  <>
                                    {state.diagnostics?.items.length ? (
                                      <div className="workspace-diagnostic-items">
                                        {state.diagnostics.items.map((item) => {
                                          const location = `${item.file}:${item.line}${item.column ? `:${item.column}` : ""}`;
                                          return (
                                            <button
                                              type="button"
                                              className="workspace-diagnostic-item"
                                              key={`${runId}:${item.index}`}
                                              aria-label={`${location} ${item.message}`}
                                              title={`${location} · ${item.message}`}
                                              onClick={() => void handleOpenWorkspaceTaskDiagnostic(runId, item.index)}
                                            >
                                              <span>{location}</span>
                                              <span>{item.message}</span>
                                              <span>{item.severity ?? "진단"} · {item.stream}</span>
                                            </button>
                                          );
                                        })}
                                      </div>
                                    ) : <span>진단 없음</span>}
                                    {state.diagnostics?.truncated ? <span className="workspace-diagnostics-truncated">일부 diagnostics만 표시됨</span> : null}
                                  </>
                                ) : null}
                              </div>
                            );
                          })}
                        </div>
                      ) : null}
                    </div>
                    <div className="job-actions">
                      <button
                        type="button"
                        className="button-primary"
                        disabled={busy || !workspaceRunnable || workspaceOperationActive}
                        title={workspaceOperationActive
                          ? "이미 workspace task orchestration이 실행 중입니다."
                          : !workspaceRunnable
                          ? workspaceSnapshotFresh
                            ? workspaceTaskGateHint(workspaceTask)
                            : "workspace task 상태를 다시 불러와야 합니다."
                          : undefined}
                        onClick={() => void handleRunNow(job)}
                      >{workspaceOperationActive ? "실행 중…" : "지금 실행"}</button>
                      <button
                        type="button"
                        className="button-danger"
                        disabled={busy || (workspaceOperationActive
                          ? workspaceOperation?.status === "stopping"
                          : !activeSnapshotFresh || !activeRuns[job.id])}
                        onClick={() => void handleStopRun(job)}
                      >{workspaceOperationActive && workspaceOperation?.status === "stopping" ? "중지 중…" : workspaceOperationActive ? "오케스트레이션 중지" : "중지"}</button>
                      {workspaceTask && !workspaceTask.trusted ? (
                        <button
                          type="button"
                          className="button-secondary"
                          disabled={busy || !workspaceTask.available}
                          onClick={() => void handleTrustWorkspaceTask(workspaceTask)}
                        >소스 승인</button>
                      ) : null}
                      {workspaceTask && workspaceTask.taskKind === "shell" && workspaceTask.trusted && !workspaceTask.shellTrusted ? (
                        <button
                          type="button"
                          className="button-danger"
                          disabled={busy || !workspaceTask.available}
                          onClick={() => openShellTrustConfirmation(workspaceTask)}
                        >셸 실행 승인</button>
                      ) : null}
                      <button type="button" className="button-secondary" onClick={() => openEdit(job)}>편집</button>
                      <button type="button" className="button-danger" disabled={busy || !activeSnapshotFresh || Boolean(activeRuns[job.id]) || workspaceOperationActive} onClick={() => void handleDelete(job)}>삭제</button>
                    </div>
                  </article>
                  );
                })}
              </div>
            ) : null}
          </section>
        )}
      </section>
      {importOpen && (
        <ImportDialog
          onDone={(_created, result: WorkspaceTaskApplyResult | undefined) => {
            if (result) {
              setWorkspaceNotice(
                `workspace task import 완료: 생성 ${result.created} · 갱신 ${result.updated} · 사용 불가 전환 ${result.madeUnavailable} · 충돌 건너뜀 ${result.skippedConflicts}. source revision 승인 후에만 활성화할 수 있습니다.`,
              );
            }
            closeImport();
            void refreshServices().catch((cause) => setError(friendlyErrorMessage(cause)));
            void refreshJobs().catch((cause) => setError(friendlyErrorMessage(cause)));
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
                ? launcherWorkspaceTask
                  ? "선행 dependency를 포함한 workspace task orchestration을 한 번 실행합니다."
                  : "현재 저장된 작업을 한 번 실행합니다."
                : "현재 저장된 서비스를 시작합니다."}
            </p>
            {launcherOperationActive ? (
              <div className="workspace-task-notice" role="note">
                이 workspace task는 이미 {workspaceOperationStatusLabel(launcherWorkspaceOperation!.status)} 상태입니다.
              </div>
            ) : null}
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
                disabled={busy || launcherOperationActive}
                onClick={() => void confirmLauncherTask()}
              >
                {launcherOperationActive ? "이미 실행 중" : launcherTask.kind === "job" ? "실행" : "시작"}
              </button>
            </div>
          </section>
        </div>
      )}
      {shellTrustTask && (
        <div className="modal-backdrop" role="presentation">
          <section
            className="launcher-task-dialog shell-trust-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="shell-trust-title"
            aria-describedby="shell-trust-description"
            onKeyDown={onShellTrustDialogKeyDown}
          >
            <h2 id="shell-trust-title">셸 실행 승인</h2>
            <p id="shell-trust-description">
              <strong>{shellTrustTask.label}</strong> task가 source의 셸 명령을 실행하도록 별도로 승인합니다.
              셸 명령은 source revision에 따라 바뀔 수 있으며, 승인 후 스케줄 실행·수동 실행에서 실제 셸을 호출할 수 있습니다.
              현재 revision <code>{shortRevision(shellTrustTask.revision)}</code>만 승인되며, source가 변경되면 승인이 무효화됩니다.
            </p>
            <dl className="workspace-task-details shell-trust-details">
              <div><dt>source</dt><dd><code>{shellTrustTask.sourceRoot}</code></dd></div>
              <div><dt>명령</dt><dd><code>{jobs.find((job) => job.id === shellTrustTask.jobId)?.command ?? "source revision에서 읽음"}</code></dd></div>
            </dl>
            <div className="workspace-task-notice" role="note">
              이 확인은 일반 source 승인과 별개입니다. 셸 실행 위험을 이해했고 이 source의 셸 task를 실행하겠다면 아래 버튼을 선택하세요.
            </div>
            <div className="launcher-task-actions">
              <button
                ref={shellTrustCancelRef}
                type="button"
                className="button-secondary"
                disabled={busy}
                onClick={() => setShellTrustTask(null)}
              >
                취소
              </button>
              <button
                type="button"
                className="button-danger"
                disabled={busy}
                onClick={() => void handleTrustWorkspaceTaskShell()}
              >
                셸 실행 승인
              </button>
            </div>
          </section>
        </div>
      )}
      {taskControlPreview && (
        <div className="modal-backdrop" role="presentation">
          <section
            className="launcher-task-dialog task-control-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="task-control-title"
            aria-describedby="task-control-description"
            onKeyDown={onTaskControlDialogKeyDown}
          >
            <h2 id="task-control-title">Workbench 요청 확인</h2>
            <p id="task-control-description">
              Workbench가 요청한 workspace task <strong>{taskControlActionLabel(taskControlPreview.action)}</strong> 작업을 확인합니다.
              {taskControlPreview.action === "start"
                ? " 승인하면 현재 저장된 source revision을 다시 검증한 뒤 요청된 작업을 수행합니다."
                : " 승인하면 이 task가 root인 Run Manager 소유의 활성 operation만 중지합니다."}
            </p>
            <dl className="workspace-task-details task-control-details">
              <div><dt>task</dt><dd>{taskControlPreview.label}</dd></div>
              <div><dt>종류</dt><dd>{taskControlPreview.taskKind}</dd></div>
              <div><dt>source revision</dt><dd><code>{shortRevision(taskControlPreview.expectedRevision)}</code></dd></div>
              <div><dt>요청</dt><dd>{taskControlActionLabel(taskControlPreview.action)}</dd></div>
            </dl>
            <div className="workspace-task-notice" role="note">
              명령·경로·환경변수는 이 handoff에 포함되지 않으며, 실행 여부는 Run Manager가 다시 검증합니다.
              {taskControlLeaseUntil ? ` 확인 lease 만료 예정: ${new Date(taskControlLeaseUntil).toLocaleTimeString()}` : " 확인 요청은 제한 시간 동안만 유효합니다."}
            </div>
            <div className="launcher-task-actions">
              <button
                ref={taskControlCancelRef}
                type="button"
                className="button-secondary"
                disabled={busy}
                onClick={() => void handleRejectTaskControl()}
              >
                거절
              </button>
              <button
                type="button"
                className="button-primary"
                disabled={busy}
                onClick={() => void handleAcceptTaskControl()}
              >
                승인하고 {taskControlActionLabel(taskControlPreview.action)}
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
