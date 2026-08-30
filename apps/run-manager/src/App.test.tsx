import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  acceptWorkspaceTaskControl,
  deleteJob,
  deleteService,
  getServiceInstance,
  getWorkspaceTaskOperation,
  listWorkspaceTaskControlReceipts,
  listWorkspaceTaskDiagnostics,
  listActiveRuns,
  listJobs,
  listRuns,
  listServices,
  listWorkspaceTasks,
  listWorkspaceTaskOperations,
  onOpenRequest,
  restartService,
  openWorkspaceTaskDiagnostic,
  previewWorkspaceTaskControl,
  rejectWorkspaceTaskControl,
  renewWorkspaceTaskControl,
  runJobNow,
  runWorkspaceTaskOperation,
  setJobEnabled,
  startService,
  stopActiveRun,
  stopService,
  stopWorkspaceTaskOperation,
  tailLog,
  takePendingOpen,
  trustWorkspaceTaskSource,
  trustWorkspaceTaskShellSource,
} from "./api";
import type {
  Job,
  Run,
  ServiceInstance,
  WorkspaceTaskControlPreview,
  WorkspaceTaskControlReceipt,
  WorkspaceTaskDiagnostics,
  WorkspaceTaskOperation,
  WorkspaceTaskState,
} from "./types";

vi.mock("./api", () => ({
  acceptWorkspaceTaskControl: vi.fn(),
  applyImport: vi.fn(async () => 0),
  createJob: vi.fn(),
  createService: vi.fn(),
  deleteJob: vi.fn(),
  deleteService: vi.fn(),
  exportDefinitions: vi.fn(async () => null),
  friendlyErrorMessage: vi.fn((cause: unknown) => cause instanceof Error ? cause.message : String(cause)),
  getServiceInstance: vi.fn(),
  getServiceObservability: vi.fn(async () => null),
  getWorkspaceTaskOperation: vi.fn(async () => null),
  listWorkspaceTaskControlReceipts: vi.fn(async () => []),
  listWorkspaceTaskDiagnostics: vi.fn(async () => ({ runId: "", items: [], truncated: false })),
  hideMainWindow: vi.fn(async () => undefined),
  importDefinitions: vi.fn(),
  listActiveRuns: vi.fn(),
  listJobs: vi.fn(),
  listRuns: vi.fn(),
  listServices: vi.fn(),
  listWorkspaceTasks: vi.fn(async () => []),
  listWorkspaceTaskOperations: vi.fn(async () => []),
  loadRuntimeStatus: vi.fn(async () => ({
    backgroundLaunch: false,
    schedulerRunning: true,
    shutdownRequested: false,
    databasePath: "app-owned.db",
  })),
  loadStartupShortcutStatus: vi.fn(async () => ({
    supported: false,
    enabled: false,
    shortcutPath: "startup-link",
  })),
  onOpenRequest: vi.fn(async () => () => undefined),
  openWorkspaceTaskDiagnostic: vi.fn(),
  openRunLogInLogLens: vi.fn(async () => undefined),
  previewCron: vi.fn(async () => []),
  previewWorkspaceTaskControl: vi.fn(),
  quitApp: vi.fn(async () => undefined),
  restartService: vi.fn(),
  runJobNow: vi.fn(),
  runWorkspaceTaskOperation: vi.fn(),
  rejectWorkspaceTaskControl: vi.fn(),
  renewWorkspaceTaskControl: vi.fn(),
  searchRunLogs: vi.fn(),
  setJobEnabled: vi.fn(),
  setStartupShortcutEnabled: vi.fn(),
  startService: vi.fn(),
  stopActiveRun: vi.fn(),
  stopService: vi.fn(),
  stopWorkspaceTaskOperation: vi.fn(),
  tailLog: vi.fn(),
  takePendingOpen: vi.fn(async () => null),
  updateJob: vi.fn(),
  updateService: vi.fn(),
  trustWorkspaceTaskSource: vi.fn(async () => true),
  trustWorkspaceTaskShellSource: vi.fn(async () => true),
}));

const job: Job = {
  id: "job-1",
  kind: "job",
  name: "백업",
  command: "backup",
  cwd: null,
  targetKind: "windows",
  targetDistro: null,
  envConfigured: true,
  cronExpr: "0 * * * *",
  enabled: true,
  overlapPolicy: "skip",
  catchUp: false,
  lastEvaluatedAt: null,
  nextQueueSequence: 0,
  restartPolicy: null,
  autoStart: null,
  healthTcpAddress: null,
  healthTcpPort: null,
  healthStartGraceMs: null,
  createdAt: 1_000,
  updatedAt: 1_000,
};

const secondJob: Job = { ...job, id: "job-2", name: "보고서", command: "report" };
const service: Job = {
  ...job,
  id: "service-1",
  kind: "service",
  name: "로컬 서버",
  command: "serve",
  cronExpr: null,
  enabled: true,
  restartPolicy: "on-failure",
  autoStart: false,
};
const activeRun: Run = {
  id: "run-1",
  jobId: job.id,
  scheduledAt: null,
  occurrenceWallKey: null,
  queueSequence: 0,
  startedAt: 2_000,
  endedAt: null,
  exitCode: null,
  status: "running",
  logsAvailable: true,
  failureCode: null,
  createdAt: 1_500,
};
const stoppedInstance: ServiceInstance = {
  jobId: service.id,
  generation: 1,
  state: "stopped",
  consecutiveFailures: 0,
  nextRetryAt: null,
};

const managedWorkspaceTask: WorkspaceTaskState = {
  jobId: job.id,
  sourceId: "source-1",
  label: job.name,
  taskKind: "process",
  sourceRoot: "C:\\work\\demo",
  revision: "revision-1234567890",
  targetKind: "windows",
  targetDistro: null,
  environmentKeys: ["BUILD_TOKEN"],
  appliedOverride: "windows",
  dependsOn: [],
  dependsOrder: "parallel",
  hasProblemMatcher: false,
  trusted: false,
  shellTrusted: false,
  available: true,
};

const managedShellWorkspaceTask: WorkspaceTaskState = {
  ...managedWorkspaceTask,
  label: "Publish shell",
  taskKind: "shell",
  trusted: true,
  shellTrusted: false,
  dependsOn: ["Build"],
  dependsOrder: "sequence",
};

const managedRunnableWorkspaceTask: WorkspaceTaskState = {
  ...managedWorkspaceTask,
  trusted: true,
};

const runningWorkspaceOperation: WorkspaceTaskOperation = {
  id: "workspace-operation-1",
  rootJobId: job.id,
  sourceId: managedWorkspaceTask.sourceId,
  revision: managedWorkspaceTask.revision,
  status: "running",
  failFast: true,
  failureCode: null,
  createdAt: 3_000,
  startedAt: 3_001,
  endedAt: null,
  runs: [
    {
      jobId: secondJob.id,
      runId: "child-run-1",
      layerIndex: 0,
      sequence: 0,
      status: "succeeded",
      failureCode: null,
    },
    {
      jobId: job.id,
      runId: "root-run-1",
      layerIndex: 1,
      sequence: 1,
      status: "running",
      failureCode: null,
    },
  ],
};

const cancelledWorkspaceOperation: WorkspaceTaskOperation = {
  ...runningWorkspaceOperation,
  status: "cancelled",
  endedAt: 3_500,
  runs: runningWorkspaceOperation.runs.map((run) => (
    run.status === "running" ? { ...run, status: "cancelled" as const } : run
  )),
};

const matcherWorkspaceTask: WorkspaceTaskState = {
  ...managedRunnableWorkspaceTask,
  hasProblemMatcher: true,
};

const matcherChildWorkspaceTask: WorkspaceTaskState = {
  ...matcherWorkspaceTask,
  jobId: secondJob.id,
  label: secondJob.name,
};

const completedWorkspaceOperation: WorkspaceTaskOperation = {
  ...runningWorkspaceOperation,
  status: "succeeded",
  endedAt: 3_500,
  runs: runningWorkspaceOperation.runs.map((run) => ({ ...run, status: "succeeded" as const })),
};

const workspaceDiagnostics: WorkspaceTaskDiagnostics = {
  runId: "root-run-1",
  truncated: true,
  items: [{
    index: 0,
    file: "src/main.ts",
    line: 4,
    column: 2,
    message: "type error",
    severity: "error",
    stream: "stderr",
  }],
};

const taskControlPreview: WorkspaceTaskControlPreview = {
  requestId: "request-1",
  taskId: job.id,
  action: "start",
  expectedRevision: "a".repeat(64),
  label: job.name,
  taskKind: "process",
};

const taskControlReceipt: WorkspaceTaskControlReceipt = {
  schemaVersion: 1,
  requestId: taskControlPreview.requestId,
  taskId: taskControlPreview.taskId,
  action: taskControlPreview.action,
  status: "started",
  operationId: runningWorkspaceOperation.id,
  failureCode: null,
  createdAt: 4_000,
  updatedAt: 4_001,
};

const listJobsMock = vi.mocked(listJobs);
const listServicesMock = vi.mocked(listServices);
const listWorkspaceTasksMock = vi.mocked(listWorkspaceTasks);
const listWorkspaceTaskOperationsMock = vi.mocked(listWorkspaceTaskOperations);
const listWorkspaceTaskControlReceiptsMock = vi.mocked(listWorkspaceTaskControlReceipts);
const listWorkspaceTaskDiagnosticsMock = vi.mocked(listWorkspaceTaskDiagnostics);
const getServiceInstanceMock = vi.mocked(getServiceInstance);
const getWorkspaceTaskOperationMock = vi.mocked(getWorkspaceTaskOperation);
const listActiveRunsMock = vi.mocked(listActiveRuns);
const listRunsMock = vi.mocked(listRuns);
const runJobNowMock = vi.mocked(runJobNow);
const runWorkspaceTaskOperationMock = vi.mocked(runWorkspaceTaskOperation);
const openWorkspaceTaskDiagnosticMock = vi.mocked(openWorkspaceTaskDiagnostic);
const previewWorkspaceTaskControlMock = vi.mocked(previewWorkspaceTaskControl);
const acceptWorkspaceTaskControlMock = vi.mocked(acceptWorkspaceTaskControl);
const rejectWorkspaceTaskControlMock = vi.mocked(rejectWorkspaceTaskControl);
const renewWorkspaceTaskControlMock = vi.mocked(renewWorkspaceTaskControl);
const setJobEnabledMock = vi.mocked(setJobEnabled);
const stopActiveRunMock = vi.mocked(stopActiveRun);
const stopWorkspaceTaskOperationMock = vi.mocked(stopWorkspaceTaskOperation);
const startServiceMock = vi.mocked(startService);
const stopServiceMock = vi.mocked(stopService);
const restartServiceMock = vi.mocked(restartService);
const deleteJobMock = vi.mocked(deleteJob);
const deleteServiceMock = vi.mocked(deleteService);
const tailLogMock = vi.mocked(tailLog);
const onOpenRequestMock = vi.mocked(onOpenRequest);
const takePendingOpenMock = vi.mocked(takePendingOpen);
const trustWorkspaceTaskSourceMock = vi.mocked(trustWorkspaceTaskSource);
const trustWorkspaceTaskShellSourceMock = vi.mocked(trustWorkspaceTaskShellSource);
const confirmMock = vi.fn<(message?: string) => boolean>();

function card(name: string): HTMLElement {
  const element = screen.getByRole("heading", { name }).closest("article");
  if (!(element instanceof HTMLElement)) throw new Error(`${name} card was not rendered`);
  return element;
}

beforeEach(() => {
  listJobsMock.mockReset().mockResolvedValue([job]);
  listServicesMock.mockReset().mockResolvedValue([service]);
  listWorkspaceTasksMock.mockReset().mockResolvedValue([]);
  listWorkspaceTaskOperationsMock.mockReset().mockResolvedValue([]);
  listWorkspaceTaskControlReceiptsMock.mockReset().mockResolvedValue([]);
  listWorkspaceTaskDiagnosticsMock.mockReset().mockResolvedValue({ runId: "", items: [], truncated: false });
  getServiceInstanceMock.mockReset().mockResolvedValue(stoppedInstance);
  getWorkspaceTaskOperationMock.mockReset().mockResolvedValue(null);
  listActiveRunsMock.mockReset().mockResolvedValue([]);
  listRunsMock.mockReset().mockResolvedValue([]);
  runJobNowMock.mockReset().mockResolvedValue(activeRun);
  runWorkspaceTaskOperationMock.mockReset();
  openWorkspaceTaskDiagnosticMock.mockReset().mockResolvedValue(true);
  previewWorkspaceTaskControlMock.mockReset();
  acceptWorkspaceTaskControlMock.mockReset();
  rejectWorkspaceTaskControlMock.mockReset();
  renewWorkspaceTaskControlMock.mockReset();
  setJobEnabledMock.mockReset().mockResolvedValue({ ...job, enabled: false });
  stopActiveRunMock.mockReset().mockResolvedValue({ ...activeRun, status: "cancelled" });
  stopWorkspaceTaskOperationMock.mockReset();
  startServiceMock.mockReset().mockResolvedValue({ ...stoppedInstance, state: "running" });
  stopServiceMock.mockReset().mockResolvedValue(stoppedInstance);
  restartServiceMock.mockReset().mockResolvedValue({ ...stoppedInstance, state: "running" });
  deleteJobMock.mockReset().mockResolvedValue(true);
  deleteServiceMock.mockReset().mockResolvedValue(true);
  tailLogMock.mockReset().mockResolvedValue({
    data: [],
    retainedStartOffset: "0",
    nextCursor: "0",
    truncated: false,
  });
  onOpenRequestMock.mockReset().mockResolvedValue(() => undefined);
  takePendingOpenMock.mockReset().mockResolvedValue(null);
  trustWorkspaceTaskSourceMock.mockReset().mockResolvedValue(true);
  trustWorkspaceTaskShellSourceMock.mockReset().mockResolvedValue(true);
  confirmMock.mockReset().mockReturnValue(false);
  Object.defineProperty(window, "confirm", {
    configurable: true,
    value: confirmMock,
  });
});

afterEach(() => cleanup());

describe("Run Manager context menus", () => {
  it("gates managed workspace task run and requires explicit source trust", async () => {
    listWorkspaceTasksMock.mockResolvedValue([managedWorkspaceTask]);
    render(<App />);

    const target = await screen.findByRole("heading", { name: "백업" });
    expect(screen.getByText("소스 승인 필요")).toBeTruthy();
    expect(screen.getByText("환경 키: BUILD_TOKEN")).toBeTruthy();
    const cardElement = target.closest("article");
    if (!(cardElement instanceof HTMLElement)) throw new Error("managed task card was not rendered");
    expect(cardElement.querySelector("button.button-primary")).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "소스 승인" }));
    expect(trustWorkspaceTaskSourceMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(screen.getByRole("button", { name: "소스 승인" }));
    await waitFor(() => expect(trustWorkspaceTaskSourceMock).toHaveBeenCalledWith(
      managedWorkspaceTask.sourceId,
      managedWorkspaceTask.revision,
    ));
    expect(confirmMock.mock.calls[1][0]).toContain("task를 실행하거나 프로세스를 시작하지 않습니다");
  });

  it("shows the separate shell trust gate and keeps shell jobs disabled until it is approved", async () => {
    listWorkspaceTasksMock.mockResolvedValue([managedShellWorkspaceTask]);
    render(<App />);

    const target = await screen.findByRole("heading", { name: "백업" });
    const cardElement = target.closest("article");
    if (!(cardElement instanceof HTMLElement)) throw new Error("managed shell task card was not rendered");
    expect(screen.getByText("소스 승인됨")).toBeTruthy();
    expect(screen.getByText("셸 실행 승인 필요")).toBeTruthy();
    expect(cardElement.querySelector("button.button-primary")).toBeDisabled();

    fireEvent.click(within(cardElement).getByRole("button", { name: "셸 실행 승인" }));
    const dialog = await screen.findByRole("dialog", { name: "셸 실행 승인" });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "취소" }));
    expect(dialog.textContent).toContain("일반 source 승인과 별개");
    fireEvent.click(within(dialog).getByRole("button", { name: "셸 실행 승인" }));

    await waitFor(() => expect(trustWorkspaceTaskShellSourceMock).toHaveBeenCalledWith(
      managedShellWorkspaceTask.sourceId,
      managedShellWorkspaceTask.revision,
    ));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "셸 실행 승인" })).toBeNull());
    expect(screen.getByText(/셸 실행을 승인했습니다/)).toBeTruthy();
  });

  it("routes managed workspace runs through an operation and exposes child progress and stop", async () => {
    listJobsMock.mockResolvedValue([job, secondJob]);
    listWorkspaceTasksMock.mockResolvedValue([managedRunnableWorkspaceTask]);
    runWorkspaceTaskOperationMock.mockResolvedValue(runningWorkspaceOperation);
    getWorkspaceTaskOperationMock.mockResolvedValue(runningWorkspaceOperation);
    stopWorkspaceTaskOperationMock.mockResolvedValue(cancelledWorkspaceOperation);
    render(<App />);

    const target = await screen.findByRole("heading", { name: "백업" });
    const targetCard = target.closest("article");
    if (!(targetCard instanceof HTMLElement)) throw new Error("managed task card was not rendered");
    fireEvent.click(within(targetCard).getByRole("button", { name: "지금 실행" }));

    await waitFor(() => expect(runWorkspaceTaskOperationMock).toHaveBeenCalledWith(job.id, true));
    expect(runJobNowMock).not.toHaveBeenCalled();
    expect(await within(targetCard).findByLabelText("workspace task operation 상태: 실행 중")).toBeTruthy();
    expect(within(targetCard).getByText(/child 진행: 보고서: 완료 · 백업: 실행 중/)).toBeTruthy();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(within(targetCard).getByRole("button", { name: "오케스트레이션 중지" }));
    await waitFor(() => expect(stopWorkspaceTaskOperationMock).toHaveBeenCalledWith(runningWorkspaceOperation.id));
    expect(stopActiveRunMock).not.toHaveBeenCalled();
    expect(await within(targetCard).findByText(/오케스트레이션 취소됨/)).toBeTruthy();
  });

  it("keeps an active operation visible ahead of same-timestamp terminal history", async () => {
    const sameTimestampHistory: WorkspaceTaskOperation = {
      ...cancelledWorkspaceOperation,
      id: "workspace-operation-history",
      createdAt: runningWorkspaceOperation.createdAt,
    };
    listJobsMock.mockResolvedValue([job, secondJob]);
    listWorkspaceTasksMock.mockResolvedValue([managedRunnableWorkspaceTask]);
    listWorkspaceTaskOperationsMock.mockResolvedValue([
      runningWorkspaceOperation,
      sameTimestampHistory,
    ]);
    getWorkspaceTaskOperationMock.mockResolvedValue(runningWorkspaceOperation);
    stopWorkspaceTaskOperationMock.mockResolvedValue(cancelledWorkspaceOperation);
    render(<App />);

    await screen.findByRole("heading", { name: "백업" });
    const targetCard = card("백업");
    expect(await within(targetCard).findByLabelText(
      "workspace task operation 상태: 실행 중",
    )).toBeTruthy();
    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(within(targetCard).getByRole("button", { name: "오케스트레이션 중지" }));
    await waitFor(() => expect(stopWorkspaceTaskOperationMock).toHaveBeenCalledWith(
      runningWorkspaceOperation.id,
    ));
  });

  it("loads diagnostics only for terminal matcher children and opens the selected item in Code Pad", async () => {
    listJobsMock.mockResolvedValue([job, secondJob]);
    listWorkspaceTasksMock.mockResolvedValue([matcherWorkspaceTask, matcherChildWorkspaceTask]);
    listWorkspaceTaskOperationsMock.mockResolvedValue([completedWorkspaceOperation]);
    listWorkspaceTaskDiagnosticsMock.mockResolvedValue(workspaceDiagnostics);
    render(<App />);

    await screen.findByRole("heading", { name: "백업" });
    const targetCard = card("백업");
    await waitFor(() => expect(listWorkspaceTaskDiagnosticsMock).toHaveBeenCalledWith("root-run-1"));
    await waitFor(() => expect(listWorkspaceTaskDiagnosticsMock).toHaveBeenCalledWith("child-run-1"));
    expect(listWorkspaceTaskDiagnosticsMock).toHaveBeenCalledTimes(2);
    const diagnostic = within(targetCard).getAllByRole("button", { name: "src/main.ts:4:2 type error" })[0];
    expect(diagnostic).toBeTruthy();
    fireEvent.click(diagnostic);
    await waitFor(() => expect(openWorkspaceTaskDiagnosticMock).toHaveBeenCalledWith("child-run-1", 0));
    expect(within(targetCard).getAllByText("일부 diagnostics만 표시됨")).toHaveLength(2);
  });

  it("previews task-control handoffs in a trapped dialog and records the accepted receipt", async () => {
    takePendingOpenMock.mockResolvedValueOnce({
      target: { kind: "handoff", handoffKind: "task-control/v1", id: "handoff-1" },
      from: "workbench",
    });
    previewWorkspaceTaskControlMock.mockResolvedValue(taskControlPreview);
    acceptWorkspaceTaskControlMock.mockResolvedValue(taskControlReceipt);
    render(<App />);

    const dialog = await screen.findByRole("dialog", { name: "Workbench 요청 확인" });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "거절" }));
    expect(dialog.textContent).toContain("source revision");
    expect(dialog.textContent).not.toContain("backup");
    fireEvent.click(screen.getByRole("button", { name: "승인하고 시작" }));

    await waitFor(() => expect(acceptWorkspaceTaskControlMock).toHaveBeenCalledWith(taskControlPreview.requestId));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Workbench 요청 확인" })).toBeNull());
    expect(screen.getByText(/시작됨/)).toBeTruthy();
  });

  it("requires explicit confirmation before a Launcher task mutates runtime state", async () => {
    takePendingOpenMock.mockResolvedValueOnce({
      target: { kind: "task", id: job.id },
      from: "devbox-launcher",
    });
    render(<App />);

    const dialog = await screen.findByRole("dialog", { name: "Launcher 요청 확인" });
    expect(runJobNowMock).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "취소" }));

    fireEvent.click(screen.getByRole("button", { name: "실행" }));
    await waitFor(() => expect(runJobNowMock).toHaveBeenCalledWith(job.id));
    await waitFor(() => expect(dialog.isConnected).toBe(false));
  });

  it("selects the right-clicked job and exposes every job-owned action", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "백업" });
    const target = card("백업");

    fireEvent.contextMenu(target, { clientX: 16, clientY: 24 });

    expect(target.getAttribute("aria-current")).toBe("true");
    expect(screen.getByRole("menu", { name: "작업 메뉴" })).toBeTruthy();
    for (const label of ["지금 실행", "비활성화", "편집", "로그 열기", "삭제"]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeTruthy();
    }
  });

  it("toggles the exact keyboard-selected job and restores row focus", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "백업" });
    const target = card("백업");
    target.focus();

    fireEvent.keyDown(target, { key: "F10", code: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "비활성화" }));

    await waitFor(() => expect(setJobEnabledMock).toHaveBeenCalledWith(job.id, false));
    await waitFor(() => expect(document.activeElement).toBe(target));
  });

  it("fails closed after the workspace task snapshot becomes unavailable", async () => {
    listWorkspaceTasksMock
      .mockReset()
      .mockResolvedValueOnce([])
      .mockRejectedValueOnce(new Error("workspace-state-failed"));
    render(<App />);
    await screen.findByRole("heading", { name: "백업" });
    const target = card("백업");

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "비활성화" }));

    await screen.findByText(/workspace-state-failed/);
    expect(target.querySelector("button.button-primary")).toBeDisabled();
  });

  it("opens history for the right-clicked job instead of the previous row", async () => {
    listJobsMock.mockResolvedValue([job, secondJob]);
    render(<App />);
    await screen.findByRole("heading", { name: "보고서" });

    fireEvent.contextMenu(card("보고서"));
    fireEvent.click(screen.getByRole("menuitem", { name: "로그 열기" }));

    await waitFor(() => expect(listRunsMock).toHaveBeenCalledWith(
      secondJob.id,
      expect.objectContaining({ limit: 50 }),
    ));
  });

  it("requires confirmation before job deletion and active-run stop", async () => {
    listActiveRunsMock.mockResolvedValue([activeRun]);
    render(<App />);
    await screen.findByRole("heading", { name: "백업" });
    const stop = screen.getByRole("button", { name: "중지" });
    await waitFor(() => expect(stop.hasAttribute("disabled")).toBe(false));

    fireEvent.click(stop);
    expect(confirmMock).toHaveBeenCalledWith("'백업' 작업의 활성 실행을 중지할까요?");
    expect(stopActiveRunMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    listActiveRunsMock.mockResolvedValue([]);
    fireEvent.click(stop);
    await waitFor(() => expect(stopActiveRunMock).toHaveBeenCalledWith(job.id));

    await waitFor(() => expect(stop.hasAttribute("disabled")).toBe(true));
    fireEvent.contextMenu(card("백업"));
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
    expect(deleteJobMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.contextMenu(card("백업"));
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
    await waitFor(() => expect(deleteJobMock).toHaveBeenCalledWith(job.id));
  });

  it("uses the initial service snapshot for disabled states and confirms stop/delete", async () => {
    getServiceInstanceMock.mockResolvedValue({ ...stoppedInstance, state: "running" });
    render(<App />);
    await screen.findByRole("heading", { name: "백업" });
    fireEvent.click(screen.getByRole("button", { name: /^서비스/ }));
    await screen.findByRole("heading", { name: "로컬 서버" });
    const target = card("로컬 서버");

    fireEvent.contextMenu(target);
    for (const label of ["시작", "정지", "재시작", "편집", "삭제"]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeTruthy();
    }
    expect(screen.getByRole("menuitem", { name: "시작" }).getAttribute("aria-disabled")).toBe("true");
    expect(screen.getByRole("menuitem", { name: "정지" }).getAttribute("aria-disabled")).toBeNull();
    fireEvent.click(screen.getByRole("menuitem", { name: "정지" }));
    expect(confirmMock).toHaveBeenCalledWith("'로컬 서버' 서비스를 정지할까요?");
    expect(stopServiceMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "정지" }));
    await waitFor(() => expect(stopServiceMock).toHaveBeenCalledWith(service.id));
    expect(startServiceMock).not.toHaveBeenCalled();
    expect(restartServiceMock).not.toHaveBeenCalled();
  });

  it("deletes a stopped service only after explicit confirmation", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "백업" });
    fireEvent.click(screen.getByRole("button", { name: /^서비스/ }));
    await screen.findByRole("heading", { name: "로컬 서버" });
    const target = card("로컬 서버");

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
    expect(confirmMock).toHaveBeenCalledWith(
      "'로컬 서버' 서비스를 삭제할까요? 저장된 정의와 실행 기록도 함께 삭제됩니다.",
    );
    expect(deleteServiceMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
    await waitFor(() => expect(deleteServiceMock).toHaveBeenCalledWith(service.id));
  });

  it("allows retry-waiting cancellation and restart while keeping deletion fail-closed", async () => {
    getServiceInstanceMock.mockResolvedValue({
      ...stoppedInstance,
      state: "retry_waiting",
      nextRetryAt: 60_000,
    });
    render(<App />);
    await screen.findByRole("heading", { name: "백업" });
    fireEvent.click(screen.getByRole("button", { name: /^서비스/ }));
    await screen.findByRole("heading", { name: "로컬 서버" });
    const target = card("로컬 서버");

    fireEvent.contextMenu(target);

    expect(screen.getByRole("menuitem", { name: "시작" }).getAttribute("aria-disabled")).toBe("true");
    expect(screen.getByRole("menuitem", { name: "정지" }).getAttribute("aria-disabled")).toBeNull();
    expect(screen.getByRole("menuitem", { name: "재시작" }).getAttribute("aria-disabled")).toBeNull();
    expect(screen.getByRole("menuitem", { name: "삭제" }).getAttribute("aria-disabled")).toBe("true");
  });

  it("disables every service lifecycle transition while a stop is pending", async () => {
    getServiceInstanceMock.mockResolvedValue({ ...stoppedInstance, state: "stopping" });
    render(<App />);
    await screen.findByRole("heading", { name: "백업" });
    fireEvent.click(screen.getByRole("button", { name: /^서비스/ }));
    await screen.findByRole("heading", { name: "로컬 서버" });

    fireEvent.contextMenu(card("로컬 서버"));

    for (const label of ["시작", "정지", "재시작", "삭제"]) {
      expect(screen.getByRole("menuitem", { name: label }).getAttribute("aria-disabled")).toBe("true");
    }
  });

  it("fails closed when a service instance snapshot is unavailable", async () => {
    getServiceInstanceMock.mockResolvedValue(null);
    render(<App />);
    await screen.findByRole("heading", { name: "백업" });
    fireEvent.click(screen.getByRole("button", { name: /^서비스/ }));
    await screen.findByText("상태 확인 불가");

    fireEvent.contextMenu(card("로컬 서버"));

    for (const label of ["시작", "정지", "재시작", "삭제"]) {
      expect(screen.getByRole("menuitem", { name: label }).getAttribute("aria-disabled")).toBe("true");
    }
  });
});
