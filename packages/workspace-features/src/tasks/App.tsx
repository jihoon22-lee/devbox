import type { Screen, WorkspaceDiagnosticState } from "./lib/taskViewTypes";
import { useTaskOperations } from "./hooks/useTaskOperations";
import { TaskControlDialog } from "./components/TaskControlDialog";
import { JobSection } from "./components/JobSection";
import { ServiceSection } from "./components/ServiceSection";
import { TaskSidebar } from "./components/TaskSidebar";
import {
  shortRevision,
  canUseWorkspaceTask,
  workspaceTaskGateCode,
  isWorkspaceOperationTerminal,
  workspaceOperationStatusLabel,
  isWorkspaceOperationRunTerminal,
  taskControlActionLabel,
  taskControlReceiptStatusLabel,
} from "./lib/taskPresentation";
import { usePolling } from "@devbox/hooks";
import { ContextMenu, useContextMenu, type ContextMenuEntry } from "@devbox/context-menu";
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
  listWorkspaceTasks,
  listServices,
  listJobs,
  listActiveRuns,
  listWorkspaceTaskControlReceipts,
  listWorkspaceTaskDiagnostics,
  listWorkspaceTaskOperations,
  loadStartupShortcutStatus,
  loadRuntimeStatus,
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
import RuntimeRecovery from "./components/RuntimeRecovery";
import ServiceEditor from "./components/ServiceEditor";
import type {
  Job,
  JobInput,
  TargetKind,
  Run,
  RuntimeStatus,
  ServiceInput,
  ServiceInstance,
  StartupShortcutStatus,
  WorkspaceTaskApplyResult,
  WorkspaceTaskControlPreview,
  WorkspaceTaskControlReceipt,
  WorkspaceTaskOperation,
  WorkspaceTaskState,
} from "./types";
import "./App.css";

const WORKSPACE_OPERATION_POLL_INTERVAL_MS = 500;
const WORKSPACE_OPERATION_POLL_MAX_MS = 10 * 60 * 1000;
const TASK_CONTROL_HANDOFF_KIND = "task-control/v1";
const TASK_CONTROL_RENEW_AFTER_MS = 30 * 1000;
const TASK_CONTROL_RENEW_INTERVAL_MS = 20 * 1000;
const TASK_CONTROL_MAX_RENEWALS = 24;

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
    instances: Object.fromEntries(entries.filter((entry): entry is [string, ServiceInstance] => entry !== null)),
  };
}

export default function App({
  active: visible = true,
  onDirtyChange,
  openTask,
  onTaskConsumed,
  importSource,
}: {
  active?: boolean;
  onDirtyChange?: (dirty: boolean) => void;
  importSource?: { path: string; targetKind: TargetKind; targetDistro: string | null } | null;
  openTask?: { id: string; jobId: string } | null;
  onTaskConsumed?: (id: string) => void;
}) {
  const viewGenerationRef = useRef(0);
  const loadedGenerationRef = useRef(-1);
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
  const { busy, run: runTaskAction } = useTaskOperations();
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
      if (
        !current ||
        (operationActive && !currentActive) ||
        (operationActive === currentActive &&
          (operation.createdAt > current.createdAt ||
            (operation.createdAt === current.createdAt && operation.id > current.id)))
      ) {
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
  const [taskControlRenewReady, setTaskControlRenewReady] = useState<string | null>(null);
  const taskControlRenewCallback = useRef<() => Promise<void> | void>(() => {});
  const workspaceDiagnosticsRequestedRef = useRef(new Set<string>());
  const workspaceOperationTimersRef = useRef(new Map<string, number>());
  const workspaceOperationPollHealthyAtRef = useRef(new Map<string, number>());
  const mountedRef = useRef(false);

  const prepareJobContext = useCallback(
    (target: HTMLElement) => {
      const id = target.dataset.jobId;
      const job = jobs.find((candidate) => candidate.id === id);
      if (!job) return;
      setSelectedJobId(job.id);
      setContextJob(job);
    },
    [jobs],
  );
  const jobContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareJobContext(target),
  });
  const jobContextTrigger = jobContextMenu.triggerProps;

  const prepareServiceContext = useCallback(
    (target: HTMLElement) => {
      const id = target.dataset.serviceId;
      const service = services.find((candidate) => candidate.id === id);
      if (!service) return;
      setSelectedServiceId(service.id);
      setContextService(service);
    },
    [services],
  );
  const serviceContextMenu = useContextMenu({
    onBeforeOpen: (_reason, target) => prepareServiceContext(target),
  });
  const serviceContextTrigger = serviceContextMenu.triggerProps;

  const refreshActiveRuns = useCallback(async () => {
    if (!mountedRef.current) return;
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
          if (!mountedRef.current || generation !== activeRefresh.current.generation) continue;
          setActiveRuns(Object.fromEntries(runs.map((run) => [run.jobId, run])));
          setActiveSnapshotFresh(true);
          setActiveSnapshotError(null);
        } catch (cause) {
          if (mountedRef.current && generation === activeRefresh.current.generation) {
            setActiveRuns({});
            setActiveSnapshotFresh(false);
            setActiveSnapshotError(friendlyErrorMessage(cause));
          }
        }
      } while (mountedRef.current && activeRefresh.current.pending);
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
    if (!mountedRef.current) return;
    const view = viewGenerationRef.current;
    setWorkspaceSnapshotFresh(false);
    try {
      const [nextJobs, nextWorkspaceTasks] = await Promise.all([listJobs(), listWorkspaceTasks()]);
      if (!mountedRef.current || view !== viewGenerationRef.current) return;
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
    if (!mountedRef.current) return;
    const view = viewGenerationRef.current;
    const snapshot = await loadServiceSnapshot();
    if (!mountedRef.current || view !== viewGenerationRef.current) return;
    setServices(snapshot.services);
    setServiceInstances(snapshot.instances);
  }, []);

  const stopWorkspaceOperationPolling = useCallback((operationId: string) => {
    const timer = workspaceOperationTimersRef.current.get(operationId);
    if (timer !== undefined) window.clearTimeout(timer);
    workspaceOperationTimersRef.current.delete(operationId);
    workspaceOperationPollHealthyAtRef.current.delete(operationId);
  }, []);

  const pollWorkspaceTaskOperation = useCallback(
    async (operationId: string): Promise<void> => {
      if (!mountedRef.current || !workspaceOperationPollHealthyAtRef.current.has(operationId)) return;
      workspaceOperationTimersRef.current.delete(operationId);
      const view = viewGenerationRef.current;
      const lastHealthyAt = workspaceOperationPollHealthyAtRef.current.get(operationId) ?? Date.now();

      try {
        const operation = await getWorkspaceTaskOperation(operationId);
        if (
          !mountedRef.current ||
          view !== viewGenerationRef.current ||
          !workspaceOperationPollHealthyAtRef.current.has(operationId)
        )
          return;
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
        if (
          !mountedRef.current ||
          view !== viewGenerationRef.current ||
          !workspaceOperationPollHealthyAtRef.current.has(operationId)
        )
          return;
        if (Date.now() - lastHealthyAt >= WORKSPACE_OPERATION_POLL_MAX_MS) {
          stopWorkspaceOperationPolling(operationId);
          setError(friendlyErrorMessage(cause));
          return;
        }
      }

      if (
        !mountedRef.current ||
        view !== viewGenerationRef.current ||
        !workspaceOperationPollHealthyAtRef.current.has(operationId)
      )
        return;
      const timer = window.setTimeout(() => {
        workspaceOperationTimersRef.current.delete(operationId);
        void pollWorkspaceTaskOperation(operationId);
      }, WORKSPACE_OPERATION_POLL_INTERVAL_MS);
      workspaceOperationTimersRef.current.set(operationId, timer);
    },
    [stopWorkspaceOperationPolling],
  );

  const trackWorkspaceTaskOperation = useCallback(
    (operation: WorkspaceTaskOperation) => {
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
    },
    [pollWorkspaceTaskOperation, stopWorkspaceOperationPolling],
  );

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

  const retryWorkspaceTaskDiagnostics = useCallback(
    (runId: string) => {
      workspaceDiagnosticsRequestedRef.current.delete(runId);
      workspaceDiagnosticsRequestedRef.current.add(runId);
      void loadWorkspaceTaskDiagnostics(runId);
    },
    [loadWorkspaceTaskDiagnostics],
  );

  useEffect(() => {
    mountedRef.current = visible;
    return () => {
      mountedRef.current = false;
      viewGenerationRef.current += 1;
      activeRefresh.current.generation += 1;
      activeRefresh.current.pending = false;
      for (const timer of workspaceOperationTimersRef.current.values()) window.clearTimeout(timer);
      workspaceOperationTimersRef.current.clear();
      workspaceOperationPollHealthyAtRef.current.clear();
      workspaceDiagnosticsRequestedRef.current.clear();
      if (taskControlRenewTimerRef.current !== null) {
        window.clearTimeout(taskControlRenewTimerRef.current);
        taskControlRenewTimerRef.current = null;
      }
    };
  }, [visible]);

  useEffect(() => {
    if (!visible) return;
    for (const operation of Object.values(workspaceOperations)) {
      for (const run of operation.runs) {
        if (!run.runId || !isWorkspaceOperationRunTerminal(run.status)) continue;
        const task = workspaceTaskByJobId.get(run.jobId);
        if (!task?.hasProblemMatcher || workspaceDiagnosticsRequestedRef.current.has(run.runId)) continue;
        workspaceDiagnosticsRequestedRef.current.add(run.runId);
        void loadWorkspaceTaskDiagnostics(run.runId);
      }
    }
  }, [visible, loadWorkspaceTaskDiagnostics, workspaceOperations, workspaceTaskByJobId]);

  const refreshTaskControlReceipts = useCallback(async () => {
    const receipts = await listWorkspaceTaskControlReceipts(20);
    if (mountedRef.current) setTaskControlReceipts(receipts);
  }, []);

  const handleTaskControlHandoff = useCallback(
    async (handoffId: string) => {
      if (!mountedRef.current || taskControlPreview) return;
      taskControlRestoreRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
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
    },
    [refreshTaskControlReceipts, taskControlPreview],
  );

  const closeTaskControlPreview = useCallback(() => {
    setTaskControlPreview(null);
    setTaskControlLeaseUntil(null);
  }, []);

  const handleAcceptTaskControl = useCallback(async () => {
    const preview = taskControlPreview;
    if (!preview || busy) return;
    return runTaskAction(async () => {
      try {
        const receipt = await acceptWorkspaceTaskControl(preview.requestId);
        if (mountedRef.current) {
          setTaskControlReceipt(receipt);
          setError(null);
        }
        closeTaskControlPreview();
        await refreshTaskControlReceipts();
        if (receipt.operationId)
          void refreshWorkspaceOperations().catch((cause) => setError(friendlyErrorMessage(cause)));
        if (receipt.status === "started") void refreshActiveRuns();
      } catch (cause) {
        if (mountedRef.current) setError(friendlyErrorMessage(cause));
        closeTaskControlPreview();
        void refreshTaskControlReceipts().catch((refreshCause) => setError(friendlyErrorMessage(refreshCause)));
      }
    });
  }, [
    runTaskAction,
    busy,
    closeTaskControlPreview,
    refreshActiveRuns,
    refreshTaskControlReceipts,
    refreshWorkspaceOperations,
    taskControlPreview,
  ]);

  const handleRejectTaskControl = useCallback(async () => {
    const preview = taskControlPreview;
    if (!preview || busy) return;
    return runTaskAction(async () => {
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
      }
    });
  }, [runTaskAction, busy, closeTaskControlPreview, refreshTaskControlReceipts, taskControlPreview]);

  usePolling(() => taskControlRenewCallback.current(), {
    intervalMs: TASK_CONTROL_RENEW_INTERVAL_MS,
    active: visible && Boolean(taskControlPreview) && taskControlRenewReady === taskControlPreview?.requestId,
  });
  useEffect(() => {
    const preview = taskControlPreview;
    setTaskControlRenewReady(null);
    if (!visible || !preview) return;
    let disposed = false;
    let renewCount = 0;
    taskControlRenewCallback.current = async () => {
      if (disposed || !mountedRef.current) return;
      if (renewCount >= TASK_CONTROL_MAX_RENEWALS) {
        setTaskControlRenewReady(null);
        setError(friendlyErrorMessage("task-control-lease-expired"));
        return;
      }
      try {
        const leaseUntil = await renewWorkspaceTaskControl(preview.requestId);
        if (!disposed && mountedRef.current) {
          renewCount += 1;
          setTaskControlLeaseUntil(leaseUntil);
        }
      } catch (cause) {
        if (!disposed && mountedRef.current) {
          setError(friendlyErrorMessage(cause));
          setTaskControlRenewReady(null);
        }
      }
    };
    taskControlRenewTimerRef.current = window.setTimeout(() => {
      taskControlRenewTimerRef.current = null;
      setTaskControlRenewReady(preview.requestId);
    }, TASK_CONTROL_RENEW_AFTER_MS);
    return () => {
      disposed = true;
      taskControlRenewCallback.current = () => {};
      if (taskControlRenewTimerRef.current !== null) {
        window.clearTimeout(taskControlRenewTimerRef.current);
        taskControlRenewTimerRef.current = null;
      }
    };
  }, [visible, taskControlPreview]);

  const handleLauncherTask = useCallback(
    (id: string, openOnly = false) => {
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
      if (!openOnly) setLauncherTask({ id: task.id, kind: task.kind });
    },
    [jobs, services],
  );

  const consumedProductTask = useRef<string | null>(null);
  useEffect(() => {
    if (
      !visible ||
      !openTask ||
      consumedProductTask.current === openTask.id ||
      loading ||
      busy ||
      importOpen ||
      screen === "editor" ||
      screen === "service-editor"
    )
      return;
    consumedProductTask.current = openTask.id;
    if (jobs.some((job) => job.id === openTask.jobId) || services.some((service) => service.id === openTask.jobId)) {
      handleLauncherTask(openTask.jobId, true);
    } else setError("선택한 작업 또는 서비스가 더 이상 없습니다.");
    onTaskConsumed?.(openTask.id);
  }, [visible, openTask, loading, busy, importOpen, screen, jobs, services, handleLauncherTask, onTaskConsumed]);

  const confirmLauncherTask = async () => {
    if (!launcherTask || busy) return;
    const task =
      launcherTask.kind === "job"
        ? jobs.find((candidate) => candidate.id === launcherTask.id && candidate.kind === "job")
        : services.find((candidate) => candidate.id === launcherTask.id && candidate.kind === "service");
    if (!task) {
      setLauncherTask(null);
      setError("Launcher가 요청한 작업을 찾지 못했습니다.");
      return;
    }
    const workspaceTask = task.kind === "job" ? workspaceTaskByJobId.get(task.id) : undefined;
    const workspaceOperation = task.kind === "job" ? workspaceOperationByRootJobId.get(task.id) : undefined;
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
    return runTaskAction(async () => {
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
      }
    });
  };

  openRequestRef.current = (request) => {
    if (request.target.kind === "task") {
      void handleLauncherTask(request.target.id, request.from === "workspace");
    } else if (request.target.kind === "handoff" && request.target.handoffKind === TASK_CONTROL_HANDOFF_KIND) {
      void handleTaskControlHandoff(request.target.id);
    }
  };

  useLayoutEffect(() => {
    if (!visible) return;
    if (launcherTask) launcherCancelRef.current?.focus();
    else document.querySelector<HTMLElement>(".job-card.selected")?.focus();
  }, [visible, launcherTask]);

  useLayoutEffect(() => {
    if (!visible) return;
    if (shellTrustTask) {
      shellTrustCancelRef.current?.focus();
    } else {
      shellTrustRestoreRef.current?.focus();
      shellTrustRestoreRef.current = null;
    }
  }, [visible, shellTrustTask]);

  useLayoutEffect(() => {
    if (!visible) return;
    if (taskControlPreview) {
      taskControlCancelRef.current?.focus();
    } else {
      taskControlRestoreRef.current?.focus();
      taskControlRestoreRef.current = null;
    }
  }, [visible, taskControlPreview]);

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
    if (!mountedRef.current) return;
    const view = viewGenerationRef.current;
    try {
      const status = await loadRuntimeStatus();
      if (!mountedRef.current || view !== viewGenerationRef.current) return;
      setStatus(status);
      setStatusError(null);
    } catch (cause) {
      setStatusError(friendlyErrorMessage(cause));
    }
  }, []);

  useEffect(() => {
    if (!visible) return;
    setWorkspaceSnapshotFresh(false);
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
        loadedGenerationRef.current = viewGenerationRef.current;
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
  }, [visible, refreshActiveRuns, refreshTaskControlReceipts, refreshWorkspaceOperations]);

  // AppLink events are only a wake-up signal. The authoritative request is
  // pulled from the native one-shot slot, then the current job/service list is
  // checked before any run, service action, or task-control handoff is used.
  useEffect(() => {
    if (
      !visible ||
      loading ||
      !workspaceSnapshotFresh ||
      screen === "editor" ||
      screen === "service-editor" ||
      importOpen ||
      busy
    )
      return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const consumePendingOpen = () => {
      if (disposed || loadedGenerationRef.current !== viewGenerationRef.current) return;
      void takePendingOpen()
        .then((request) => {
          if (!disposed && request) openRequestRef.current(request);
        })
        .catch((cause) => {
          if (!disposed) setError(friendlyErrorMessage(cause));
        });
    };
    let coldStartConsumed = false;
    const consumeColdStart = () => {
      if (disposed || coldStartConsumed) return;
      coldStartConsumed = true;
      consumePendingOpen();
    };
    void onOpenRequest(() => consumePendingOpen())
      .then((stop) => {
        if (disposed) stop();
        else {
          unlisten = stop;
          consumeColdStart();
        }
      })
      .catch(() => consumeColdStart());
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [visible, loading, workspaceSnapshotFresh, screen, importOpen, busy]);

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

  usePolling(
    async () => {
      await Promise.all([refreshStatus(), refreshActiveRuns()]);
    },
    { intervalMs: 1_000, active: visible, immediate: false },
  );

  useEffect(() => {
    onDirtyChange?.(screen === "editor" || screen === "service-editor" || importOpen || busy);
  }, [onDirtyChange, screen, importOpen, busy]);
  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange]);
  useEffect(() => {
    if (!visible) {
      jobContextMenu.close();
      serviceContextMenu.close();
      setContextJob(null);
      setContextService(null);
    }
  }, [visible, jobContextMenu.close, serviceContextMenu.close]);

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
    return runTaskAction(async () => {
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
      }
    });
  };

  const handleStopRun = async (job: Job) => {
    const workspaceOperation = workspaceOperationByRootJobId.get(job.id);
    const operationActive = workspaceOperation && !isWorkspaceOperationTerminal(workspaceOperation.status);
    if (
      !window.confirm(
        operationActive
          ? `'${job.name}' workspace task orchestration을 중지할까요?`
          : `'${job.name}' 작업의 활성 실행을 중지할까요?`,
      )
    )
      return;
    return runTaskAction(async () => {
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
      }
    });
  };

  const handleServiceStart = async (service: Job) => {
    return runTaskAction(async () => {
      try {
        await startService(service.id);
        await refreshServices();
        setError(null);
      } catch (cause) {
        setError(friendlyErrorMessage(cause));
      }
    });
  };

  const handleServiceStop = async (service: Job) => {
    if (!window.confirm(`'${service.name}' 서비스를 정지할까요?`)) return;
    return runTaskAction(async () => {
      try {
        await stopService(service.id);
        await refreshServices();
        setError(null);
      } catch (cause) {
        setError(friendlyErrorMessage(cause));
      }
    });
  };

  const handleServiceRestart = async (service: Job) => {
    return runTaskAction(async () => {
      try {
        await restartService(service.id);
        await refreshServices();
        setError(null);
      } catch (cause) {
        setError(friendlyErrorMessage(cause));
      }
    });
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
    () => (editingJobId ? (jobs.find((job) => job.id === editingJobId) ?? null) : null),
    [editingJobId, jobs],
  );

  const editingService = useMemo(
    () => (editingServiceId ? (services.find((service) => service.id === editingServiceId) ?? null) : null),
    [editingServiceId, services],
  );

  const editingWorkspaceTask = useMemo(
    () => (editingJobId ? (workspaceTaskByJobId.get(editingJobId) ?? null) : null),
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
    return runTaskAction(async () => {
      if (editingJobId) {
        await updateJob(editingJobId, input);
      } else {
        await createJob(input);
      }
      await refreshJobs();
      closeEditor();
    });
  };

  const handleDelete = async (job: Job) => {
    if (!window.confirm(`'${job.name}' 작업을 삭제할까요? 실행 기록도 함께 삭제됩니다.`)) return;
    return runTaskAction(async () => {
      try {
        await deleteJob(job.id);
        await refreshJobs();
        if (editingJobId === job.id) closeEditor();
      } catch (cause) {
        setError(friendlyErrorMessage(cause));
      }
    });
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
    return runTaskAction(async () => {
      try {
        await setJobEnabled(job.id, !job.enabled);
        await refreshJobs();
        setError(null);
      } catch (cause) {
        setError(friendlyErrorMessage(cause));
      }
    });
  };

  const toggleStartup = async () => {
    if (!startupStatus?.supported) return;
    return runTaskAction(async () => {
      try {
        setStartupStatus(await setStartupShortcutEnabled(!startupStatus.enabled));
        setError(null);
      } catch (cause) {
        setError(friendlyErrorMessage(cause));
      }
    });
  };

  const handleServiceSave = async (input: ServiceInput) => {
    return runTaskAction(async () => {
      if (editingServiceId) {
        await updateService(editingServiceId, input);
      } else {
        await createService(input);
      }
      await refreshServices();
      closeServiceEditor();
    });
  };

  const handleServiceDelete = async (service: Job) => {
    if (!window.confirm(`'${service.name}' 서비스를 삭제할까요? 저장된 정의와 실행 기록도 함께 삭제됩니다.`)) return;
    return runTaskAction(async () => {
      try {
        await deleteService(service.id);
        await refreshServices();
        if (editingServiceId === service.id) closeServiceEditor();
      } catch (cause) {
        setError(friendlyErrorMessage(cause));
      }
    });
  };

  const handleTrustWorkspaceTask = async (task: WorkspaceTaskState) => {
    if (busy || task.trusted) return;
    const approved = window.confirm(
      `현재 source revision ${shortRevision(task.revision)}을 신뢰할까요?\n` +
        "이 승인은 이 revision을 실행 대상으로 사용할 수 있도록 권한을 부여하지만, task를 실행하거나 프로세스를 시작하지 않습니다.",
    );
    if (!approved) return;
    return runTaskAction(async () => {
      setWorkspaceNotice(null);
      try {
        await trustWorkspaceTaskSource(task.sourceId, task.revision);
        await refreshJobs();
        setWorkspaceNotice(
          `source revision ${shortRevision(task.revision)}을 승인했습니다. 작업은 자동 실행되지 않았습니다.`,
        );
        setError(null);
      } catch (cause) {
        setError(friendlyErrorMessage(cause));
      }
    });
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
    return runTaskAction(async () => {
      setWorkspaceNotice(null);
      try {
        await trustWorkspaceTaskShellSource(task.sourceId, task.revision);
        await refreshJobs();
        setShellTrustTask(null);
        setWorkspaceNotice(
          `source revision ${shortRevision(task.revision)}의 셸 실행을 승인했습니다. 작업은 자동 실행되지 않았습니다.`,
        );
        setError(null);
      } catch (cause) {
        setError(friendlyErrorMessage(cause));
      }
    });
  };

  const jobContextItems = useMemo<readonly ContextMenuEntry[]>(() => {
    if (!contextJob) return [];
    const workspaceTask = workspaceTaskByJobId.get(contextJob.id);
    const workspaceOperation = workspaceOperationByRootJobId.get(contextJob.id);
    const operationActive =
      workspaceOperation !== undefined && !isWorkspaceOperationTerminal(workspaceOperation.status);
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
        disabled: busy || (!contextJob.enabled && (!workspaceRunnable || Boolean(workspaceTask?.dependsOn.length))),
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

  const contextServiceState = contextService ? (serviceInstances[contextService.id]?.state ?? null) : null;
  const serviceCanStart = contextServiceState === "stopped";
  const serviceCanStop =
    contextServiceState !== null && ["starting", "running", "retry_waiting"].includes(contextServiceState);
  const serviceCanRestart =
    contextServiceState !== null && ["starting", "running", "retry_waiting"].includes(contextServiceState);
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

  const launcherWorkspaceTask = launcherTask?.kind === "job" ? workspaceTaskByJobId.get(launcherTask.id) : undefined;
  const launcherWorkspaceOperation =
    launcherTask?.kind === "job" ? workspaceOperationByRootJobId.get(launcherTask.id) : undefined;
  const launcherOperationActive =
    launcherWorkspaceOperation !== undefined && !isWorkspaceOperationTerminal(launcherWorkspaceOperation.status);
  const visibleTaskControlReceipts =
    taskControlReceipt && !taskControlReceipts.some((receipt) => receipt.requestId === taskControlReceipt.requestId)
      ? [taskControlReceipt, ...taskControlReceipts]
      : taskControlReceipts;

  return (
    <main className="app-shell">
      <TaskSidebar
        screen={screen}
        setScreen={setScreen}
        jobs={jobs}
        services={services}
        busy={busy}
        startupStatus={startupStatus}
        toggleStartup={toggleStartup}
      />

      <section className="content">
        <header>
          <div>
            <span className="eyebrow">로컬 스케줄러</span>
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

        {(error ?? statusError ?? activeSnapshotError) ? (
          <div className="error-banner" role="alert">
            오류: {error ?? statusError ?? activeSnapshotError}
          </div>
        ) : null}
        {workspaceNotice ? (
          <div className="success-banner" role="status">
            {workspaceNotice}
          </div>
        ) : null}
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
                    <span>
                      {taskControlActionLabel(receipt.action)} · {taskControlReceiptStatusLabel(receipt.status)}
                    </span>
                    {receipt.failureCode ? (
                      <span className="workspace-task-unavailable">{friendlyErrorMessage(receipt.failureCode)}</span>
                    ) : null}
                  </li>
                );
              })}
            </ul>
          </section>
        ) : null}

        <RuntimeRecovery
          active={visible}
          busy={busy}
          onReviewed={() => {
            void refreshActiveRuns();
          }}
        />
        {screen === "editor" ? (
          <JobEditor
            active={visible}
            job={editingJob}
            workspaceTask={editingWorkspaceTask}
            onSave={handleSave}
            onCancel={closeEditor}
          />
        ) : screen === "service-editor" ? (
          <ServiceEditor service={editingService} onSave={handleServiceSave} onCancel={closeServiceEditor} />
        ) : screen === "history" ? (
          <RunHistory active={visible} jobs={historyDefinitions} requestedJobId={historyJobId} />
        ) : screen === "services" ? (
          <ServiceSection
            openServiceCreate={openServiceCreate}
            onExportDefs={onExportDefs}
            importTriggerRef={importTriggerRef}
            setImportOpen={setImportOpen}
            loading={loading}
            services={services}
            serviceInstances={serviceInstances}
            obsMap={obsMap}
            selectedServiceId={selectedServiceId}
            setSelectedServiceId={setSelectedServiceId}
            serviceContextTrigger={serviceContextTrigger}
            busy={busy}
            handleServiceRestart={handleServiceRestart}
            handleServiceStop={handleServiceStop}
            handleServiceStart={handleServiceStart}
            onToggleObs={onToggleObs}
            obsOpen={obsOpen}
            openServiceEdit={openServiceEdit}
            handleServiceDelete={handleServiceDelete}
            fmtUptime={fmtUptime}
          />
        ) : (
          <JobSection
            openCreate={openCreate}
            importTriggerRef={importTriggerRef}
            setImportOpen={setImportOpen}
            loading={loading}
            jobs={jobs}
            status={status}
            workspaceTaskByJobId={workspaceTaskByJobId}
            workspaceOperationByRootJobId={workspaceOperationByRootJobId}
            workspaceSnapshotFresh={workspaceSnapshotFresh}
            selectedJobId={selectedJobId}
            setSelectedJobId={setSelectedJobId}
            jobContextTrigger={jobContextTrigger}
            activeRuns={activeRuns}
            workspaceDiagnostics={workspaceDiagnostics}
            retryWorkspaceTaskDiagnostics={retryWorkspaceTaskDiagnostics}
            handleOpenWorkspaceTaskDiagnostic={handleOpenWorkspaceTaskDiagnostic}
            busy={busy}
            handleRunNow={handleRunNow}
            activeSnapshotFresh={activeSnapshotFresh}
            handleStopRun={handleStopRun}
            handleTrustWorkspaceTask={handleTrustWorkspaceTask}
            openShellTrustConfirmation={openShellTrustConfirmation}
            openEdit={openEdit}
            handleDelete={handleDelete}
          />
        )}
      </section>
      {importOpen && (
        <ImportDialog
          initialSource={importSource}
          active={visible}
          onDone={(_created, result: WorkspaceTaskApplyResult | undefined) => {
            if (result) {
              setWorkspaceNotice(
                `workspace task 가져오기 완료: 생성 ${result.created} · 갱신 ${result.updated} · 사용 불가 전환 ${result.madeUnavailable} · 충돌 건너뜀 ${result.skippedConflicts}. source revision 승인 후에만 활성화할 수 있습니다.`,
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
              <strong>{shellTrustTask.label}</strong> task가 source의 셸 명령을 실행하도록 별도로 승인합니다. 셸 명령은
              source revision에 따라 바뀔 수 있으며, 승인 후 스케줄 실행·수동 실행에서 실제 셸을 호출할 수 있습니다.
              현재 revision <code>{shortRevision(shellTrustTask.revision)}</code>만 승인되며, source가 변경되면 승인이
              무효화됩니다.
            </p>
            <dl className="workspace-task-details shell-trust-details">
              <div>
                <dt>source</dt>
                <dd>
                  <code>{shellTrustTask.sourceRoot}</code>
                </dd>
              </div>
              <div>
                <dt>명령</dt>
                <dd>
                  <code>
                    {jobs.find((job) => job.id === shellTrustTask.jobId)?.command ?? "source revision에서 읽음"}
                  </code>
                </dd>
              </div>
            </dl>
            <div className="workspace-task-notice" role="note">
              이 확인은 일반 source 승인과 별개입니다. 셸 실행 위험을 이해했고 이 source의 셸 task를 실행하겠다면 아래
              버튼을 선택하세요.
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
          <TaskControlDialog
            onTaskControlDialogKeyDown={onTaskControlDialogKeyDown}
            taskControlPreview={taskControlPreview}
            taskControlLeaseUntil={taskControlLeaseUntil}
            taskControlCancelRef={taskControlCancelRef}
            busy={busy}
            handleRejectTaskControl={handleRejectTaskControl}
            handleAcceptTaskControl={handleAcceptTaskControl}
          />
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
