import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it } from "vitest";
import type { CollectionRoot, GrpcCatalog } from "../../../../rpc/gen/rpc";
import { ApiBackendProvider } from "../backend";
import { useApiTabs } from "../store";
import { FakeBackend } from "../test-backend";
import { ENV_REVIEW_BLOCK } from "../useSend";
import { GrpcPanel } from "./GrpcPanel";

afterEach(() => {
  cleanup();
  useApiTabs.getState().reset();
});

const project: CollectionRoot = { kind: "project", projectId: "p1" };
const roots = [
  { id: "p1", label: "프로젝트 컬렉션", root: project },
  { id: "personal", label: "개인 컬렉션", root: { kind: "personal" } as CollectionRoot },
];
const method = (name: string) => ({
  name,
  fullName: `demo.Echo/${name}`,
  clientStreaming: false,
  serverStreaming: false,
  inputDefault: `{"name":"${name.toLowerCase()}"}`,
});
const catalog: GrpcCatalog = {
  source: "reflection",
  services: [{ name: "demo.Echo", methods: [method("Alpha"), method("Beta")] }],
};

function setup(backend = new FakeBackend()) {
  backend.grpcCatalog = catalog;
  render(
    <ApiBackendProvider backend={backend}>
      <GrpcPanel roots={roots} />
    </ApiBackendProvider>,
  );
  return backend;
}

async function described(
  user: ReturnType<typeof userEvent.setup>,
  backend: FakeBackend,
  target = "http://127.0.0.1:9",
) {
  await user.type(screen.getByLabelText("gRPC 대상"), target);
  await user.click(screen.getByRole("button", { name: "서비스 불러오기" }));
  await waitFor(() => expect(backend.grpcDescribe).toHaveBeenCalledOnce());
  await screen.findByRole("option", { name: /demo\.Echo\/Alpha/ });
}
const json = () => screen.getByLabelText("요청 JSON") as HTMLTextAreaElement;

it("late_grpc_response_shows_its_origin", async () => {
  const user = userEvent.setup();
  const backend = setup();
  await described(user, backend);
  await user.click(screen.getByRole("option", { name: /demo\.Echo\/Alpha/ }));
  await user.click(screen.getByRole("button", { name: "호출" }));
  await waitFor(() => expect(backend.openGrpc).toHaveBeenCalledOnce());
  expect(backend.openGrpc.mock.calls[0]![0]).toMatchObject({ method: "demo.Echo/Alpha", json: '{"name":"alpha"}' });
  // A를 기다리는 동안 B를 고른다.
  await user.click(screen.getByRole("option", { name: /demo\.Echo\/Beta/ }));
  const a = backend.grpcCalls[0]!;
  act(() => {
    a.emit({ type: "headers", metadata: [] });
    a.emit({ type: "message", json: '{"name":"late-a"}' });
  });
  const result = screen.getByRole("region", { name: "호출 결과" });
  expect(within(result).getByRole("heading")).toHaveTextContent("demo.Echo/Alpha의 결과");
  expect(result).toHaveTextContent("late-a");
  const log = screen.getByRole("list", { name: "호출 기록" });
  expect(within(log).getAllByRole("listitem")[0]).toHaveTextContent("demo.Echo/Alpha");
  // [취소]는 A 호출 스트림을 닫는다.
  await user.click(screen.getByRole("button", { name: "취소" }));
  expect(a.closed).toBe(true);
  expect(within(log).getAllByRole("listitem")[0]).toHaveTextContent("취소");
});

it("grpc_filter_switch_keeps_the_dirty_draft", async () => {
  const user = userEvent.setup();
  const backend = setup();
  await described(user, backend);
  await user.click(screen.getByRole("option", { name: /demo\.Echo\/Alpha/ }));
  await user.clear(json());
  await user.type(json(), '{{"name":"edited"}');
  await user.type(screen.getByLabelText("메서드 필터"), "bet");
  expect(screen.getByRole("option", { name: /demo\.Echo\/Beta/ })).toHaveAttribute("aria-selected", "true");
  expect(json().value).toBe('{"name":"beta"}');
  await user.clear(screen.getByLabelText("메서드 필터"));
  const alpha = screen.getByRole("option", { name: /demo\.Echo\/Alpha/ });
  expect(alpha).toHaveTextContent("수정됨");
  await user.click(alpha);
  expect(json().value).toBe('{"name":"edited"}');
});

const fileText = JSON.stringify({
  version: 1,
  kind: "grpc",
  name: "svc",
  grpc: {
    target: "http://127.0.0.1:9",
    metadata: [],
    tls: { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null },
    protoFiles: ["/home/u/echo.proto"],
    method: "demo.Echo/Beta",
    json: '{"name":"from-file"}',
  },
});

it("project_grpc_file_requires_review_before_describe_or_call", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  const digest = backend.seed(project, "svc.grpc.json", fileText);
  setup(backend);
  await user.click(await screen.findByRole("button", { name: "svc.grpc.json" }));
  await screen.findByText(/검토 필요/);
  expect(screen.getByRole("button", { name: "서비스 불러오기" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "호출" })).toBeDisabled();
  expect(backend.grpcDescribe).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "검토하고 사용" }));
  await waitFor(() => expect(backend.review).toHaveBeenCalledWith(project, "svc.grpc.json", digest));
  await waitFor(() => expect(screen.getByRole("button", { name: "서비스 불러오기" })).toBeEnabled());
  await user.click(screen.getByRole("button", { name: "서비스 불러오기" }));
  await waitFor(() => expect(backend.grpcDescribe).toHaveBeenCalledOnce());
  expect(backend.grpcDescribe.mock.calls[0]![0]).toMatchObject({ protoFiles: ["/home/u/echo.proto"] });
});

it("grpc_review_is_refused_when_the_file_changed_meanwhile", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  backend.seed(project, "svc.grpc.json", fileText);
  backend.review.mockImplementation(async () => {
    backend.seed(project, "svc.grpc.json", fileText.replace("from-file", "changed"));
  });
  setup(backend);
  await user.click(await screen.findByRole("button", { name: "svc.grpc.json" }));
  await user.click(await screen.findByRole("button", { name: "검토하고 사용" }));
  await screen.findByText(/검토하는 동안 파일이 바뀌었습니다/);
  expect(screen.getByRole("button", { name: "서비스 불러오기" })).toBeDisabled();
});

it("unreviewed_project_env_is_not_used_for_grpc", async () => {
  const user = userEvent.setup();
  const backend = setup();
  await described(user, backend);
  useApiTabs
    .getState()
    .setEnvironment({ name: "dev", variables: [{ name: "host", value: "127.0.0.1:9" }], unreviewed: true });
  const target = screen.getByLabelText("gRPC 대상");
  await user.clear(target);
  await user.type(target, "http://{{{{host}}");
  await user.click(screen.getByRole("option", { name: /demo\.Echo\/Alpha/ }));
  await user.click(screen.getByRole("button", { name: "호출" }));
  await screen.findByText(ENV_REVIEW_BLOCK);
  await user.click(screen.getByRole("button", { name: "서비스 불러오기" }));
  expect(backend.openGrpc).not.toHaveBeenCalled();
  expect(backend.grpcDescribe).toHaveBeenCalledOnce();
});

it("leaving_the_grpc_panel_closes_running_calls", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  backend.grpcCatalog = catalog;
  const view = render(
    <ApiBackendProvider backend={backend}>
      <GrpcPanel roots={roots} />
    </ApiBackendProvider>,
  );
  await described(user, backend);
  await user.click(screen.getByRole("button", { name: "호출" }));
  await waitFor(() => expect(backend.openGrpc).toHaveBeenCalledOnce());
  view.unmount();
  expect(backend.grpcCalls[0]!.closed).toBe(true);
});
