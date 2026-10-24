import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ApiBackendProvider } from "../backend";
import { useApiTabs } from "../store";
import { FakeBackend } from "../test-backend";
import { ENV_REVIEW_BLOCK } from "../useSend";
import { SsePanel } from "./SsePanel";

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
      <SsePanel />
    </ApiBackendProvider>,
  );
  return { backend, ...view };
}
const status = () => screen.getByRole("status", { name: "연결 상태" });

it("sse_panel_shows_events_reconnects_and_closes", async () => {
  const user = userEvent.setup();
  const { backend, unmount } = setup();
  act(() =>
    useApiTabs.getState().setEnvironment({ name: "dev", variables: [{ name: "base", value: "http://127.0.0.1:9" }] }),
  );
  await user.type(screen.getByLabelText("URL"), "{{{{base}}/events");
  await user.type(screen.getByLabelText("Last-Event-ID"), "3");
  await user.click(screen.getByRole("button", { name: "연결 열기" }));
  await waitFor(() => expect(backend.openSse).toHaveBeenCalledOnce());
  expect(backend.openSse.mock.calls[0]![0]).toMatchObject({
    url: "{{base}}/events",
    lastEventId: "3",
    env: { variables: [{ name: "base", value: "http://127.0.0.1:9", origin: "environment" }] },
  });
  const session = backend.realtime[0]!;
  act(() => {
    session.emit({ type: "open", protocol: null });
    session.emit({ type: "sseEvent", id: "7", event: "tick", data: "a\nb", retryMs: 50, atMs: 1 });
    session.emit({ type: "reconnecting", attempt: 1, lastEventId: "7" });
  });
  expect(status()).toHaveTextContent("다시 연결 중");
  const list = screen.getByRole("log", { name: "SSE 이벤트" });
  expect(within(list).getByText(/tick #7 a/)).toBeInTheDocument();
  expect(within(list).getByText(/다시 연결하는 중 \(1\/5\)/)).toBeInTheDocument();
  // 다시 연결 중에도 닫을 수 있다.
  await user.click(screen.getByRole("button", { name: "연결 닫기" }));
  expect(session.closed).toBe(true);
  expect(status()).toHaveTextContent("닫힘");
  unmount();
});

it("sse_panel_does_not_use_an_unreviewed_project_environment", async () => {
  const user = userEvent.setup();
  const { backend } = setup();
  act(() =>
    useApiTabs.getState().setEnvironment({
      name: "dev",
      variables: [{ name: "token", value: "{{secret:GH}}" }],
      unreviewed: true,
    }),
  );
  await user.type(screen.getByLabelText("URL"), "http://127.0.0.1:9/events?t={{{{token}}");
  await user.click(screen.getByRole("button", { name: "연결 열기" }));
  expect(await screen.findByText(ENV_REVIEW_BLOCK)).toBeInTheDocument();
  expect(backend.openSse).not.toHaveBeenCalled();
});

it("sse_failure_event_is_shown_with_its_message", async () => {
  const user = userEvent.setup();
  const { backend } = setup();
  await user.type(screen.getByLabelText("URL"), "http://127.0.0.1:9/events");
  await user.click(screen.getByRole("button", { name: "연결 열기" }));
  await waitFor(() => expect(backend.realtime).toHaveLength(1));
  act(() => {
    backend.realtime[0]!.emit({
      type: "failed",
      error: { code: "api.stream_rejected", detail: { status: 500, reason: "status" } },
    });
    backend.realtime[0]!.end();
  });
  expect(await screen.findByText(/서버가 연결을 받지 않았습니다\(HTTP 500\)/)).toBeInTheDocument();
  expect(status()).toHaveTextContent("실패");
});

it("sse_network_failure_shows_its_cause_without_values", async () => {
  const user = userEvent.setup();
  const { backend } = setup();
  await user.type(screen.getByLabelText("URL"), "http://127.0.0.1:9/events");
  await user.click(screen.getByRole("button", { name: "연결 열기" }));
  await waitFor(() => expect(backend.realtime).toHaveLength(1));
  act(() => {
    backend.realtime[0]!.emit({ type: "failed", error: { code: "api.network", detail: { kind: "reconnect_limit" } } });
    backend.realtime[0]!.end();
  });
  const banner = await screen.findByText(/요청 전송에 실패했습니다/);
  expect(banner).toHaveTextContent("원인: 다시 연결 횟수 한도에 닿았습니다");
});
