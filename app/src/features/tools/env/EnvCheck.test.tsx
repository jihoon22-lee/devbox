import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { EnvCheck } from "./EnvCheck";

const mock = vi.hoisted(() => ({
  invoke: vi.fn(),
  popout: vi.fn(),
  create: vi.fn(),
  bundle: vi.fn(),
  refresh: vi.fn(),
  plan: {
    kind: "wsl",
    title: "tmux 설치",
    command: "sudo apt install tmux",
    shellCommand: "bash -lc 'sudo apt install tmux; exec bash -l'",
  } as Record<string, string>,
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mock.invoke }));
vi.mock("../../terminal/popout", () => ({ openPopout: mock.popout }));
vi.mock("../../../rpc/query", () => ({
  useRpcQuery: (method: string) => ({
    isPending: false,
    error: null,
    refetch: vi.fn(),
    data:
      method === "system.doctor"
        ? {
            checks: [
              {
                id: mock.plan.kind === "windows" ? "tool.docker" : "tool.tmux",
                status: "warn",
                message: "도구 설치 필요",
                fix: null,
              },
            ],
          }
        : method === "system.info"
          ? {
              version: "fixture",
              instance: "accept",
              pid: 123,
              dataDir: "/fixture/data",
              configPath: "/fixture/config.toml",
            }
          : method === "tools.diagnostics"
            ? { logDir: "/fixture/logs" }
            : method === "tools.install_command"
              ? mock.plan
              : undefined,
  }),
  useRpcMutation: (method: string) => ({
    isPending: false,
    mutateAsync:
      method === "terminal.create" ? mock.create : method === "tools.support_bundle" ? mock.bundle : mock.refresh,
  }),
}));
beforeEach(() => {
  mock.invoke.mockReset().mockResolvedValue(undefined);
  mock.popout.mockReset().mockResolvedValue(undefined);
  mock.create.mockReset().mockResolvedValue({ id: "install-session", projectId: null });
  mock.bundle
    .mockReset()
    .mockResolvedValue({ path: "/fixture/data/support/bundle.zip", sizeBytes: 123, logsOmitted: "secrets_locked" });
  mock.refresh.mockReset().mockResolvedValue({ restarting: false });
  mock.plan = {
    kind: "wsl",
    title: "tmux 설치",
    command: "sudo apt install tmux",
    shellCommand: "bash -lc 'sudo apt install tmux; exec bash -l'",
  };
  Object.assign(window, { __TAURI_INTERNALS__: {} });
});
afterEach(() => Reflect.deleteProperty(window, "__TAURI_INTERNALS__"));
function setup() {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <EnvCheck />
    </QueryClientProvider>,
  );
}
it("install_requires_confirmation_before_creating_a_terminal", async () => {
  setup();
  expect(screen.getByText("/fixture/logs")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "tmux 설치" }));
  const dialog = screen.getByRole("dialog");
  expect(within(dialog).getByText(mock.plan.shellCommand!)).toBeInTheDocument();
  expect(dialog).toHaveTextContent("새 터미널에서 바로 실행");
  expect(mock.create).not.toHaveBeenCalled();
  await userEvent.click(within(dialog).getByRole("button", { name: "취소" }));
  expect(mock.create).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: "tmux 설치" }));
  await userEvent.click(screen.getByRole("button", { name: "실행" }));
  await waitFor(() => expect(mock.create).toHaveBeenCalledOnce());
  expect(mock.create).toHaveBeenCalledWith({
    projectId: null,
    cwd: null,
    title: "tmux 설치",
    command: mock.plan.shellCommand,
    attachZellij: null,
  });
  await waitFor(() => expect(mock.popout).toHaveBeenCalledWith("terminal", "install-session"));
});
it("windows_install_confirms_fixed_command_and_sends_only_the_package_enum", async () => {
  mock.plan = { kind: "windows", package: "dockerDesktop" };
  setup();
  await userEvent.click(screen.getByRole("button", { name: "Docker Desktop 설치" }));
  expect(screen.getByRole("dialog")).toHaveTextContent(
    "winget.exe install --id Docker.DockerDesktop -e --source winget",
  );
  expect(mock.invoke).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: "실행" }));
  expect(mock.invoke).toHaveBeenCalledWith("install_windows_tool", { package: "dockerDesktop" });
  expect(mock.create).not.toHaveBeenCalled();
});
it("native_redeploy_and_cleanup_always_require_separate_confirmation", async () => {
  setup();
  for (const [label, command] of [
    ["다시 설정", "redeploy_wsl"],
    ["WSL 쪽 정리", "cleanup_wsl"],
  ] as const) {
    await userEvent.click(screen.getByRole("button", { name: label }));
    expect(mock.invoke).not.toHaveBeenCalledWith(command);
    await userEvent.click(screen.getByRole("button", { name: "취소" }));
    expect(mock.invoke).not.toHaveBeenCalledWith(command);
    await userEvent.click(screen.getByRole("button", { name: label }));
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "실행" }));
    await waitFor(() => expect(mock.invoke).toHaveBeenCalledWith(command));
  }
});
it("support_bundle_shows_log_omission_and_opens_its_parent_folder", async () => {
  setup();
  await userEvent.click(screen.getByRole("button", { name: "지원 번들" }));
  expect(await screen.findByText("비밀 저장소가 잠겨 있어 로그를 넣지 않았습니다")).toBeInTheDocument();
  expect(screen.getByText("/fixture/data/support/bundle.zip")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "폴더 열기" }));
  expect(mock.invoke).toHaveBeenCalledWith("open_external", { kind: "explorer", path: "/fixture/data/support" });
});
it("a double confirmation cannot launch two immediate installations", async () => {
  let finish!: (value: { id: string }) => void;
  mock.create.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  setup();
  await userEvent.click(screen.getByRole("button", { name: "tmux 설치" }));
  const execute = screen.getByRole("button", { name: "실행" });
  fireEvent.click(execute);
  fireEvent.click(execute);
  expect(mock.create).toHaveBeenCalledOnce();
  await act(async () => finish({ id: "install-session" }));
});
