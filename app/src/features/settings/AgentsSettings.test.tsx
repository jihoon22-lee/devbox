import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { useCurrentProject } from "../projects/store";
import { ProjectWire } from "../projects/test-wire";
import { AgentsSettings } from "./AgentsSettings";

const info = {
  version: "1.0.0-dev",
  instance: "test",
  pid: 1,
  startedMs: 1,
  dataDir: "/fixture/data",
  configPath: "/fixture/xdg/devbox/config.toml",
};
async function setup(project?: string) {
  const wire = new ProjectWire();
  useCurrentProject.getState().set(project);
  const view = render(
    <RpcProvider client={wire.client}>
      <AgentsSettings />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("system.doctor")).toHaveLength(1));
  await act(async () => {
    wire.reply("agents.profiles", {
      items: [{ id: "claude", label: "Claude Code", command: "claude", hooks: "claude", waitingPatterns: [] }],
    });
    wire.reply("system.info", info);
    wire.reply("system.doctor", {
      checks: [
        { id: "tool.claude", status: "warn", message: "PATH에서 찾지 못했습니다", fix: null },
        { id: "hook.binary", status: "ok", message: "상태 알림 실행 파일 사용 가능", fix: null },
        { id: "systemd", status: "ok", message: "systemd가 켜져 있습니다", fix: null },
      ],
    });
  });
  await screen.findByText("PATH에서 찾지 못했습니다");
  return { wire, view };
}
test("agent settings filter tool checks, copy actual config path and refresh only once", async () => {
  const copy = vi.fn(async () => {});
  Object.defineProperty(navigator, "clipboard", { value: { writeText: copy }, configurable: true });
  const { wire } = await setup();
  expect(screen.getByText("PATH에서 찾지 못했습니다")).toBeInTheDocument();
  expect(screen.getByText("Claude Code ·")).toBeInTheDocument();
  expect(screen.queryByText("systemd가 켜져 있습니다")).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "설정 파일 위치 복사" }));
  expect(copy).toHaveBeenCalledWith(info.configPath);
  await userEvent.dblClick(screen.getByRole("button", { name: "셸 환경 다시 읽기" }));
  await waitFor(() => expect(wire.requests("system.refresh_env")).toHaveLength(1));
  expect(wire.request("system.refresh_env").params).toEqual({});
  expect(wire.request("system.refresh_env").clientRequestId).toEqual(expect.any(String));
  await act(async () => wire.reply("system.refresh_env", { restarting: true }));
  expect(await screen.findByText(/데몬을 다시 시작합니다/)).toBeInTheDocument();
});
test("foreground refresh explains that current environment is unchanged", async () => {
  const { wire } = await setup();
  await userEvent.click(screen.getByRole("button", { name: "셸 환경 다시 읽기" }));
  await waitFor(() => expect(wire.requests("system.refresh_env")).toHaveLength(1));
  await act(async () => wire.reply("system.refresh_env", { restarting: false }));
  expect(await screen.findByText(/다음 시작부터 적용됩니다/)).toBeInTheDocument();
});
test("selected cleanup always confirms concrete targets, focuses cancel and blocks duplicate work", async () => {
  const { wire } = await setup("owned");
  await waitFor(() => expect(wire.requests("agents.cleanup_candidates")).toHaveLength(1));
  await act(async () =>
    wire.reply("agents.cleanup_candidates", { worktrees: ["/fixture/owned"], branches: ["devbox/closed"] }),
  );
  await userEvent.click(await screen.findByRole("checkbox", { name: "/fixture/owned" }));
  await userEvent.click(screen.getByRole("button", { name: "선택 항목 정리" }));
  const dialog = screen.getByRole("dialog");
  expect(within(dialog).getByText("/fixture/owned")).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "취소" })).toHaveFocus();
  expect(wire.requests("agents.cleanup")).toHaveLength(0);
  await userEvent.dblClick(within(dialog).getByRole("button", { name: "정리" }));
  await waitFor(() => expect(wire.requests("agents.cleanup")).toHaveLength(1));
  expect(wire.request("agents.cleanup").params).toEqual({
    projectId: "owned",
    worktrees: ["/fixture/owned"],
    branches: [],
  });
  await userEvent.keyboard("{Escape}");
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  await act(async () => wire.reply("agents.cleanup", { done: true }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(await screen.findByText("선택 항목을 정리했습니다")).toBeInTheDocument();
});
test("changing project removes old cleanup confirmation and selections", async () => {
  const { wire } = await setup("first");
  await waitFor(() => expect(wire.requests("agents.cleanup_candidates")).toHaveLength(1));
  await act(async () => wire.reply("agents.cleanup_candidates", { worktrees: ["/fixture/first"], branches: [] }));
  await userEvent.click(await screen.findByRole("checkbox", { name: "/fixture/first" }));
  await userEvent.click(screen.getByRole("button", { name: "선택 항목 정리" }));
  await act(async () => useCurrentProject.getState().set("second"));
  expect(screen.queryByRole("dialog")).toBeNull();
  await waitFor(() => expect(wire.requests("agents.cleanup_candidates")).toHaveLength(2));
  expect(wire.request("agents.cleanup_candidates", -1).params).toEqual({ projectId: "second" });
  await act(async () => wire.reply("agents.cleanup_candidates", { worktrees: ["/fixture/second"], branches: [] }));
  fireEvent.click(screen.getByRole("button", { name: "선택 항목 정리" }));
  expect(wire.requests("agents.cleanup")).toHaveLength(0);
});

test("refresh typed errors provide a retryable Korean diagnostic without claiming restart", async () => {
  const { wire } = await setup();
  await userEvent.click(screen.getByRole("button", { name: "셸 환경 다시 읽기" }));
  await waitFor(() => expect(wire.requests("system.refresh_env")).toHaveLength(1));
  await act(async () =>
    wire.deliver({
      type: "response",
      id: wire.request("system.refresh_env").id,
      error: { code: "system.env_refresh_failed" },
    }),
  );
  expect(await screen.findByText(/셸 환경을 다시 읽지 못했습니다/)).toBeInTheDocument();
  expect(screen.queryByText(/데몬을 다시 시작합니다/)).toBeNull();
  expect(screen.getByRole("button", { name: "셸 환경 다시 읽기" })).toBeEnabled();
});
