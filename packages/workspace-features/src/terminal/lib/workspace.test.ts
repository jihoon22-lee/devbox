import { beforeEach, describe, expect, it } from "vitest";
import {
  isSafeWorkspacePath,
  loadLastWorkspace,
  normalizeProfile,
  saveLastWorkspace,
  startCommandError,
  workspaceFromRuntime,
} from "./workspace";
import type { Pane, Tab, WorkspaceDefinition } from "../types";

const workspace: WorkspaceDefinition = {
  tabs: [{
    id: "tab-1",
    title: "개발",
    customTitle: true,
    layout: "cols",
    paneKeys: ["pane-1", "pane-2"],
    sizing: { columns: [0.65, 0.35], rows: [1] },
  }],
  panes: [
    { key: "pane-1", distro: "Ubuntu", cwd: "/mnt/e/devbox", startCommand: "pnpm dev", multiplexer: "native" },
    { key: "pane-2", distro: "Ubuntu", cwd: "E:\\devbox", startCommand: null, multiplexer: "tmux" },
  ],
  activeTabId: "tab-1",
  activePaneKey: "pane-2",
};

beforeEach(() => localStorage.clear());

describe("workspace persistence", () => {
  it("마지막 레이아웃을 versioned JSON으로 왕복한다", () => {
    saveLastWorkspace(workspace);
    expect(loadLastWorkspace()).toEqual(workspace);
    expect(JSON.parse(localStorage.getItem("wsl-desktop:last-layout") ?? "null").version).toBe(2);
  });

  it("version 1 레이아웃을 균등 비율의 version 2 모델로 마이그레이션한다", () => {
    const legacy = {
      version: 1,
      ...workspace,
      tabs: workspace.tabs.map(({ sizing: _sizing, ...tab }) => tab),
    };
    localStorage.setItem("wsl-desktop:last-layout", JSON.stringify(legacy));

    expect(loadLastWorkspace()?.tabs[0].sizing).toEqual({ columns: [0.5, 0.5], rows: [1] });
  });

  it("손상 JSON·버전 불일치·orphan pane을 fail-closed 처리한다", () => {
    localStorage.setItem("wsl-desktop:last-layout", "not-json");
    expect(loadLastWorkspace()).toBeNull();

    localStorage.setItem("wsl-desktop:last-layout", JSON.stringify({ version: 99, ...workspace }));
    expect(loadLastWorkspace()).toBeNull();

    localStorage.setItem("wsl-desktop:last-layout", JSON.stringify({
      version: 2,
      ...workspace,
      panes: workspace.panes.slice(0, 1),
    }));
    expect(loadLastWorkspace()).toBeNull();
  });

  it("runtime session id 대신 stable pane key를 저장한다", () => {
    const panes: Pane[] = [
      { key: "pane-1", sessionId: "session-99", distro: "Ubuntu", cwd: "/mnt/e/devbox", multiplexer: "native" },
    ];
    const tabs: Tab[] = [{
      id: "tab-1",
      title: "개발",
      layout: "grid",
      paneIds: ["session-99"],
      sizing: { columns: [1], rows: [1] },
    }];
    const saved = workspaceFromRuntime(tabs, panes, "tab-1", "session-99");
    expect(saved?.tabs[0].paneKeys).toEqual(["pane-1"]);
    expect(JSON.stringify(saved)).not.toContain("session-99");
  });

  it("복원 실패 placeholder도 원래 key·요청 방식·분할 비율로 저장한다", () => {
    const panes: Pane[] = [{
      key: "pane-1",
      sessionId: null,
      distro: "Ubuntu",
      cwd: "/mnt/e/devbox",
      multiplexer: "zellij",
      requestedMultiplexer: "zellij",
      restoreStatus: "failed",
    }];
    const tabs: Tab[] = [{
      id: "tab-1",
      title: "개발",
      layout: "grid",
      paneIds: ["pane-1"],
      sizing: { columns: [1], rows: [1] },
    }];

    const saved = workspaceFromRuntime(tabs, panes, "tab-1", "pane-1");

    expect(saved?.panes[0]).toMatchObject({ key: "pane-1", multiplexer: "zellij" });
    expect(saved?.activePaneKey).toBe("pane-1");
    expect(saved?.tabs[0].sizing).toEqual({ columns: [1], rows: [1] });
  });
});

describe("workspace safety", () => {
  it("안전한 POSIX·Windows 경로만 허용한다", () => {
    expect(isSafeWorkspacePath("/mnt/e/projects/devbox")).toBe(true);
    expect(isSafeWorkspacePath("E:\\projects\\devbox")).toBe(true);
    expect(isSafeWorkspacePath("relative/path")).toBe(false);
    expect(isSafeWorkspacePath("/work/../escape")).toBe(false);
    expect(isSafeWorkspacePath("E:\\work\\NUL.txt")).toBe(false);
  });

  it("여러 줄·평문 credential 시작 명령은 거부하고 환경 변수 참조는 허용한다", () => {
    expect(startCommandError("echo one\necho two")).not.toBeNull();
    expect(startCommandError("tool --token=literal-value")).not.toBeNull();
    expect(startCommandError("tool --token=$TOKEN next --token=literal-value")).not.toBeNull();
    expect(startCommandError("echo '-----BEGIN OPENSSH PRIVATE KEY-----'")).not.toBeNull();
    expect(startCommandError("tool --token=$TOKEN")).toBeNull();
    expect(startCommandError("pnpm dev")).toBeNull();
    expect(startCommandError("task-runner --mode dev")).toBeNull();
  });

  it("profile 참조 중복과 unsafe path를 거부한다", () => {
    expect(normalizeProfile({ id: "profile-1", name: "개발", ...workspace })).not.toBeNull();
    expect(normalizeProfile({
      id: "profile-1",
      name: "개발",
      ...workspace,
      panes: [{ ...workspace.panes[0], cwd: "../../escape" }, workspace.panes[1]],
    })).toBeNull();
    expect(normalizeProfile({
      id: "profile-1",
      name: "개발",
      tabs: [
        {
          id: "tab-1",
          title: "one",
          customTitle: false,
          layout: "grid",
          paneKeys: ["pane-1"],
          sizing: { columns: [1], rows: [1] },
        },
        {
          id: "tab-2",
          title: "two",
          customTitle: false,
          layout: "grid",
          paneKeys: ["pane-2"],
          sizing: { columns: [1], rows: [1] },
        },
      ],
      panes: workspace.panes,
      activeTabId: "tab-1",
      activePaneKey: "pane-2",
    })).toBeNull();
  });
});
