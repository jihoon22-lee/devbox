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
  type ProjectProfile,
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
const confirmMock = vi.fn<(message?: string) => boolean>();
const writeTextMock = vi.fn<(value: string) => Promise<void>>();
let profiles: ProjectProfile[];

function profileRow(name: string): HTMLDivElement {
  const row = screen.getByRole("button", { name }).closest(".profile-row");
  if (!(row instanceof HTMLDivElement)) throw new Error(`${name} profile row was not rendered`);
  return row;
}

beforeEach(() => {
  profiles = [{ ...firstProfile }, { ...secondProfile }];
  listProfilesMock.mockReset().mockImplementation(async () => profiles.map((profile) => ({ ...profile })));
  createProfileMock.mockReset().mockResolvedValue(firstProfile);
  currentWorkspaceRunMock.mockReset().mockResolvedValue(null);
  updateProfileMock.mockReset().mockResolvedValue(undefined);
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
});
