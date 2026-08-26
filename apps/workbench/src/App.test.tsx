import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  createProfile,
  currentWorkspaceRun,
  deleteProfile,
  listProfiles,
  openProfileIn,
  profileCopyPath,
  profileOpenTargets,
  projectHealth,
  startWorkspace,
  stopWorkspace,
  updateProfile,
  wslRuntimeSuggestions,
  type ProjectHealth,
  type ProjectProfile,
  type RuntimeSuggestions,
} from "./api";

vi.mock("./api", () => ({
  createProfile: vi.fn(),
  currentWorkspaceRun: vi.fn(),
  deleteProfile: vi.fn(),
  listProfiles: vi.fn(),
  onOpenRequest: vi.fn(async () => () => undefined),
  openProfileIn: vi.fn(),
  profileCopyPath: vi.fn(),
  profileOpenTargets: vi.fn(),
  projectHealth: vi.fn(),
  startWorkspace: vi.fn(),
  stopWorkspace: vi.fn(),
  takePendingOpen: vi.fn(async () => null),
  updateProfile: vi.fn(),
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
};
const secondProfile: ProjectProfile = {
  id: "p-2",
  name: "toolbox",
  windowsPath: "E:\\projects\\toolbox",
  wsl: { distro: "Ubuntu", path: "/mnt/e/projects/toolbox" },
  gitRoot: "E:\\projects\\toolbox",
  expectedPorts: [],
  runManagerServiceIds: [],
};

const listProfilesMock = vi.mocked(listProfiles);
const createProfileMock = vi.mocked(createProfile);
const currentWorkspaceRunMock = vi.mocked(currentWorkspaceRun);
const updateProfileMock = vi.mocked(updateProfile);
const deleteProfileMock = vi.mocked(deleteProfile);
const projectHealthMock = vi.mocked(projectHealth);
const startWorkspaceMock = vi.mocked(startWorkspace);
const stopWorkspaceMock = vi.mocked(stopWorkspace);
const profileOpenTargetsMock = vi.mocked(profileOpenTargets);
const profileCopyPathMock = vi.mocked(profileCopyPath);
const openProfileInMock = vi.mocked(openProfileIn);
const wslRuntimeSuggestionsMock = vi.mocked(wslRuntimeSuggestions);
const confirmMock = vi.fn<(message?: string) => boolean>();
const writeTextMock = vi.fn<(value: string) => Promise<void>>();
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

function profileRow(name: string): HTMLDivElement {
  const row = screen.getByRole("button", { name }).closest(".profile-row");
  if (!(row instanceof HTMLDivElement)) throw new Error(`${name} profile row was not rendered`);
  return row;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

beforeEach(() => {
  profiles = [{ ...firstProfile }, { ...secondProfile }];
  listProfilesMock.mockReset().mockImplementation(async () => profiles.map((profile) => ({ ...profile })));
  createProfileMock.mockReset().mockResolvedValue(firstProfile);
  currentWorkspaceRunMock.mockReset().mockResolvedValue(null);
  updateProfileMock.mockReset().mockImplementation(async (profile) => {
    profiles = profiles.map((candidate) => (candidate.id === profile.id ? { ...profile } : candidate));
  });
  deleteProfileMock.mockReset().mockImplementation(async (id) => {
    profiles = profiles.filter((profile) => profile.id !== id);
  });
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
    startedPids: [101],
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

    await waitFor(() => expect(startWorkspaceMock).toHaveBeenCalledWith("p-2"));
    await waitFor(() => expect(document.activeElement).toBe(target));
    expect(await screen.findByRole("button", { name: "Stop What I Started" })).toBeTruthy();
  });

  it("gates active-run lifecycle by profile and confirms exact stop ownership", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "devbox" });
    const first = profileRow("devbox");
    const second = profileRow("toolbox");

    fireEvent.contextMenu(first);
    fireEvent.click(screen.getByRole("menuitem", { name: "Start Workspace" }));
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
});
