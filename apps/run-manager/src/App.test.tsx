import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  deleteJob,
  deleteService,
  getServiceInstance,
  listActiveRuns,
  listJobs,
  listRuns,
  listServices,
  listWorkspaceTasks,
  onOpenRequest,
  restartService,
  runJobNow,
  setJobEnabled,
  startService,
  stopActiveRun,
  stopService,
  tailLog,
  takePendingOpen,
  trustWorkspaceTaskSource,
} from "./api";
import type { Job, Run, ServiceInstance, WorkspaceTaskState } from "./types";

vi.mock("./api", () => ({
  applyImport: vi.fn(async () => 0),
  createJob: vi.fn(),
  createService: vi.fn(),
  deleteJob: vi.fn(),
  deleteService: vi.fn(),
  exportDefinitions: vi.fn(async () => null),
  friendlyErrorMessage: vi.fn((cause: unknown) => cause instanceof Error ? cause.message : String(cause)),
  getServiceInstance: vi.fn(),
  getServiceObservability: vi.fn(async () => null),
  hideMainWindow: vi.fn(async () => undefined),
  importDefinitions: vi.fn(),
  listActiveRuns: vi.fn(),
  listJobs: vi.fn(),
  listRuns: vi.fn(),
  listServices: vi.fn(),
  listWorkspaceTasks: vi.fn(async () => []),
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
  openRunLogInLogLens: vi.fn(async () => undefined),
  previewCron: vi.fn(async () => []),
  quitApp: vi.fn(async () => undefined),
  restartService: vi.fn(),
  runJobNow: vi.fn(),
  searchRunLogs: vi.fn(),
  setJobEnabled: vi.fn(),
  setStartupShortcutEnabled: vi.fn(),
  startService: vi.fn(),
  stopActiveRun: vi.fn(),
  stopService: vi.fn(),
  tailLog: vi.fn(),
  takePendingOpen: vi.fn(async () => null),
  updateJob: vi.fn(),
  updateService: vi.fn(),
  trustWorkspaceTaskSource: vi.fn(async () => true),
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
  trusted: false,
  available: true,
};

const listJobsMock = vi.mocked(listJobs);
const listServicesMock = vi.mocked(listServices);
const listWorkspaceTasksMock = vi.mocked(listWorkspaceTasks);
const getServiceInstanceMock = vi.mocked(getServiceInstance);
const listActiveRunsMock = vi.mocked(listActiveRuns);
const listRunsMock = vi.mocked(listRuns);
const runJobNowMock = vi.mocked(runJobNow);
const setJobEnabledMock = vi.mocked(setJobEnabled);
const stopActiveRunMock = vi.mocked(stopActiveRun);
const startServiceMock = vi.mocked(startService);
const stopServiceMock = vi.mocked(stopService);
const restartServiceMock = vi.mocked(restartService);
const deleteJobMock = vi.mocked(deleteJob);
const deleteServiceMock = vi.mocked(deleteService);
const tailLogMock = vi.mocked(tailLog);
const onOpenRequestMock = vi.mocked(onOpenRequest);
const takePendingOpenMock = vi.mocked(takePendingOpen);
const trustWorkspaceTaskSourceMock = vi.mocked(trustWorkspaceTaskSource);
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
  getServiceInstanceMock.mockReset().mockResolvedValue(stoppedInstance);
  listActiveRunsMock.mockReset().mockResolvedValue([]);
  listRunsMock.mockReset().mockResolvedValue([]);
  runJobNowMock.mockReset().mockResolvedValue(activeRun);
  setJobEnabledMock.mockReset().mockResolvedValue({ ...job, enabled: false });
  stopActiveRunMock.mockReset().mockResolvedValue({ ...activeRun, status: "cancelled" });
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
