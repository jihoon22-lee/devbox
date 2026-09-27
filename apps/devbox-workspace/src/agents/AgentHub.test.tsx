import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { findA11yViolations } from "@devbox/a11y/testing";
import { fixtureDescription } from "@devbox/product-shell/api";
import { nativeCall } from "../native";
import AgentHub from "./AgentHub";
vi.mock("../native", () => ({ nativeCall: vi.fn() }));
const call = vi.mocked(nativeCall);
const base = { projectId: "p1", worktreeId: "w1", revision: 1, target: { kind: "wsl" as const, distroId: "d1" } };
const agent = { ...base, worktreeId: "w2" };
const registry = {
  revision: 1,
  projects: [{ id: "p1", name: "fixture" }],
  worktrees: [
    { id: "w1", projectId: "p1", revision: 1, binding: { root: "/repo", target: base.target }, trustedDigest: null },
    {
      id: "w2",
      projectId: "p1",
      revision: 1,
      binding: { root: "/repo-fix", target: base.target },
      trustedDigest: null,
    },
  ],
};
const task = {
  id: "t1",
  revision: 1,
  projectId: "p1",
  baseWorktreeId: "w1",
  title: "Fix login",
  tool: "claudeCode",
  command: "claude",
  branch: "agent/fix-login",
  targetDir: "/repo-fix",
  worktreeId: "w2",
  terminalId: "terminal-1",
  state: "running",
  createdAtMs: 1,
  updatedAtMs: 1,
};
const navigate = vi.fn();
const refreshContext = vi.fn(async () => {});
let tasks: unknown[];
let selected = base;
vi.mock("@devbox/product-shell/api", async (original) => ({
  ...(await original<object>()),
  currentDescription: vi.fn(async () => ({ context: selected })),
}));
beforeEach(() => {
  selected = base;
  tasks = [task];
  vi.clearAllMocks();
  call.mockImplementation(async (_component, method, args) => {
    if (method === "list") return tasks;
    if (method === "terminal_sessions") return [{ id: "terminal-1", state: "active", context: agent }];
    if (method === "snapshot") return registry;
    if (method === "inspect_agent_worktree")
      return tasks.some((task) => (task as { state: string }).state === "running") ? "present" : "absent";
    if (method === "select_project") {
      selected = args?.context as typeof base;
      return { context: selected };
    }
    if (method === "repo_status") return { branch: { current: "main", detached: false } };
    if (method === "repo_merge") return { merged: true, head: "a".repeat(40), conflicts: [] };
    if (method === "finish") return { ...task, state: args?.outcome };
    return null;
  });
});
afterEach(cleanup);
function view(context = base) {
  return render(
    <AgentHub
      description={{ ...fixtureDescription("workspace"), context }}
      registry={registry}
      navigate={navigate}
      refreshContext={refreshContext}
    />,
  );
}
it("shows only WSL guidance for a Windows project", async () => {
  const { container } = render(
    <AgentHub
      description={{ ...fixtureDescription("workspace"), context: { ...base, target: { kind: "windows" } } }}
      registry={registry}
      navigate={navigate}
      refreshContext={refreshContext}
    />,
  );
  expect(screen.getByText("에이전트 작업은 WSL 프로젝트에서 사용할 수 있습니다.")).toBeTruthy();
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(await findA11yViolations(container)).toEqual([]);
});
it("keeps a planned task when source review is required and links to Source", async () => {
  tasks = [];
  const original = call.getMockImplementation()!;
  call.mockImplementation(async (...args) => {
    if (args[1] === "plan") {
      const planned = { ...task, state: "planned", worktreeId: null, terminalId: null };
      tasks = [planned];
      return planned;
    }
    if (args[1] === "preview_worktree")
      throw Object.assign(new Error("source_review_required"), { code: "source_review_required" });
    return original(...args);
  });
  const { container } = view();
  fireEvent.change(screen.getByRole("textbox", { name: "제목" }), { target: { value: "Fix login" } });
  fireEvent.click(screen.getByRole("button", { name: "작업 만들기" }));
  await screen.findByText("기본 작업 폴더의 Git 설정을 먼저 확인해 주세요.");
  expect(await screen.findByRole("button", { name: "다시 시도" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "소스 화면 열기" }));
  expect(navigate).toHaveBeenCalledWith("source");
  expect(await findA11yViolations(container)).toEqual([]);
});
it("confirms the base branch before merging and separates cleanup", async () => {
  const { container } = view(agent);
  selected = agent;
  fireEvent.click(await screen.findByRole("button", { name: "병합" }));
  await screen.findByText("'agent/fix-login'을 기본 작업 폴더의 현재 branch 'main'에 병합합니다.");
  expect(call.mock.calls.some(([, method]) => method === "repo_merge")).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "병합 확인" }));
  await screen.findByText("병합했습니다. 작업 폴더를 정리할까요?");
  expect(call.mock.calls.some(([, method]) => method === "remove_agent_worktree")).toBe(false);
  expect(await findA11yViolations(container)).toEqual([]);
  fireEvent.click(screen.getByRole("button", { name: "정리" }));
  await waitFor(() => expect(call.mock.calls.some(([, method]) => method === "finish")).toBe(true));
  const methods = call.mock.calls.map(([, method]) => method);
  expect(methods.indexOf("stop_terminal")).toBeLessThan(methods.indexOf("remove_agent_worktree"));
  expect(methods.indexOf("remove_agent_worktree")).toBeLessThan(methods.indexOf("remove"));
  expect(call.mock.calls.find(([, method]) => method === "remove_agent_worktree")?.[2]).toMatchObject({
    request: { force: false },
  });
});
it("requires confirmation before discarding uncommitted work", async () => {
  const { container } = view();
  fireEvent.click(await screen.findByRole("button", { name: "버리기" }));
  expect(screen.getByText("작업 폴더와 branch를 지웁니다. 커밋하지 않은 변경도 사라집니다.")).toBeTruthy();
  expect(call.mock.calls.some(([, method]) => method === "remove_agent_worktree")).toBe(false);
  expect(await findA11yViolations(container)).toEqual([]);
  await waitFor(() =>
    expect((screen.getByRole("button", { name: "버리기 확인" }) as HTMLButtonElement).disabled).toBe(false),
  );
  fireEvent.click(screen.getByRole("button", { name: "버리기 확인" }));
  await waitFor(() => expect(call.mock.calls.some(([, method]) => method === "finish")).toBe(true));
  expect(call.mock.calls.find(([, method]) => method === "remove_agent_worktree")?.[2]).toMatchObject({
    request: { force: true },
  });
});

it("shows merge conflicts without removing a worktree", async () => {
  const original = call.getMockImplementation()!;
  call.mockImplementation(async (...args) =>
    args[1] === "repo_merge" ? { merged: false, head: "a".repeat(40), conflicts: ["fixture.txt"] } : original(...args),
  );
  const { container } = view();
  fireEvent.click(await screen.findByRole("button", { name: "병합" }));
  fireEvent.click(await screen.findByRole("button", { name: "병합 확인" }));
  await screen.findByText("충돌 파일 1개");
  expect(screen.getByText("fixture.txt")).toBeTruthy();
  expect(call.mock.calls.some(([, method]) => method === "remove_agent_worktree")).toBe(false);
  expect(await findA11yViolations(container)).toEqual([]);
});
it("finishes a cleanup retry after Git removal succeeded but its reply was lost", async () => {
  const original = call.getMockImplementation()!;
  call.mockImplementation(async (...args) => (args[1] === "inspect_agent_worktree" ? "absent" : original(...args)));
  view();
  fireEvent.click(await screen.findByRole("button", { name: "버리기" }));
  await waitFor(() =>
    expect((screen.getByRole("button", { name: "버리기 확인" }) as HTMLButtonElement).disabled).toBe(false),
  );
  fireEvent.click(screen.getByRole("button", { name: "버리기 확인" }));
  await waitFor(() => expect(call.mock.calls.some(([, method]) => method === "finish")).toBe(true));
  expect(call.mock.calls.some(([, method]) => method === "remove_agent_worktree")).toBe(false);
});

it("reviews a ready task without starting its tool", async () => {
  tasks = [{ ...task, state: "ready", terminalId: null }];
  view();
  fireEvent.click(await screen.findByRole("button", { name: "변경 검토" }));
  await waitFor(() => expect(navigate).toHaveBeenCalledWith("source"));
  expect(call.mock.calls.some(([, method]) => method === "open_agent_terminal")).toBe(false);
});
