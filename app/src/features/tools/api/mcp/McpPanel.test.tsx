import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import type { CollectionRoot, McpInput } from "../../../../rpc/gen/rpc";
import { ApiBackendProvider } from "../backend";
import { useApiTabs } from "../store";
import { FakeBackend } from "../test-backend";
import { ENV_REVIEW_BLOCK } from "../useSend";
import { McpPanel } from "./McpPanel";

const mocks = vi.hoisted(() => ({ open: vi.fn(async (_url: string) => {}) }));
vi.mock("../../../../shell/external", () => ({ openExternal: mocks.open }));

afterEach(() => {
  cleanup();
  useApiTabs.getState().reset();
  mocks.open.mockClear();
});

const project: CollectionRoot = { kind: "project", projectId: "p1" };
const roots = [
  { id: "p1", label: "프로젝트 컬렉션", root: project },
  { id: "personal", label: "개인 컬렉션", root: { kind: "personal" } as CollectionRoot },
];
const profile = JSON.stringify({
  version: 1,
  kind: "mcp",
  name: "srv",
  transport: {
    kind: "stdio",
    command: "python3",
    args: ["/p/server.py", "a b"],
    cwd: "/p",
    env: [{ name: "TOKEN", value: "{{secret:TOKEN}}" }],
  },
});
const shown = "python3 /p/server.py 'a b'";

function setup(backend = new FakeBackend()) {
  const view = render(
    <ApiBackendProvider backend={backend}>
      <McpPanel roots={roots} />
    </ApiBackendProvider>,
  );
  return { backend, view };
}
const connect = () => screen.getByRole("button", { name: "연결 열기" });

it("project_stdio_profile_requires_review", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  const digest = backend.seed(project, "srv.mcp.json", profile);
  setup(backend);
  await user.click(await screen.findByRole("button", { name: "srv.mcp.json" }));
  const banner = await screen.findByText(/검토 필요/);
  expect(screen.getByLabelText("실행할 명령")).toHaveTextContent(shown);
  // 검토 화면은 env 값을 적힌 그대로(비밀 참조는 참조로) 보이고 작업 폴더도 보인다.
  expect(screen.getByLabelText("실행 환경 변수")).toHaveTextContent("TOKEN={{secret:TOKEN}}");
  expect(screen.getByLabelText("실행 작업 폴더")).toHaveTextContent("/p");
  expect(banner.closest("[role]") ?? document.body).toBeTruthy();
  expect(connect()).toBeDisabled();
  await user.click(screen.getByRole("button", { name: "검토하고 사용" }));
  await waitFor(() => expect(backend.review).toHaveBeenCalledWith(project, "srv.mcp.json", digest));
  await waitFor(() => expect(connect()).toBeEnabled());
  await user.click(connect());
  // 검토한 파일도 실행 전에 확인 대화상자를 거친다.
  const dialog = await screen.findByRole("dialog");
  expect(dialog).toHaveTextContent(shown);
  // 확인 대화상자도 env 이름=값(풀지 않은 비밀 참조)과 작업 폴더를 그대로 보인다.
  expect(dialog).toHaveTextContent("TOKEN={{secret:TOKEN}}");
  expect(dialog).toHaveTextContent("작업 폴더: /p");
  expect(backend.openMcp).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole("button", { name: "실행" }));
  await waitFor(() => expect(backend.openMcp).toHaveBeenCalledOnce());
  expect(backend.openMcp.mock.calls[0]![0]).toMatchObject({
    source: { kind: "file", root: project, path: "srv.mcp.json", digest },
    transport: {
      kind: "stdio",
      command: "python3",
      args: ["/p/server.py", "a b"],
      cwd: "/p",
      env: [{ name: "TOKEN", value: "{{secret:TOKEN}}" }],
    },
  });
});

it("mcp_review_is_refused_when_the_file_changed_meanwhile", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  backend.seed(project, "srv.mcp.json", profile);
  backend.review.mockImplementation(async () => {
    backend.seed(project, "srv.mcp.json", profile.replace("a b", "rm -rf"));
  });
  setup(backend);
  await user.click(await screen.findByRole("button", { name: "srv.mcp.json" }));
  await user.click(await screen.findByRole("button", { name: "검토하고 사용" }));
  await screen.findByText(/검토하는 동안 파일이 바뀌었습니다/);
  expect(screen.getByLabelText("실행할 명령")).toHaveTextContent("python3 /p/server.py 'rm -rf'");
  expect(connect()).toBeDisabled();
  expect(backend.openMcp).not.toHaveBeenCalled();
});

it("draft_stdio_connect_needs_confirmation", async () => {
  const user = userEvent.setup();
  const { backend } = setup();
  await user.type(screen.getByLabelText("명령"), "node");
  await user.type(screen.getByLabelText("인자"), "server.js{Enter}--port 1");
  // 초안은 작업 폴더(등록 프로젝트 안)를 적어야 연결할 수 있다.
  expect(connect()).toBeDisabled();
  await user.type(screen.getByLabelText("작업 폴더"), "/home/u/proj");
  await user.click(connect());
  const dialog = await screen.findByRole("dialog");
  expect(dialog).toHaveTextContent("node server.js '--port 1'");
  expect(dialog).toHaveTextContent("작업 폴더: /home/u/proj");
  await user.click(within(dialog).getByRole("button", { name: "취소" }));
  expect(backend.openMcp).not.toHaveBeenCalled();
  await user.click(connect());
  await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "실행" }));
  await waitFor(() => expect(backend.openMcp).toHaveBeenCalledOnce());
  expect(backend.openMcp.mock.calls[0]![0]).toMatchObject({
    source: { kind: "draft" },
    transport: { kind: "stdio", command: "node", args: ["server.js", "--port 1"], cwd: "/home/u/proj", env: [] },
  });
});

async function httpDraft(user: ReturnType<typeof userEvent.setup>, url = "http://127.0.0.1:9/mcp") {
  await user.selectOptions(screen.getByLabelText("연결 방식"), "http");
  await user.type(screen.getByLabelText("MCP 주소"), url);
}

it("unreviewed_project_env_is_not_used_for_mcp", async () => {
  const user = userEvent.setup();
  const { backend } = setup();
  useApiTabs
    .getState()
    .setEnvironment({ name: "dev", variables: [{ name: "host", value: "127.0.0.1:9" }], unreviewed: true });
  await httpDraft(user, "http://{{{{host}}/mcp");
  await user.click(connect());
  await screen.findByText(ENV_REVIEW_BLOCK);
  expect(backend.openMcp).not.toHaveBeenCalled();
});

it("tool_calls_wait_for_ready_and_send_arguments_verbatim", async () => {
  const user = userEvent.setup();
  const { backend, view } = setup();
  await httpDraft(user);
  await user.click(connect());
  await waitFor(() => expect(backend.openMcp).toHaveBeenCalledOnce());
  expect(backend.openMcp.mock.calls[0]![0]).toMatchObject({
    source: { kind: "draft" },
    transport: { kind: "http", url: "http://127.0.0.1:9/mcp", headers: [] },
  });
  const session = backend.mcp[0]!;
  expect(screen.queryByRole("button", { name: "도구 호출" })).toBeNull();
  expect(session.sent).toEqual([]);
  act(() =>
    session.emit({
      type: "ready",
      serverName: "fixture",
      serverVersion: "1",
      protocolVersion: "2025-06-18",
      capabilitiesJson: "{}",
    }),
  );
  await screen.findByText(/fixture/);
  await waitFor(() => expect(session.sent).toHaveLength(1));
  const list = session.sent[0] as Extract<McpInput, { type: "list" }>;
  expect(list).toMatchObject({ type: "list", what: "tools", cursor: null });
  act(() => session.emit({ type: "result", id: list.id, json: JSON.stringify({ tools: [{ name: "echo" }] }) }));
  await user.click(await screen.findByRole("option", { name: /echo/ }));
  const args = screen.getByLabelText("도구 인자 JSON");
  await user.clear(args);
  const text = '{{"text":  "hi",  "n":1}';
  await user.type(args, text);
  await user.click(screen.getByRole("button", { name: "도구 호출" }));
  const call = session.sent[1] as Extract<McpInput, { type: "call" }>;
  expect(call).toMatchObject({ type: "call", name: "echo", argumentsJson: '{"text":  "hi",  "n":1}' });
  act(() => session.emit({ type: "result", id: call.id, json: '{"content":[{"type":"text","text":"echoed"}]}' }));
  expect(await screen.findByRole("region", { name: "MCP 결과" })).toHaveTextContent("echoed");
  await user.click(screen.getByRole("button", { name: "연결 닫기" }));
  expect(session.closed).toBe(true);
  view.unmount();
});

it("closing_the_panel_closes_the_session", async () => {
  const user = userEvent.setup();
  const { backend, view } = setup();
  await httpDraft(user);
  await user.click(connect());
  await waitFor(() => expect(backend.openMcp).toHaveBeenCalledOnce());
  view.unmount();
  expect(backend.mcp[0]!.closed).toBe(true);
});

it("authorize_event_opens_the_browser_on_request", async () => {
  const user = userEvent.setup();
  const { backend } = setup();
  await httpDraft(user);
  await user.click(connect());
  await waitFor(() => expect(backend.openMcp).toHaveBeenCalledOnce());
  act(() => backend.mcp[0]!.emit({ type: "authorize", url: "https://as.test/authorize?state=s" }));
  expect(mocks.open).not.toHaveBeenCalled();
  await user.click(await screen.findByRole("button", { name: "브라우저에서 로그인" }));
  expect(mocks.open).toHaveBeenCalledWith("https://as.test/authorize?state=s");
});

it("empty_cwd_means_the_project_root_and_personal_files_need_one", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  const noCwd = profile.replace('"cwd":"/p"', '"cwd":null');
  backend.seed(project, "srv.mcp.json", noCwd);
  backend.seed({ kind: "personal" }, "mine.mcp.json", noCwd);
  setup(backend);
  await user.click(await screen.findByRole("button", { name: "srv.mcp.json" }));
  expect(await screen.findByLabelText("실행 작업 폴더")).toHaveTextContent("프로젝트 루트(프로젝트 컬렉션)");
  await user.click(screen.getByRole("button", { name: "mine.mcp.json" }));
  await waitFor(() => expect(screen.getByLabelText("실행 작업 폴더")).toHaveTextContent("없음"));
  expect(screen.getByText(/작업 폴더\(cwd\)를 적어야/)).toBeInTheDocument();
  expect(connect()).toBeDisabled();
  expect(backend.openMcp).not.toHaveBeenCalled();
});
