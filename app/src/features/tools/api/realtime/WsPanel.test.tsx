import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ApiBackendProvider, SendFailure } from "../backend";
import { useApiTabs } from "../store";
import { FakeBackend } from "../test-backend";
import { ENV_REVIEW_BLOCK } from "../useSend";
import { WsPanel } from "./WsPanel";

// 가상 목록이 jsdom에서도 행을 그리도록 크기를 준다(`ui/VirtualList.test.tsx`와 같은 방식).
beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(240);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(800);
  Object.defineProperty(HTMLElement.prototype, "scrollTo", { configurable: true, value: vi.fn() });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useApiTabs.getState().reset();
});
function setup(backend = new FakeBackend()) {
  const view = render(
    <ApiBackendProvider backend={backend}>
      <WsPanel />
    </ApiBackendProvider>,
  );
  return { backend, ...view };
}
const status = () => screen.getByRole("status", { name: "연결 상태" });

it("ws_panel_opens_sends_shows_messages_and_closes", async () => {
  const user = userEvent.setup();
  const { backend } = setup();
  await user.type(screen.getByLabelText("URL"), "ws://127.0.0.1:9/echo");
  await user.click(screen.getByText("헤더·TLS"));
  await user.click(screen.getByRole("button", { name: "헤더 추가" }));
  await user.type(screen.getByLabelText("헤더 1 이름"), "Authorization");
  await user.type(screen.getByLabelText("헤더 1 값"), "Bearer {{{{secret:GH}}");
  await user.type(screen.getByLabelText("하위 프로토콜"), "chat");
  await user.click(screen.getByRole("button", { name: "연결 열기" }));
  await waitFor(() => expect(backend.openWs).toHaveBeenCalledOnce());
  expect(backend.openWs.mock.calls[0]![0]).toMatchObject({
    url: "ws://127.0.0.1:9/echo",
    headers: [{ row: 0, name: "Authorization", value: "Bearer {{secret:GH}}" }],
    subprotocols: ["chat"],
    env: { variables: [] },
  });
  const session = backend.realtime[0]!;
  expect(status()).toHaveTextContent("연결 중");
  act(() => session.emit({ type: "open", protocol: "chat" }));
  expect(status()).toHaveTextContent("열림 · chat");

  await user.type(screen.getByLabelText("보낼 메시지"), "ping");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  expect(session.sent).toEqual([{ type: "text", data: "ping" }]);
  act(() => {
    session.emit({ type: "message", direction: "sent", kind: "text", data: "ping", dataBase64: null, atMs: 1 });
    session.emit({
      type: "message",
      direction: "received",
      kind: "text",
      data: "pong:ping",
      dataBase64: null,
      atMs: 2,
    });
    session.emit({ type: "dropped", count: 3 });
  });
  const list = screen.getByRole("log", { name: "WebSocket 메시지" });
  expect(within(list).getByText(/받음 pong:ping/)).toBeInTheDocument();
  expect(screen.getByRole("status", { name: "WebSocket 메시지 개수" })).toHaveTextContent("2개 · 버림 3개");

  // 잘못된 JSON은 보내지 않는다.
  await user.selectOptions(screen.getByLabelText("보낼 형식"), "json");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("JSON 형식이 올바르지 않습니다.");
  expect(session.sent).toHaveLength(1);

  await user.type(screen.getByLabelText("닫기 코드"), "4000");
  await user.click(screen.getByRole("button", { name: "닫기 보내기" }));
  expect(session.sent.at(-1)).toEqual({ type: "close", code: 4000, reason: null });
  await user.click(screen.getByRole("button", { name: "연결 닫기" }));
  expect(session.closed).toBe(true);
  expect(status()).toHaveTextContent("닫힘");
});

it("leaving_the_panel_closes_the_connection", async () => {
  const user = userEvent.setup();
  const { backend, unmount } = setup();
  await user.type(screen.getByLabelText("URL"), "ws://127.0.0.1:9/");
  await user.click(screen.getByRole("button", { name: "연결 열기" }));
  await waitFor(() => expect(backend.realtime).toHaveLength(1));
  unmount();
  expect(backend.realtime[0]!.closed).toBe(true);
});

it("reopening_closes_the_previous_connection_and_ignores_its_late_events", async () => {
  const user = userEvent.setup();
  const { backend } = setup();
  await user.type(screen.getByLabelText("URL"), "ws://127.0.0.1:9/");
  await user.click(screen.getByRole("button", { name: "연결 열기" }));
  await waitFor(() => expect(backend.realtime).toHaveLength(1));
  const first = backend.realtime[0]!;
  act(() => first.emit({ type: "open", protocol: null }));
  await user.click(screen.getByRole("button", { name: "연결 닫기" }));
  await user.click(screen.getByRole("button", { name: "연결 열기" }));
  await waitFor(() => expect(backend.realtime).toHaveLength(2));
  expect(first.closed).toBe(true);
  act(() =>
    first.emit({ type: "message", direction: "received", kind: "text", data: "late", dataBase64: null, atMs: 0 }),
  );
  expect(screen.queryByText(/late/)).toBeNull();
  expect(status()).toHaveTextContent("연결 중");
});

it("ws_panel_does_not_use_an_unreviewed_project_environment", async () => {
  const user = userEvent.setup();
  const { backend } = setup();
  act(() =>
    useApiTabs.getState().setEnvironment({
      name: "dev",
      variables: [{ name: "wsUrl", value: "ws://127.0.0.1:9/{{secret:GH}}" }],
      unreviewed: true,
    }),
  );
  await user.type(screen.getByLabelText("URL"), "{{{{wsUrl}}");
  await user.click(screen.getByRole("button", { name: "연결 열기" }));
  expect(await screen.findByText(ENV_REVIEW_BLOCK)).toBeInTheDocument();
  expect(backend.openWs).not.toHaveBeenCalled();
});

it("rejected_open_shows_the_row_error", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  backend.rejectOpen = new SendFailure("api.invalid_header", { index: 0, field: "name", reason: "reserved" });
  setup(backend);
  await user.type(screen.getByLabelText("URL"), "ws://127.0.0.1:9/");
  await user.click(screen.getByRole("button", { name: "연결 열기" }));
  expect(await screen.findByText(/헤더 1행 이름이 올바르지 않습니다/)).toBeInTheDocument();
  expect(status()).toHaveTextContent("실패");
});

it("ws_network_failure_shows_its_cause_and_ignores_unknown_kinds", async () => {
  const user = userEvent.setup();
  const { backend } = setup();
  await user.type(screen.getByLabelText("URL"), "wss://127.0.0.1:9/");
  await user.click(screen.getByRole("button", { name: "연결 열기" }));
  await waitFor(() => expect(backend.realtime).toHaveLength(1));
  act(() => {
    backend.realtime[0]!.emit({ type: "failed", error: { code: "api.network", detail: { kind: "tls" } } });
    backend.realtime[0]!.end();
  });
  expect(await screen.findByText(/요청 전송에 실패했습니다/)).toHaveTextContent(
    "원인: TLS 인증서를 확인하지 못했습니다",
  );
  // 모르는 kind 문자열은 화면에 옮기지 않는다(원인 문구는 정해 둔 것만).
  await user.click(screen.getByRole("button", { name: "연결 열기" }));
  await waitFor(() => expect(backend.realtime).toHaveLength(2));
  act(() => {
    backend.realtime[1]!.emit({
      type: "failed",
      error: { code: "api.network", detail: { kind: "unknown-kind-marker" } },
    });
    backend.realtime[1]!.end();
  });
  const banner = await screen.findByText(/요청 전송에 실패했습니다/);
  await waitFor(() => expect(banner).not.toHaveTextContent("TLS"));
  expect(document.body).not.toHaveTextContent("unknown-kind-marker");
  expect(banner).not.toHaveTextContent("원인:");
});
