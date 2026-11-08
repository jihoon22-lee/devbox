import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ErrorCode } from "../../rpc/client";
import type { Run } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { RpcProvider } from "../../rpc/RpcProvider";
import { useToasts } from "../../ui";
import { ToastHost } from "../../ui/Toast";
import { trustPreview } from "../projects/trust-fixtures";
import { RunStopUndo } from "./RunStopUndo";
import { ProjectRuns } from "./RunsPage";
import { RunWire, runFixture, serviceDefinition } from "./test-wire";

function show(wire: RunWire) {
  return render(
    <RpcProvider client={wire.client}>
      <RunStopUndo />
      <ProjectRuns projectId="p" />
      <ToastHost />
    </RpcProvider>,
  );
}
afterEach(() => {
  for (const toast of useToasts.getState().items) useToasts.getState().dismiss(toast.id);
});
test("open_run_detail_follows_restart and retains a selected run beyond the latest page", async () => {
  const wire = new RunWire();
  show(wire);
  expect(await screen.findByText("PID 111")).toBeVisible();
  wire.selected = { ...runFixture, pid: 777, restarts: 2, attemptId: "attempt-2" };
  wire.runs = [{ ...runFixture, id: "newer-run" }];
  // Click pins this invocation before new history arrives.
  await userEvent.click(screen.getByRole("button", { name: "web 로그" }));
  act(() => wire.changed("2"));
  expect(await screen.findByText("PID 777")).toBeVisible();
  expect(screen.getByText("재시작 2회")).toBeVisible();
  expect(wire.request("runtime.get", -1).params).toEqual({ runId: "run-1" });
});
test("start_with_stale_trust_shows_preview_then_retries_once with visible controls", async () => {
  const wire = new RunWire();
  wire.runs = [];
  wire.defs = [{ ...serviceDefinition, source: "projectFile" }];
  wire.trust = "approval_required";
  show(wire);
  const start = await screen.findByRole("button", { name: "web 시작" });
  expect(screen.getByText("설정이 바뀌어 다음 시작에 재승인 필요")).toBeVisible();
  await userEvent.click(start);
  await waitFor(() => expect(wire.requests("runtime.start")).toHaveLength(1));
  act(() => wire.reject("runtime.start", "projects.trust_required"));
  await waitFor(() => expect(wire.requests("projects.trust_preview")).toHaveLength(1));
  expect(wire.request("projects.trust_preview").params).toMatchObject({ operation: "run", id: "p" });
  act(() =>
    wire.reply(
      "projects.trust_preview",
      trustPreview("p", { runDefinitions: [{ ...serviceDefinition.def, command: "echo \u202eowned" }] }),
    ),
  );
  expect(await screen.findByText("echo \\u202Eowned")).toBeVisible();
  await userEvent.click(screen.getByRole("button", { name: "명령 실행 승인" }));
  await waitFor(() => expect(wire.requests("projects.trust_approve")).toHaveLength(1));
  act(() => wire.reply("projects.trust_approve", { rev: "2" }));
  await waitFor(() => expect(wire.requests("runtime.start")).toHaveLength(2));
  act(() => wire.reject("runtime.start", "projects.trust_preview_stale"));
  await screen.findByRole("alert");
  expect(wire.requests("runtime.start")).toHaveLength(2);
  expect(wire.requests("projects.trust_preview")).toHaveLength(1);
});
test.each([
  ["runtime.windows_copy_busy", "dotnet build-server shutdown"],
  ["windows.config_invalid", "설정 → 앱 진단 → [다시 설정]을 눌러 주세요."],
])("a failed start shows the title and the next action of its code (%s)", async (code, action) => {
  const wire = new RunWire();
  wire.runs = [];
  show(wire);
  await userEvent.click(await screen.findByRole("button", { name: "web 시작" }));
  await waitFor(() => expect(wire.requests("runtime.start")).toHaveLength(1));
  act(() => wire.reject("runtime.start", code));
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent(messageFor(code as ErrorCode).title);
  expect(alert).toHaveTextContent(action);
});
test("this PC start avoids approval and service stop undo restarts the same run with current trust", async () => {
  const wire = new RunWire();
  show(wire);
  await screen.findByText("PID 111");
  await userEvent.click(screen.getByRole("button", { name: "web 중지" }));
  await waitFor(() => expect(wire.requests("runtime.stop")).toHaveLength(1));
  wire.selected = { ...runFixture, state: "stopped", pid: null };
  act(() => wire.reply("runtime.stop", wire.selected));
  await userEvent.click(await screen.findByRole("button", { name: "되돌리기" }));
  await waitFor(() => expect(wire.requests("runtime.restart")).toHaveLength(1));
  expect(wire.request("runtime.restart").params).toEqual({ runId: "run-1" });
  expect(wire.requests("projects.trust_preview")).toHaveLength(0);
  act(() => wire.reject("runtime.restart", "projects.trust_required"));
  await waitFor(() => expect(wire.requests("projects.trust_preview")).toHaveLength(1));
  act(() => wire.reply("projects.trust_preview", trustPreview("p", { runDefinitions: [serviceDefinition.def] })));
  await userEvent.click(await screen.findByRole("button", { name: "명령 실행 승인" }));
  await waitFor(() => expect(wire.requests("projects.trust_approve")).toHaveLength(1));
  act(() => wire.reply("projects.trust_approve", { rev: "2" }));
  await waitFor(() => expect(wire.requests("runtime.restart")).toHaveLength(2));
  act(() => wire.reply("runtime.restart", runFixture));
  expect(await screen.findByText("web 다시 시작됨")).toBeVisible();
});
test("a service that undo cannot restart shows what to do next in its toast", async () => {
  const wire = new RunWire();
  show(wire);
  await screen.findByText("PID 111");
  await userEvent.click(screen.getByRole("button", { name: "web 중지" }));
  await waitFor(() => expect(wire.requests("runtime.stop")).toHaveLength(1));
  wire.selected = { ...runFixture, state: "stopped", pid: null };
  act(() => wire.reply("runtime.stop", wire.selected));
  await userEvent.click(await screen.findByRole("button", { name: "되돌리기" }));
  await waitFor(() => expect(wire.requests("runtime.restart")).toHaveLength(1));
  act(() => wire.reject("runtime.restart", "runtime.windows_copy_busy"));
  expect(await screen.findByText(/dotnet build-server shutdown/)).toBeVisible();
});
test("a service that undo failed to restart unexpectedly shows the diagnostic ID in its toast", async () => {
  const wire = new RunWire();
  show(wire);
  await screen.findByText("PID 111");
  await userEvent.click(screen.getByRole("button", { name: "web 중지" }));
  await waitFor(() => expect(wire.requests("runtime.stop")).toHaveLength(1));
  wire.selected = { ...runFixture, state: "stopped", pid: null };
  act(() => wire.reply("runtime.stop", wire.selected));
  await userEvent.click(await screen.findByRole("button", { name: "되돌리기" }));
  await waitFor(() => expect(wire.requests("runtime.restart")).toHaveLength(1));
  act(() => wire.reject("runtime.restart", "internal", "diag-5"));
  expect(await screen.findByText("진단 ID를 복사해 두면 원인을 찾을 수 있습니다. · 진단 diag-5")).toBeVisible();
});
test("container stop uses server undo only and restart never creates an undo", async () => {
  const wire = new RunWire();
  show(wire);
  await userEvent.click(await screen.findByRole("button", { name: "owned-container 재시작" }));
  await waitFor(() => expect(wire.requests("runtime.container_action")).toHaveLength(1));
  act(() => wire.reply("runtime.container_action", { id: wire.containers[0]!.id, action: "restart", undo: null }));
  await screen.findByText("owned-container 재시작됨");
  expect(screen.queryByRole("button", { name: "되돌리기" })).not.toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole("button", { name: "owned-container 중지" })).toBeEnabled());
  await userEvent.click(screen.getByRole("button", { name: "owned-container 중지" }));
  await waitFor(() => expect(wire.requests("runtime.container_action")).toHaveLength(2));
  const undo = { id: wire.containers[0]!.id, action: "start" };
  act(() => wire.reply("runtime.container_action", { id: undo.id, action: "stop", undo }));
  await waitFor(() => expect(screen.getByRole("button", { name: "owned-container 재시작" })).toBeEnabled());
  await userEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  await waitFor(() => expect(wire.requests("runtime.container_action")).toHaveLength(3));
  expect(wire.request("runtime.container_action", -1).params).toEqual(undo);
  act(() => wire.reply("runtime.container_action", { id: undo.id, action: "start", undo: null }));
});
test("Windows details expose target directory and notice without reporting wrapper CPU", async () => {
  const wire = new RunWire();
  wire.selected = {
    ...runFixture,
    windows: { directory: "C:\\dev\\owned", notice: "Windows에서 실행 · 자식 프로세스 종료는 보장하지 않음" },
  };
  show(wire);
  expect(await screen.findByText(/실행 폴더: C:/)).toBeVisible();
  expect(screen.getByText("Windows 자원 측정 없음")).toBeVisible();
  expect(wire.requests("observe.processes")).toHaveLength(0);
  expect(screen.getByRole("button", { name: "포트 8123 브라우저로 열기" })).toBeVisible();
});

test("this PC only start ignores stale project trust without showing extra approval", async () => {
  const wire = new RunWire();
  wire.runs = [];
  wire.trust = "approval_required";
  show(wire);
  await userEvent.click(await screen.findByRole("button", { name: "web 시작" }));
  await waitFor(() => expect(wire.requests("runtime.start")).toHaveLength(1));
  act(() => wire.reply("runtime.start", runFixture));
  expect(await screen.findByText("PID 111")).toBeVisible();
  expect(wire.requests("projects.trust_preview")).toHaveLength(0);
  expect(screen.queryByText("설정이 바뀌어 다음 시작에 재승인 필요")).not.toBeInTheDocument();
});
test("stop undo survives leaving the runs page and stopped containers cannot be stopped again", async () => {
  const wire = new RunWire();
  wire.containers[0]!.state = "exited";
  const view = show(wire);
  expect(await screen.findByRole("button", { name: "owned-container 중지" })).toBeDisabled();
  await userEvent.click(await screen.findByRole("button", { name: "web 중지" }));
  await waitFor(() => expect(wire.requests("runtime.stop")).toHaveLength(1));
  act(() => wire.reply("runtime.stop", { ...runFixture, state: "stopped" }));
  await screen.findByRole("button", { name: "되돌리기" });
  view.rerender(
    <RpcProvider client={wire.client}>
      <RunStopUndo />
      <ToastHost />
    </RpcProvider>,
  );
  await userEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  await waitFor(() => expect(wire.requests("runtime.restart")).toHaveLength(1));
  expect(wire.request("runtime.restart").params).toEqual({ runId: "run-1" });
  act(() => wire.reply("runtime.restart", runFixture));
  expect(await screen.findByText("web 다시 시작됨")).toBeVisible();
});
test("정의 없는 살아 있는 서비스는 [로그]·[중지]만 보인다", async () => {
  const wire = new RunWire();
  const orphan: Run = { ...runFixture, id: "orphan-run", defId: "api", pid: 222 };
  wire.runs = [{ ...runFixture }, orphan];
  show(wire);
  const table = await screen.findByRole("table", { name: "정의 없는 실행 목록" });
  const labels = within(table)
    .getAllByRole("button")
    .map((button) => button.getAttribute("aria-label"));
  expect(labels).toEqual(["api 로그", "api 중지"]);
  for (const action of ["시작", "재시작", "수정"])
    expect(screen.queryByRole("button", { name: `api ${action}` })).not.toBeInTheDocument();
  const log = within(table).getByRole("button", { name: "api 로그" });
  expect(log).toHaveAttribute("aria-pressed", "false");
  wire.selected = orphan;
  await userEvent.click(log);
  expect(log).toHaveAttribute("aria-pressed", "true");
  await waitFor(() => expect(wire.request("runtime.get", -1).params).toEqual({ runId: "orphan-run" }));
  expect(await screen.findByRole("heading", { name: "api" })).toBeVisible();
  await userEvent.click(within(table).getByRole("button", { name: "api 중지" }));
  await waitFor(() => expect(wire.requests("runtime.stop")).toHaveLength(1));
  expect(wire.request("runtime.stop").params).toEqual({ runId: "orphan-run" });
  wire.runs = [{ ...runFixture }];
  act(() => wire.reply("runtime.stop", { ...orphan, state: "stopped", pid: null }));
  expect(await screen.findByText("api 중지됨")).toBeVisible();
  // Without its definition the run cannot start again, so the stop has no undo.
  expect(screen.queryByRole("button", { name: "되돌리기" })).not.toBeInTheDocument();
  await waitFor(() => expect(screen.queryByRole("table", { name: "정의 없는 실행 목록" })).not.toBeInTheDocument());
});
