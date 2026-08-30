import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  cancelDependencyHealth,
  cancelProjectEnvironment,
  cancelProjectHealth,
  cancelStartWorkspace,
  cancelWorkspacePreflight,
  createProfile,
  createProfileFromTemplate,
  createProfileTemplate,
  currentWorkspaceRun,
  deleteProfile,
  deleteProfileTemplate,
  dependencyHealth,
  listProfiles,
  listProfileTemplates,
  openProfileIn,
  packageDependencySummary,
  previewProjectEnvironment,
  profileCopyPath,
  profileOpenTargets,
  projectHealth,
  retryWorkspace,
  startWorkspace,
  stopWorkspace,
  updateProfile,
  updateProfileTemplate,
  workspacePreflight,
  wslRuntimeSuggestions,
  type ProjectEnvironmentPreview,
  type ProjectHealth,
  type ProjectProfile,
  type ProfileTemplate,
  type ProfileTemplateSnapshot,
  type WorkspacePreflight,
  type WorkspaceRun,
  type RuntimeSuggestions,
  type PackageDependencySummary,
} from "./api";

vi.mock("./api", () => ({
  cancelDependencyHealth: vi.fn(),
  cancelProjectEnvironment: vi.fn(),
  cancelProjectHealth: vi.fn(),
  cancelStartWorkspace: vi.fn(),
  cancelWorkspacePreflight: vi.fn(),
  createProfile: vi.fn(),
  createProfileFromTemplate: vi.fn(),
  createProfileTemplate: vi.fn(),
  currentWorkspaceRun: vi.fn(),
  dispatchWorkspaceTaskControl: vi.fn(),
  deleteProfile: vi.fn(),
  deleteProfileTemplate: vi.fn(),
  dependencyHealth: vi.fn(),
  listProfiles: vi.fn(),
  listWorkspaceTaskControls: vi.fn().mockResolvedValue([]),
  listProfileTemplates: vi.fn(),
  onOpenRequest: vi.fn(async () => () => undefined),
  openProfileIn: vi.fn(),
  packageDependencySummary: vi.fn(),
  previewProjectEnvironment: vi.fn(),
  profileCopyPath: vi.fn(),
  profileOpenTargets: vi.fn(),
  projectHealth: vi.fn(),
  retryWorkspace: vi.fn(),
  startWorkspace: vi.fn(),
  stopWorkspace: vi.fn(),
  getWorkspaceTaskControlReceipt: vi.fn().mockResolvedValue(null),
  takePendingOpen: vi.fn(async () => null),
  updateProfile: vi.fn(),
  updateProfileTemplate: vi.fn(),
  workspacePreflight: vi.fn(),
  wslRuntimeSuggestions: vi.fn(),
}));

const firstProfile: ProjectProfile = {
  id: "p-1",
  name: "devbox",
  windowsPath: "C:\\projects\\devbox",
  wsl: { distro: "Ubuntu", path: "/mnt/e/projects/devbox" },
  gitRoot: "C:\\projects\\devbox",
  expectedPorts: [1420],
  runManagerServiceIds: ["devbox-dev"],
  environment: null,
};
const secondProfile: ProjectProfile = {
  id: "p-2",
  name: "toolbox",
  windowsPath: "E:\\projects\\toolbox",
  wsl: { distro: "Ubuntu", path: "/mnt/e/projects/toolbox" },
  gitRoot: "E:\\projects\\toolbox",
  expectedPorts: [],
  runManagerServiceIds: [],
  environment: null,
};

const listProfilesMock = vi.mocked(listProfiles);
const createProfileMock = vi.mocked(createProfile);
const createProfileFromTemplateMock = vi.mocked(createProfileFromTemplate);
const createProfileTemplateMock = vi.mocked(createProfileTemplate);
const cancelStartWorkspaceMock = vi.mocked(cancelStartWorkspace);
const cancelDependencyHealthMock = vi.mocked(cancelDependencyHealth);
const cancelProjectEnvironmentMock = vi.mocked(cancelProjectEnvironment);
const cancelProjectHealthMock = vi.mocked(cancelProjectHealth);
const cancelWorkspacePreflightMock = vi.mocked(cancelWorkspacePreflight);
const currentWorkspaceRunMock = vi.mocked(currentWorkspaceRun);
const updateProfileMock = vi.mocked(updateProfile);
const updateProfileTemplateMock = vi.mocked(updateProfileTemplate);
const deleteProfileMock = vi.mocked(deleteProfile);
const deleteProfileTemplateMock = vi.mocked(deleteProfileTemplate);
const listProfileTemplatesMock = vi.mocked(listProfileTemplates);
const dependencyHealthMock = vi.mocked(dependencyHealth);
const projectHealthMock = vi.mocked(projectHealth);
const startWorkspaceMock = vi.mocked(startWorkspace);
const retryWorkspaceMock = vi.mocked(retryWorkspace);
const stopWorkspaceMock = vi.mocked(stopWorkspace);
const workspacePreflightMock = vi.mocked(workspacePreflight);
const profileOpenTargetsMock = vi.mocked(profileOpenTargets);
const profileCopyPathMock = vi.mocked(profileCopyPath);
const openProfileInMock = vi.mocked(openProfileIn);
const packageDependencySummaryMock = vi.mocked(packageDependencySummary);
const previewProjectEnvironmentMock = vi.mocked(previewProjectEnvironment);
const wslRuntimeSuggestionsMock = vi.mocked(wslRuntimeSuggestions);
const confirmMock = vi.fn<(message?: string) => boolean>();
const writeTextMock = vi.fn<(value: string) => Promise<void>>();

const missingPackageSummary: PackageDependencySummary = {
  profileId: "p-1",
  source: "Repo Manager dependency-summary/v1",
  status: "missing",
  producerVersion: null,
  freshnessMs: null,
  revision: null,
  packageCount: 0,
  directCount: 0,
  transitiveCount: 0,
  duplicateCount: 0,
  unresolvedDependencyCount: 0,
  missingLockfileCount: 0,
  staleLockfileCount: 0,
  unsupportedCount: 0,
  invalidCount: 0,
  truncated: false,
  ecosystems: [],
};
let profiles: ProjectProfile[];

const freshRuntimeSuggestions: RuntimeSuggestions = {
  source: "WSL Desktop runtime/v1",
  status: "fresh",
  producerVersion: "0.2.1",
  freshnessMs: 12_000,
  ports: [{
    published: 8080,
    sources: [{
      distro: "Ubuntu",
      container: "api",
      containerState: "running",
      target: 80,
      protocol: "tcp",
    }],
  }],
};

const freshEnvironmentPreview: ProjectEnvironmentPreview = {
  source: ".env.local",
  revision: "b".repeat(64),
  hasConflicts: false,
  variables: [
    {
      name: "API_TOKEN",
      source: ".env.local",
      conflict: "none",
      maskedValue: "********",
      secretReference: { kind: "secret-ref/v1", name: "API_TOKEN" },
    },
    {
      name: "NODE_ENV",
      source: ".env.local",
      conflict: "none",
      maskedValue: "de*****",
      secretReference: null,
    },
  ],
};

const nodeTemplate: ProfileTemplate = {
  id: "template-node",
  name: "Node 서비스",
  windowsPath: null,
  wsl: { distro: "Ubuntu", path: "/mnt/e/projects/node" },
  gitRoot: null,
  expectedPorts: [3000],
  runManagerServiceIds: ["node-dev"],
};

const templateSnapshot: ProfileTemplateSnapshot = {
  revision: "a".repeat(64),
  templates: [nodeTemplate],
};

const readyPreflight: WorkspacePreflight = {
  profileId: "p-1",
  ready: true,
  items: [
    {
      key: "required-apps",
      status: "pass",
      detail: "필수 devbox 앱을 사용할 수 있습니다",
      resources: [
        { kind: "app", id: "wsl-desktop:path", state: "available" },
        { kind: "app", id: "code-pad:workspace", state: "available" },
      ],
    },
    {
      key: "working-directory",
      status: "pass",
      detail: "Workspace working directory를 사용할 수 있습니다",
      resources: [{ kind: "directory", id: "workspace-1", state: "available" }],
    },
    {
      key: "ports",
      status: "warning",
      detail: "이미 사용 중인 예상 port가 있습니다",
      resources: [{ kind: "tcp-port", id: "port-1", state: "existing" }],
    },
  ],
};

function profileRow(name: string): HTMLDivElement {
  const row = screen.getByRole("button", { name }).closest(".profile-row");
  if (!(row instanceof HTMLDivElement)) throw new Error(`${name} profile row was not rendered`);
  return row;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  profiles = [{ ...firstProfile }, { ...secondProfile }];
  listProfilesMock.mockReset().mockImplementation(async () => profiles.map((profile) => ({ ...profile })));
  createProfileMock.mockReset().mockResolvedValue(firstProfile);
  createProfileFromTemplateMock.mockReset().mockResolvedValue(firstProfile);
  createProfileTemplateMock.mockReset().mockImplementation(async (template) => template);
  cancelStartWorkspaceMock.mockReset().mockResolvedValue(true);
  cancelDependencyHealthMock.mockReset().mockResolvedValue(false);
  cancelProjectEnvironmentMock.mockReset().mockResolvedValue(false);
  cancelProjectHealthMock.mockReset().mockResolvedValue(false);
  cancelWorkspacePreflightMock.mockReset().mockResolvedValue(false);
  currentWorkspaceRunMock.mockReset().mockResolvedValue(null);
  updateProfileMock.mockReset().mockImplementation(async (profile) => {
    profiles = profiles.map((candidate) => (candidate.id === profile.id ? { ...profile } : candidate));
  });
  updateProfileTemplateMock.mockReset().mockResolvedValue(undefined);
  deleteProfileMock.mockReset().mockImplementation(async (id) => {
    profiles = profiles.filter((profile) => profile.id !== id);
  });
  deleteProfileTemplateMock.mockReset().mockResolvedValue(undefined);
  listProfileTemplatesMock.mockReset().mockResolvedValue(templateSnapshot);
  dependencyHealthMock.mockReset().mockImplementation(async (profileId) => ({
    ...readyPreflight,
    profileId,
  }));
  packageDependencySummaryMock.mockReset().mockImplementation(async (profileId) => ({
    ...missingPackageSummary,
    profileId,
  }));
  projectHealthMock.mockReset().mockImplementation(async (profileId) => ({
    profileId,
    items: [
      { name: "git", ok: true, detail: "clean" },
      { name: "wsl", ok: true, detail: "Ubuntu" },
    ],
  }));
  startWorkspaceMock.mockReset().mockImplementation(async (profileId) => ({
    runId: `run-${profileId}`,
    profileId,
    steps: [],
    resourceProvenance: [
      { kind: "tcp-port", id: "port-1", state: "existing" },
      { kind: "process", id: "code-pad", state: "workbenchStarted" },
    ],
    canRetry: false,
    retryCount: 0,
    failedStep: null,
  }));
  retryWorkspaceMock.mockReset().mockImplementation(async (runId, profileId) => ({
    runId,
    profileId,
    steps: [],
    resourceProvenance: [],
    retryCount: 1,
    canRetry: false,
    failedStep: null,
  }));
  workspacePreflightMock.mockReset().mockImplementation(async (profileId) => ({
    ...readyPreflight,
    profileId,
  }));
  stopWorkspaceMock.mockReset().mockResolvedValue(1);
  profileOpenTargetsMock.mockReset().mockResolvedValue([
    { id: "code-pad", displayName: "Code Pad", payloadKind: "workspace" },
    { id: "wsl-desktop", displayName: "WSL Desktop", payloadKind: "path" },
  ]);
  profileCopyPathMock.mockReset().mockImplementation(async (profileId) => (
    profileId === firstProfile.id ? firstProfile.windowsPath! : secondProfile.windowsPath!
  ));
  openProfileInMock.mockReset().mockResolvedValue(undefined);
  previewProjectEnvironmentMock.mockReset().mockRejectedValue(new Error("native preview unavailable"));
  wslRuntimeSuggestionsMock.mockReset().mockResolvedValue(freshRuntimeSuggestions);
  confirmMock.mockReset().mockReturnValue(false);
  writeTextMock.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(window, "confirm", { configurable: true, value: confirmMock });
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
});

afterEach(() => cleanup());

describe("Workbench profile context menu", () => {
  it("renders profiles, auto-selects the first one, and loads its health", async () => {
    render(<App />);

    await screen.findByRole("button", { name: "devbox" });
    expect(profileRow("devbox").getAttribute("aria-current")).toBe("true");
    expect(await screen.findByText("clean")).toBeTruthy();
    expect(projectHealthMock).toHaveBeenCalledWith("p-1");
  });

  it("selects the right-clicked profile and shows the exact app-owned actions", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "toolbox" });
    const target = profileRow("toolbox");

    fireEvent.contextMenu(target);

    expect(target.getAttribute("aria-current")).toBe("true");
    for (const label of [
      "Start Workspace",
      "Stop What I Started",
      "프로필 편집",
      "삭제",
      "경로 복사",
      "다른 앱으로 열기",
    ]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeTruthy();
    }
    expect(screen.getByRole("menuitem", { name: "Stop What I Started" }).getAttribute("aria-disabled"))
      .toBe("true");
    expect(screen.getByRole("menuitem", { name: "Stop What I Started" }).className)
      .toContain("danger");
    expect(screen.getByRole("menuitem", { name: "삭제" }).className).toContain("danger");
    await waitFor(() => {
      expect(screen.getByRole("menuitem", { name: "다른 앱으로 열기" }).getAttribute("aria-disabled"))
        .toBeNull();
    });
  });

  it("starts the exact keyboard-targeted profile and restores row focus", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "toolbox" });
    const target = profileRow("toolbox");
    target.focus();

    fireEvent.keyDown(target, { key: "F10", code: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "Start Workspace" }));

    await screen.findByRole("dialog", { name: "Start Workspace 사전 점검" });
    fireEvent.click(screen.getByRole("button", { name: "계속 시작" }));
    await waitFor(() => expect(startWorkspaceMock).toHaveBeenCalledWith("p-2"));
    await waitFor(() => expect(document.activeElement).toBe(target));
    expect(await screen.findByRole("button", { name: "Stop What I Started" })).toBeTruthy();
    expect(screen.getByText("Resource ownership")).toBeTruthy();
    expect(screen.getByText("port-1")).toBeTruthy();
    expect(screen.getByText("Workbench가 시작")).toBeTruthy();
  });

  it("blocks the launcher when preflight reports a required resource failure", async () => {
    workspacePreflightMock.mockResolvedValueOnce({
      ...readyPreflight,
      ready: false,
      items: [{
        key: "required-apps",
        status: "failure",
        detail: "필수 devbox 앱이 없습니다. Devbox Manager에서 설치하세요",
        resources: [{ kind: "app", id: "code-pad:workspace", state: "missing" }],
      }],
    });

    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "Start Workspace" }));

    expect(await screen.findByRole("dialog", { name: "Start Workspace 사전 점검" })).toBeTruthy();
    expect(screen.getByText("필수 devbox 앱이 없습니다. Devbox Manager에서 설치하세요")).toBeTruthy();
    expect(screen.getByRole("button", { name: "계속 시작" })).toBeDisabled();
    expect(startWorkspaceMock).not.toHaveBeenCalled();
  });

  it("ignores a late preflight result after the selected profile changes", async () => {
    const pending = deferred<WorkspacePreflight>();
    workspacePreflightMock.mockReturnValueOnce(pending.promise);

    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "Start Workspace" }));
    await waitFor(() => expect(workspacePreflightMock).toHaveBeenCalledWith("p-1"));

    fireEvent.click(profileRow("toolbox"));
    pending.resolve({ ...readyPreflight, profileId: "p-1" });

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Start Workspace 사전 점검" })).toBeNull());
    expect(startWorkspaceMock).not.toHaveBeenCalled();
  });

  it("cancels an in-flight preflight from its loading state", async () => {
    const pending = deferred<WorkspacePreflight>();
    workspacePreflightMock.mockReturnValueOnce(pending.promise);

    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "Start Workspace" }));
    expect(await screen.findByText("Start Workspace 사전 점검 중…")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "취소" }));

    await waitFor(() => expect(cancelWorkspacePreflightMock).toHaveBeenCalledWith("p-1"));
    pending.reject(new Error("cancelled"));
    await pending.promise.catch(() => undefined);
    expect(screen.queryByText("Start Workspace 사전 점검 중…")).toBeNull();
    expect(startWorkspaceMock).not.toHaveBeenCalled();
  });

  it("cancels the review with Escape without launching anything", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "Start Workspace" }));

    const dialog = await screen.findByRole("dialog", { name: "Start Workspace 사전 점검" });
    expect(screen.getByRole("button", { name: "계속 시작" })).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Escape" });

    expect(screen.queryByRole("dialog", { name: "Start Workspace 사전 점검" })).toBeNull();
    expect(startWorkspaceMock).not.toHaveBeenCalled();
  });

  it("keeps keyboard focus inside the preflight review", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Start Workspace" }));
    const dialog = await screen.findByRole("dialog", { name: "Start Workspace 사전 점검" });
    const continueButton = screen.getByRole("button", { name: "계속 시작" });
    const cancelButton = screen.getByRole("button", { name: "취소" });

    cancelButton.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(continueButton).toHaveFocus();

    continueButton.focus();
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(cancelButton).toHaveFocus();
  });

  it("requires a fresh review when the backend rejects a stale preflight", async () => {
    startWorkspaceMock.mockRejectedValueOnce(new Error("C:\\private\\TOP_SECRET"));
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "Start Workspace" }));
    await screen.findByRole("dialog", { name: "Start Workspace 사전 점검" });
    fireEvent.click(screen.getByRole("button", { name: "계속 시작" }));

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Start Workspace 사전 점검" })).toBeNull());
    expect(screen.getByRole("alert")).toHaveTextContent("사전 점검을 다시 실행하세요");
    expect(screen.queryByText(/TOP_SECRET|private/)).toBeNull();
  });

  it("keeps the target profile selected while Continue is starting it", async () => {
    const pending = deferred<WorkspaceRun>();
    startWorkspaceMock.mockReturnValueOnce(pending.promise);

    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "Start Workspace" }));
    await screen.findByRole("dialog", { name: "Start Workspace 사전 점검" });
    fireEvent.click(screen.getByRole("button", { name: "계속 시작" }));
    await waitFor(() => expect(startWorkspaceMock).toHaveBeenCalledWith("p-1"));

    fireEvent.click(profileRow("toolbox"));
    expect(profileRow("devbox").getAttribute("aria-current")).toBe("true");
    expect(profileRow("toolbox").getAttribute("aria-current")).toBeNull();

    pending.resolve({
      runId: "run-p-1",
      profileId: "p-1",
      steps: [],
      resourceProvenance: [],
    });
    await screen.findByRole("button", { name: "Stop What I Started" });
  });

  it("ignores a late preflight result after the component unmounts", async () => {
    const pending = deferred<WorkspacePreflight>();
    workspacePreflightMock.mockReturnValueOnce(pending.promise);
    const rendered = render(<App />);

    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "Start Workspace" }));
    await waitFor(() => expect(workspacePreflightMock).toHaveBeenCalledWith("p-1"));
    rendered.unmount();

    pending.resolve({ ...readyPreflight, profileId: "p-1" });
    await pending.promise;
    expect(startWorkspaceMock).not.toHaveBeenCalled();
  });

  it("sends native cancellation for an in-flight Start Workspace", async () => {
    const pending = deferred<Awaited<ReturnType<typeof startWorkspace>>>();
    startWorkspaceMock.mockReturnValueOnce(pending.promise);
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });

    fireEvent.click(screen.getByRole("button", { name: "Start Workspace" }));
    await screen.findByRole("dialog", { name: "Start Workspace 사전 점검" });
    fireEvent.click(screen.getByRole("button", { name: "계속 시작" }));
    const cancel = await screen.findByRole("button", { name: "시작 취소" });
    fireEvent.click(cancel);
    await waitFor(() => expect(cancelStartWorkspaceMock).toHaveBeenCalledWith("p-1"));

    pending.reject(new Error("cancelled"));
    expect(await screen.findByText("Workspace 시작을 취소했습니다.")).toBeTruthy();
  });

  it("gates active-run lifecycle by profile and confirms exact stop ownership", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    const first = profileRow("devbox");
    const second = profileRow("toolbox");

    fireEvent.contextMenu(first);
    fireEvent.click(screen.getByRole("menuitem", { name: "Start Workspace" }));
    await screen.findByRole("dialog", { name: "Start Workspace 사전 점검" });
    fireEvent.click(screen.getByRole("button", { name: "계속 시작" }));
    await waitFor(() => expect(startWorkspaceMock).toHaveBeenCalledWith("p-1"));

    fireEvent.contextMenu(second);
    expect(screen.getByRole("menuitem", { name: "Start Workspace" }).getAttribute("aria-disabled"))
      .toBe("true");
    expect(screen.getByRole("menuitem", { name: "Stop What I Started" }).getAttribute("aria-disabled"))
      .toBe("true");

    fireEvent.contextMenu(first);
    expect(screen.getByRole("menuitem", { name: "Stop What I Started" }).getAttribute("aria-disabled"))
      .toBeNull();
    expect(screen.getByRole("menuitem", { name: "삭제" }).getAttribute("aria-disabled"))
      .toBe("true");
    fireEvent.click(screen.getByRole("menuitem", { name: "Stop What I Started" }));
    expect(confirmMock).toHaveBeenCalledWith(
      "'devbox'에서 Workbench가 시작한 리소스만 중지할까요? 시작 전부터 실행 중이던 리소스는 유지됩니다.",
    );
    expect(stopWorkspaceMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.contextMenu(first);
    fireEvent.click(screen.getByRole("menuitem", { name: "Stop What I Started" }));
    await waitFor(() => expect(stopWorkspaceMock).toHaveBeenCalledWith("run-p-1", "p-1"));
  });

  it("keeps a run visible when Stop What I Started retains ownership after a failed termination", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "Start Workspace" }));
    await screen.findByRole("dialog", { name: "Start Workspace 사전 점검" });
    fireEvent.click(screen.getByRole("button", { name: "계속 시작" }));
    await screen.findByRole("button", { name: "Stop What I Started" });

    currentWorkspaceRunMock.mockResolvedValueOnce({
      runId: "run-p-1",
      profileId: "p-1",
    });
    stopWorkspaceMock.mockResolvedValueOnce(0);
    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(screen.getByRole("button", { name: "Stop What I Started" }));

    await waitFor(() => expect(stopWorkspaceMock).toHaveBeenCalledWith("run-p-1", "p-1"));
    expect(await screen.findByRole("button", { name: "Stop What I Started" })).toBeTruthy();
    expect(screen.getByRole("alert")).toHaveTextContent("일부 Workbench 프로세스를 안전하게 종료하지 못했습니다");
  });

  it("restores backend run ownership after a frontend reload", async () => {
    currentWorkspaceRunMock.mockResolvedValueOnce({
      runId: "restored-run",
      profileId: "p-1",
    });
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });

    fireEvent.contextMenu(profileRow("devbox"));

    expect(screen.getByRole("menuitem", { name: "Start Workspace" }).getAttribute("aria-disabled"))
      .toBe("true");
    expect(screen.getByRole("menuitem", { name: "Stop What I Started" }).getAttribute("aria-disabled"))
      .toBeNull();
    expect(screen.getByRole("menuitem", { name: "삭제" }).getAttribute("aria-disabled"))
      .toBe("true");
  });

  it("edits the exact context profile", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "toolbox" });
    const target = profileRow("toolbox");
    target.focus();

    fireEvent.keyDown(target, { key: "ContextMenu", code: "ContextMenu" });
    fireEvent.click(screen.getByRole("menuitem", { name: "프로필 편집" }));

    expect(await screen.findByRole("heading", { name: "프로필 편집" })).toBeTruthy();
    expect(screen.getByDisplayValue("toolbox")).toBeTruthy();
  });

  it("keeps invalid port text in the editing buffer and blocks save", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });

    fireEvent.contextMenu(profileRow("devbox"));
    fireEvent.click(screen.getByRole("menuitem", { name: "프로필 편집" }));

    const ports = screen.getByDisplayValue("1420");
    fireEvent.change(ports, { target: { value: "1420, nope" } });
    expect(screen.getByDisplayValue("1420, nope")).toBeTruthy();
    expect(screen.getByText("포트는 쉼표로 구분한 1~65535 사이의 숫자여야 합니다.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "저장" })).toBeDisabled();
    expect(updateProfileMock).not.toHaveBeenCalled();
  });

  it("supports adding, editing, removing, and saving service ID rows", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });

    fireEvent.contextMenu(profileRow("devbox"));
    fireEvent.click(screen.getByRole("menuitem", { name: "프로필 편집" }));
    fireEvent.click(screen.getByRole("button", { name: "+ 서비스 추가" }));

    const secondService = screen.getByLabelText("서비스 2");
    fireEvent.change(secondService, { target: { value: "worker" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "저장" })).not.toBeDisabled());
    fireEvent.click(screen.getByRole("button", { name: "저장" }));

    await waitFor(() => expect(updateProfileMock).toHaveBeenCalledWith({
      ...firstProfile,
      runManagerServiceIds: ["devbox-dev", "worker"],
    }));

    fireEvent.contextMenu(profileRow("devbox"));
    fireEvent.click(screen.getByRole("menuitem", { name: "프로필 편집" }));
    expect(screen.getByLabelText("서비스 2")).toHaveValue("worker");
    fireEvent.click(screen.getByRole("button", { name: "서비스 2 삭제" }));
    expect(screen.queryByLabelText("서비스 2")).toBeNull();
  });

  it("requires explicit confirmation before deleting only the context profile", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "toolbox" });
    const target = profileRow("toolbox");

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
    expect(confirmMock).toHaveBeenCalledWith(
      "'toolbox' 프로필을 삭제할까요? 저장된 프로필 정의만 삭제하며 프로젝트 파일과 이미 실행 중이던 외부 리소스는 변경하지 않습니다.",
    );
    expect(deleteProfileMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));

    await waitFor(() => expect(deleteProfileMock).toHaveBeenCalledWith("p-2"));
    await waitFor(() => expect(screen.queryByRole("button", { name: "toolbox" })).toBeNull());
  });

  it("copies a backend-validated path and opens only a catalog-derived target", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "toolbox" });
    const target = profileRow("toolbox");

    fireEvent.contextMenu(target);
    fireEvent.click(screen.getByRole("menuitem", { name: "경로 복사" }));
    await waitFor(() => expect(profileCopyPathMock).toHaveBeenCalledWith("p-2"));
    expect(writeTextMock).toHaveBeenCalledWith("E:\\projects\\toolbox");

    fireEvent.contextMenu(target);
    await waitFor(() => {
      expect(screen.getByRole("menuitem", { name: "다른 앱으로 열기" }).getAttribute("aria-disabled"))
        .toBeNull();
    });
    fireEvent.click(screen.getByRole("menuitem", { name: "다른 앱으로 열기" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Code Pad" }));
    await waitFor(() => expect(openProfileInMock).toHaveBeenCalledWith("p-2", "code-pad"));
  });

  it("keeps cross-app submenu fail-closed when target discovery fails", async () => {
    profileOpenTargetsMock.mockRejectedValueOnce(new Error("TOP_SECRET"));
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });

    fireEvent.contextMenu(profileRow("devbox"));

    await waitFor(() => expect(screen.getByText("다른 앱으로 열기 대상을 확인하지 못했습니다")).toBeTruthy());
    expect(screen.getByRole("menuitem", { name: "다른 앱으로 열기" }).getAttribute("aria-disabled"))
      .toBe("true");
    expect(screen.queryByText("TOP_SECRET")).toBeNull();
  });

  it("keeps the existing empty-profile editor guard", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });

    fireEvent.click(screen.getByRole("button", { name: "+ 프로필" }));

    expect(await screen.findByRole("heading", { name: "새 프로필" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "저장" })).toBeDisabled();
  });

  it("inspects a native project environment and saves only masked metadata", async () => {
    previewProjectEnvironmentMock.mockResolvedValueOnce(freshEnvironmentPreview);
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "devbox 프로필 편집" }));

    fireEvent.change(screen.getByRole("textbox", { name: "환경 파일 이름 (프로젝트 상대)" }), {
      target: { value: ".env.local" },
    });
    fireEvent.click(screen.getByRole("button", { name: "환경 파일 확인" }));

    expect(await screen.findByText("마스킹된 미리보기")).toBeTruthy();
    expect(screen.getByText("********")).toBeTruthy();
    expect(screen.getByText("de*****")).toBeTruthy();
    expect(screen.getByText("secret reference")).toBeTruthy();
    expect(screen.queryByText("top-secret")).toBeNull();
    expect(previewProjectEnvironmentMock).toHaveBeenCalledWith({
      windowsPath: "C:\\projects\\devbox",
      wsl: { distro: "Ubuntu", path: "/mnt/e/projects/devbox" },
      source: ".env.local",
    });

    fireEvent.click(screen.getByRole("button", { name: "저장" }));
    await waitFor(() => expect(updateProfileMock).toHaveBeenCalledWith({
      ...firstProfile,
      environment: {
        enabled: false,
        source: ".env.local",
        revision: "b".repeat(64),
        variables: freshEnvironmentPreview.variables.map(({ name, source, conflict, secretReference }) => ({
          name,
          source,
          conflict,
          secretReference,
        })),
      },
    }));
  });

  it("blocks saving an inspected environment with a conflict when enabled", async () => {
    previewProjectEnvironmentMock.mockResolvedValueOnce({
      ...freshEnvironmentPreview,
      hasConflicts: true,
      variables: [{
        ...freshEnvironmentPreview.variables[0],
        name: "PATH",
        conflict: "reserved",
        secretReference: null,
      }],
    });
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "devbox 프로필 편집" }));
    fireEvent.change(screen.getByRole("textbox", { name: "환경 파일 이름 (프로젝트 상대)" }), {
      target: { value: ".env" },
    });
    fireEvent.click(screen.getByRole("button", { name: "환경 파일 확인" }));
    await screen.findByText("마스킹된 미리보기");
    fireEvent.click(screen.getByRole("checkbox", { name: "Start Workspace에서 환경 주입 사용" }));

    expect(screen.getByRole("button", { name: "저장" })).toBeDisabled();
    expect(screen.getByText(/충돌을 해결한 뒤 저장/)).toBeTruthy();
    expect(updateProfileMock).not.toHaveBeenCalled();
  });

  it("ignores a late environment preview after cancel", async () => {
    const pending = deferred<ProjectEnvironmentPreview>();
    previewProjectEnvironmentMock.mockReturnValueOnce(pending.promise);
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "devbox 프로필 편집" }));
    fireEvent.change(screen.getByRole("textbox", { name: "환경 파일 이름 (프로젝트 상대)" }), {
      target: { value: ".env" },
    });
    fireEvent.click(screen.getByRole("button", { name: "환경 파일 확인" }));
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    pending.resolve({
      ...freshEnvironmentPreview,
      variables: [{
        ...freshEnvironmentPreview.variables[0],
        name: "DO_NOT_RENDER_LATE_SECRET",
        maskedValue: "late-secret",
      }],
    });

    await Promise.resolve();
    await Promise.resolve();
    expect(screen.queryByText("DO_NOT_RENDER_LATE_SECRET")).toBeNull();
    expect(screen.queryByText("late-secret")).toBeNull();
    expect(screen.queryByRole("heading", { name: "프로필 편집" })).toBeNull();
  });

  it("submits the form with Enter and closes the editor with Escape", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });

    fireEvent.click(screen.getByRole("button", { name: "devbox 프로필 편집" }));
    const name = screen.getByRole("textbox", { name: "이름" });
    expect(name).toHaveFocus();
    fireEvent.change(name, { target: { value: "devbox-renamed" } });
    fireEvent.submit(name.closest("form")!);

    await waitFor(() => expect(updateProfileMock).toHaveBeenCalledWith({
      ...firstProfile,
      name: "devbox-renamed",
    }));
    expect(screen.queryByRole("heading", { name: "프로필 편집" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "devbox-renamed 프로필 편집" }));
    const renamed = screen.getByRole("textbox", { name: "이름" });
    fireEvent.keyDown(renamed, { key: "Escape", isComposing: true });
    expect(screen.getByRole("heading", { name: "프로필 편집" })).toBeTruthy();
    fireEvent.keyDown(renamed, { key: "Escape" });
    expect(screen.queryByRole("heading", { name: "프로필 편집" })).toBeNull();
  });

  it("does not expose raw backend errors in the editor", async () => {
    updateProfileMock.mockRejectedValueOnce(new Error("TOP_SECRET C:\\private\\project"));
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "devbox 프로필 편집" }));
    fireEvent.change(screen.getByRole("textbox", { name: "이름" }), { target: { value: "safe-name" } });
    fireEvent.click(screen.getByRole("button", { name: "저장" }));

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("프로필을 저장할 수 없습니다"));
    expect(screen.queryByText(/TOP_SECRET|private/)).toBeNull();
  });

  it("blocks a second editor action while a save is pending", async () => {
    const pendingSave = deferred<void>();
    updateProfileMock.mockReturnValueOnce(pendingSave.promise);
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "devbox 프로필 편집" }));
    fireEvent.change(screen.getByRole("textbox", { name: "이름" }), { target: { value: "safe-name" } });
    fireEvent.click(screen.getByRole("button", { name: "저장" }));

    await waitFor(() => expect(updateProfileMock).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("button", { name: "저장" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "취소" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "+ 프로필" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "새로고침" })).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "이름" })).toBeDisabled();
    pendingSave.resolve();
    await waitFor(() => expect(screen.queryByRole("heading", { name: "프로필 편집" })).toBeNull());
  });

  it("revalidates and explicitly adds selected runtime ports only to the draft until Save", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "devbox 프로필 편집" }));

    fireEvent.click(screen.getByRole("button", { name: "제안 불러오기" }));
    const candidate = await screen.findByRole("checkbox", { name: "published port 8080 선택" });
    expect(screen.getByText(/WSL Desktop runtime\/v1 · producer 0.2.1/)).toBeTruthy();
    fireEvent.click(candidate);
    fireEvent.click(screen.getByRole("button", { name: "선택 포트를 초안에 반영" }));

    await waitFor(() => expect(screen.getByDisplayValue("1420, 8080")).toBeTruthy());
    expect(wslRuntimeSuggestionsMock).toHaveBeenCalledTimes(2);
    expect(updateProfileMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "저장" }));
    await waitFor(() => expect(updateProfileMock).toHaveBeenCalledWith({
      ...firstProfile,
      expectedPorts: [1420, 8080],
    }));
  });

  it("requires extra confirmation for stale suggestions and preserves the draft when declined", async () => {
    wslRuntimeSuggestionsMock.mockResolvedValue({
      ...freshRuntimeSuggestions,
      status: "stale",
      freshnessMs: 180_000,
    });
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "devbox 프로필 편집" }));
    fireEvent.click(screen.getByRole("button", { name: "제안 불러오기" }));
    fireEvent.click(await screen.findByRole("checkbox", { name: "published port 8080 선택" }));
    fireEvent.click(screen.getByRole("button", { name: "선택 포트를 초안에 반영" }));

    await waitFor(() => expect(confirmMock).toHaveBeenCalledTimes(1));
    expect(screen.getByDisplayValue("1420")).toBeTruthy();
    expect(updateProfileMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(screen.getByRole("button", { name: "선택 포트를 초안에 반영" }));
    await waitFor(() => expect(screen.getByDisplayValue("1420, 8080")).toBeTruthy());
  });

  it("blocks expired suggestions and distinguishes missing from corrupt producers", async () => {
    wslRuntimeSuggestionsMock.mockResolvedValueOnce({
      ...freshRuntimeSuggestions,
      status: "expired",
      freshnessMs: 901_000,
    });
    const rendered = render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "devbox 프로필 편집" }));
    fireEvent.click(screen.getByRole("button", { name: "제안 불러오기" }));
    expect(await screen.findByText("만료된 snapshot — 반영 불가")).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: "published port 8080 선택" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "선택 포트를 초안에 반영" })).toBeDisabled();

    rendered.unmount();
    wslRuntimeSuggestionsMock.mockResolvedValueOnce({
      source: "WSL Desktop runtime/v1",
      status: "missing",
      producerVersion: null,
      freshnessMs: null,
      ports: [],
    });
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "devbox 프로필 편집" }));
    fireEvent.click(screen.getByRole("button", { name: "제안 불러오기" }));
    expect(await screen.findByText("WSL Desktop snapshot 없음")).toBeTruthy();
    expect(screen.queryByText("WSL Desktop snapshot을 안전하게 읽을 수 없음")).toBeNull();

    cleanup();
    wslRuntimeSuggestionsMock.mockResolvedValueOnce({
      source: "WSL Desktop runtime/v1",
      status: "corrupt",
      producerVersion: null,
      freshnessMs: null,
      ports: [],
    });
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "devbox 프로필 편집" }));
    fireEvent.click(screen.getByRole("button", { name: "제안 불러오기" }));
    expect(await screen.findByText("WSL Desktop snapshot을 안전하게 읽을 수 없음")).toBeTruthy();
    expect(screen.queryByText("WSL Desktop snapshot 없음")).toBeNull();
  });

  it("does not apply a selection that disappeared during acceptance revalidation", async () => {
    wslRuntimeSuggestionsMock
      .mockResolvedValueOnce(freshRuntimeSuggestions)
      .mockResolvedValueOnce({
        ...freshRuntimeSuggestions,
        ports: [{
          published: 9000,
          sources: freshRuntimeSuggestions.ports[0].sources,
        }],
      });
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "devbox 프로필 편집" }));
    fireEvent.click(screen.getByRole("button", { name: "제안 불러오기" }));
    fireEvent.click(await screen.findByRole("checkbox", { name: "published port 8080 선택" }));
    fireEvent.click(screen.getByRole("button", { name: "선택 포트를 초안에 반영" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("runtime 상태가 변경되었습니다");
    expect(screen.getByDisplayValue("1420")).toBeTruthy();
    expect(updateProfileMock).not.toHaveBeenCalled();
  });

  it("ignores a late runtime response after the editor closes and never reflects its data", async () => {
    const pending = deferred<RuntimeSuggestions>();
    wslRuntimeSuggestionsMock.mockReturnValueOnce(pending.promise);
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "devbox 프로필 편집" }));
    fireEvent.click(screen.getByRole("button", { name: "제안 불러오기" }));
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    pending.resolve({
      ...freshRuntimeSuggestions,
      ports: [{
        ...freshRuntimeSuggestions.ports[0],
        sources: [{
          ...freshRuntimeSuggestions.ports[0].sources[0],
          container: "DO_NOT_RENDER_LATE_VALUE",
        }],
      }],
    });

    await Promise.resolve();
    await Promise.resolve();
    expect(screen.queryByText(/DO_NOT_RENDER_LATE_VALUE/)).toBeNull();
    expect(screen.queryByRole("heading", { name: "프로필 편집" })).toBeNull();
  });

  it("ignores stale health responses after the selected profile changes", async () => {
    const firstHealth = deferred<ProjectHealth>();
    const secondHealth = deferred<ProjectHealth>();
    projectHealthMock.mockImplementation((profileId) => (
      profileId === firstProfile.id ? firstHealth.promise : secondHealth.promise
    ));
    render(<App />);
    await screen.findByRole("button", { name: "toolbox" });
    fireEvent.click(screen.getByRole("button", { name: "toolbox" }));
    await waitFor(() => expect(projectHealthMock).toHaveBeenCalledWith(secondProfile.id));

    secondHealth.resolve({ profileId: secondProfile.id, items: [{ name: "health", ok: true, detail: "new result" }] });
    expect(await screen.findByText("new result")).toBeTruthy();
    firstHealth.resolve({ profileId: firstProfile.id, items: [{ name: "health", ok: true, detail: "stale result" }] });
    await waitFor(() => expect(screen.queryByText("stale result")).toBeNull());
  });

  it("keeps the newest refresh result when an older request resolves later", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "toolbox" });
    const older = deferred<ProjectProfile[]>();
    const newer = deferred<ProjectProfile[]>();
    listProfilesMock.mockImplementationOnce(() => older.promise).mockImplementationOnce(() => newer.promise);

    fireEvent.click(screen.getByRole("button", { name: "새로고침" }));
    fireEvent.click(screen.getByRole("button", { name: "새로고침" }));
    const latestProfile: ProjectProfile = {
      ...secondProfile,
      id: "p-latest",
      name: "latest",
      windowsPath: "E:\\projects\\latest",
      gitRoot: "E:\\projects\\latest",
    };
    newer.resolve([latestProfile]);
    await screen.findByRole("button", { name: "latest" });
    older.resolve([{ ...firstProfile, name: "stale" }]);
    await waitFor(() => expect(screen.queryByRole("button", { name: "stale" })).toBeNull());
    expect(screen.getByRole("button", { name: "latest" })).toBeTruthy();
  });

  it("clears stale actionable profiles when a refresh fails without echoing the cause", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    listProfilesMock.mockRejectedValueOnce(new Error("TOP_SECRET C:\\private\\profile-store"));

    fireEvent.click(screen.getByRole("button", { name: "새로고침" }));

    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent("프로필 목록을 불러올 수 없습니다.");
    });
    expect(screen.queryByRole("button", { name: "devbox" })).toBeNull();
    expect(screen.queryByText(/TOP_SECRET|private|profile-store/)).toBeNull();
  });

  it("shows bounded dependency health states and refreshes them independently", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    expect(await screen.findByText("필수 devbox 앱을 사용할 수 있습니다")).toBeTruthy();
    expect(screen.getByText("이미 사용 중인 예상 port가 있습니다")).toBeTruthy();
    expect(dependencyHealthMock).toHaveBeenCalledWith("p-1");

    dependencyHealthMock.mockResolvedValueOnce({
      profileId: "p-1",
      ready: false,
      items: [{
        key: "ports",
        status: "failure",
        detail: "예상 TCP port 충돌이 있습니다",
        resources: [{ kind: "tcp-port", id: "port-1", state: "conflict" }],
      }],
    });
    fireEvent.click(screen.getByRole("button", { name: "의존성 새로고침" }));
    expect(await screen.findByText("예상 TCP port 충돌이 있습니다")).toBeTruthy();
    expect(screen.getByText("충돌")).toBeTruthy();
  });

  it("creates a project from a selected template without importing environment data", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "새 프로젝트 wizard" }));
    const dialog = await screen.findByRole("dialog", { name: "새 프로젝트 wizard" });
    expect(screen.getByRole("option", { name: "Node 서비스" })).toBeTruthy();
    fireEvent.change(screen.getByLabelText("프로젝트 이름"), { target: { value: "node-app" } });
    fireEvent.change(screen.getByLabelText("Windows 경로"), { target: { value: "E:\\projects\\node-app" } });
    fireEvent.change(screen.getByLabelText("WSL 경로"), { target: { value: "/mnt/e/projects/node-app" } });
    fireEvent.click(screen.getByRole("button", { name: "프로젝트 만들기" }));
    await waitFor(() => expect(createProfileFromTemplateMock).toHaveBeenCalledWith(
      "template-node",
      expect.objectContaining({ name: "node-app", environment: null }),
    ));
    expect(dialog).toBeTruthy();
  });

  it("preserves wizard input when switching to direct entry", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "새 프로젝트 wizard" }));
    await screen.findByRole("dialog", { name: "새 프로젝트 wizard" });
    await screen.findByDisplayValue("/mnt/e/projects/node");

    fireEvent.change(screen.getByLabelText("프로젝트 이름"), { target: { value: "node-app" } });
    fireEvent.change(screen.getByRole("combobox", { name: "프로필 템플릿" }), { target: { value: "" } });

    expect(screen.getByLabelText("프로젝트 이름")).toHaveValue("node-app");
    expect(screen.getByLabelText("WSL 경로")).toHaveValue("/mnt/e/projects/node");
    expect(screen.getByLabelText("예상 포트 (쉼표)")).toHaveValue("3000");
  });

  it("supports template CRUD in the dedicated manager", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "템플릿 관리" }));
    await screen.findByRole("dialog", { name: "프로필 템플릿 관리" });
    fireEvent.change(screen.getByLabelText("템플릿 이름"), { target: { value: "Node updated" } });
    fireEvent.click(screen.getByRole("button", { name: "템플릿 저장" }));
    await waitFor(() => expect(updateProfileTemplateMock).toHaveBeenCalledWith(
      expect.objectContaining({ id: "template-node", name: "Node updated" }),
      templateSnapshot.revision,
    ));
  });

  it("creates and deletes templates without changing existing profiles", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "템플릿 관리" }));
    await screen.findByRole("dialog", { name: "프로필 템플릿 관리" });
    fireEvent.click(screen.getByRole("button", { name: "+ 새 템플릿" }));
    fireEvent.change(screen.getByLabelText("템플릿 이름"), { target: { value: "Generic" } });
    fireEvent.click(screen.getByRole("button", { name: "템플릿 저장" }));
    await waitFor(() => expect(createProfileTemplateMock).toHaveBeenCalledWith(
      expect.objectContaining({ name: "Generic", windowsPath: null, wsl: null }),
    ));
    await waitFor(() => expect(screen.getByRole("button", { name: "템플릿 저장" })).not.toBeDisabled());

    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(screen.getByRole("button", { name: "Node 서비스 템플릿 삭제" }));
    await waitFor(() => expect(deleteProfileTemplateMock).toHaveBeenCalledWith(
      "template-node",
      templateSnapshot.revision,
    ));
    expect(updateProfileMock).not.toHaveBeenCalled();
  });

  it("retries a failed step while preserving existing resource provenance", async () => {
    startWorkspaceMock.mockResolvedValueOnce({
      runId: "run-p-1",
      profileId: "p-1",
      steps: [{ name: "open-code-pad", ok: false, detail: "code-pad를 시작할 수 없습니다", status: "failure" }],
      resourceProvenance: [{ kind: "process", id: "wsl-desktop", state: "workbenchStarted" }],
      retryCount: 0,
      canRetry: true,
      failedStep: "open-code-pad",
    });
    retryWorkspaceMock.mockResolvedValueOnce({
      runId: "run-p-1",
      profileId: "p-1",
      steps: [{ name: "open-code-pad", ok: true, detail: "code-pad를 시작했습니다", status: "pass" }],
      resourceProvenance: [{ kind: "process", id: "wsl-desktop", state: "workbenchStarted" }, { kind: "process", id: "code-pad", state: "workbenchStarted" }],
      retryCount: 1,
      canRetry: false,
      failedStep: null,
    });
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    fireEvent.click(screen.getByRole("button", { name: "Start Workspace" }));
    await screen.findByRole("dialog", { name: "Start Workspace 사전 점검" });
    fireEvent.click(screen.getByRole("button", { name: "계속 시작" }));
    await screen.findByRole("button", { name: "실패 단계부터 다시 시도" });
    fireEvent.click(screen.getByRole("button", { name: "실패 단계부터 다시 시도" }));
    await waitFor(() => expect(retryWorkspaceMock).toHaveBeenCalledWith("run-p-1", "p-1"));
    expect(await screen.findByText("code-pad를 시작했습니다")).toBeTruthy();
  });
});
