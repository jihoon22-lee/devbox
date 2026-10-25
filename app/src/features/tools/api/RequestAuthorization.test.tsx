import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { RpcClient } from "../../../rpc/client";
import type { StreamHandlers } from "../../../rpc/client-streams";
import type { SendEvent } from "../../../rpc/gen/rpc";
import { ApiWorkspace } from "./ApiWorkspace";
import { ApiBackendProvider } from "./backend";
import { emptyDraft } from "./model/draft";
import { rpcBackend } from "./rpcBackend";
import { approveHttpAuthorization } from "./sendAuthorization";
import { useApiTabs } from "./store";
import { FakeBackend } from "./test-backend";

const external = vi.hoisted(() => ({ open: vi.fn().mockResolvedValue(undefined) }));
vi.mock("../../../shell/external", () => ({ openExternal: external.open }));
const personal = { kind: "personal" } as const;
function setup() {
  const streams: { handlers: StreamHandlers; end: ReturnType<typeof vi.fn> }[] = [];
  const client = {
    openStream: vi.fn((_method, _params, handlers: StreamHandlers) => {
      const stream = { handlers, end: vi.fn() };
      streams.push(stream);
      return Promise.resolve(stream);
    }),
  } as unknown as RpcClient;
  const backend = new FakeBackend();
  backend.send = vi.fn(rpcBackend(client).send);
  useApiTabs.getState().openNew(personal);
  const tab = useApiTabs.getState().tabs[0]!;
  useApiTabs.getState().setDraftById(tab.id, { ...emptyDraft(), url: "https://resource.invalid/" });
  const view = render(
    <ApiBackendProvider backend={backend}>
      <ApiWorkspace />
    </ApiBackendProvider>,
  );
  const emit = (event: SendEvent, index = 0) =>
    act(() => streams[index]!.handlers.onData(new TextEncoder().encode(`${JSON.stringify(event)}\n`), vi.fn()));
  return { streams, backend, tab, emit, ...view };
}
afterEach(() => {
  cleanup();
  useApiTabs.getState().reset();
  external.open.mockReset().mockResolvedValue(undefined);
});
it("real HTTP adapter forwards authorization to explicit request action without auto opening", async () => {
  const s = setup();
  fireEvent.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(s.streams).toHaveLength(1));
  s.emit({ type: "started", historyId: "owned-history", sentAtMs: 1 });
  s.emit({ type: "authorize", url: "https://auth.invalid/authorize?state=owned" });
  expect(external.open).not.toHaveBeenCalled();
  fireEvent.click(await screen.findByRole("button", { name: "브라우저에서 승인" }));
  expect(external.open).toHaveBeenCalledWith("https://auth.invalid/authorize?state=owned");
  await finish(s, 0);
  expect(await screen.findByText("200 OK")).toBeInTheDocument();
  expect(useApiTabs.getState().tabs[0]?.result?.response.historyId).toBe("owned-history");
  expect(screen.queryByRole("button", { name: "브라우저에서 승인" })).not.toBeInTheDocument();
});

async function start(s: ReturnType<typeof setup>, index = 0) {
  fireEvent.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(s.streams).toHaveLength(index + 1));
  s.emit({ type: "started", historyId: `history-${index}`, sentAtMs: 100 + index }, index);
}
function authorize(s: ReturnType<typeof setup>, index = 0) {
  s.emit({ type: "authorize", url: `https://auth.invalid/?state=${index}` }, index);
}
async function finish(s: ReturnType<typeof setup>, index: number) {
  s.emit(
    { type: "head", status: 200, reason: "OK", httpVersion: "HTTP/1.1", headers: [], elapsedMs: 1, redirects: [] },
    index,
  );
  s.emit({ type: "end", sizeBytes: 0, elapsedMs: 2, binary: false, historyTruncated: false }, index);
  await act(async () => s.streams[index]!.handlers.onEnd());
}
it("cancel and resend reject late events browser failures and wrong-owner actions", async () => {
  const s = setup();
  await start(s);
  authorize(s);
  const stale = useApiTabs.getState().tabs[0]!.authorization!;
  let reject!: (e: Error) => void;
  external.open.mockImplementationOnce(
    () =>
      new Promise((_yes, no) => {
        reject = no;
      }),
  );
  fireEvent.click(screen.getByRole("button", { name: "브라우저에서 승인" }));
  fireEvent.click(screen.getByRole("button", { name: "취소" }));
  await start(s, 1);
  authorize(s, 1);
  s.emit({ type: "authorize", url: "https://old.invalid/" }, 0);
  await act(async () => reject(new Error("private browser diagnostic")));
  await act(async () => approveHttpAuthorization(s.tab.id, stale));
  const current = useApiTabs.getState().tabs[0]!.authorization;
  expect(current).toMatchObject({ historyId: "history-1", url: "https://auth.invalid/?state=1" });
  expect(current?.error).toBeUndefined();
  expect(external.open).toHaveBeenCalledTimes(1);
  expect(screen.queryByText(/private browser/)).not.toBeInTheDocument();
  await finish(s, 0);
  expect(useApiTabs.getState().tabs[0]!.sending).toBe(true);
  await finish(s, 1);
  expect(useApiTabs.getState().tabs[0]!.result?.response.historyId).toBe("history-1");
});
it("switching tabs cancels pending authorization and returning does not revive it", async () => {
  const s = setup();
  await start(s);
  authorize(s);
  act(() => useApiTabs.getState().openNew(personal));
  expect(s.streams[0]!.end).toHaveBeenCalled();
  act(() => useApiTabs.getState().select(s.tab.key));
  authorize(s);
  expect(screen.queryByRole("button", { name: "브라우저에서 승인" })).not.toBeInTheDocument();
  expect(external.open).not.toHaveBeenCalled();
});
it("a background ordinary send completes but a later authorization loses its tab ownership", async () => {
  const s = setup();
  await start(s);
  act(() => useApiTabs.getState().openNew(personal));
  expect(s.streams[0]!.end).not.toHaveBeenCalled();
  await finish(s, 0);
  expect(useApiTabs.getState().tabs.find((t) => t.id === s.tab.id)?.result?.response.historyId).toBe("history-0");
  act(() => useApiTabs.getState().select(s.tab.key));
  await start(s, 1);
  act(() => useApiTabs.getState().openNew(personal));
  act(() => useApiTabs.getState().select(s.tab.key));
  authorize(s, 1);
  expect(s.streams[1]!.end).toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: "브라우저에서 승인" })).not.toBeInTheDocument();
});
it("changing the request root invalidates pending action and callback ownership", async () => {
  const s = setup();
  await start(s);
  authorize(s);
  act(() => useApiTabs.getState().updateById(s.tab.id, { root: { kind: "project", projectId: "other" } }));
  authorize(s);
  await finish(s, 0);
  expect(useApiTabs.getState().tabs[0]!.authorization).toBeUndefined();
  expect(useApiTabs.getState().tabs[0]!.result).toBeUndefined();
  expect(external.open).not.toHaveBeenCalled();
});
it("current browser failure is explicit fixed text and can be retried without completing the HTTP request", async () => {
  const s = setup();
  await start(s);
  authorize(s);
  external.open.mockRejectedValueOnce(new Error("sensitive diagnostic"));
  fireEvent.click(screen.getByRole("button", { name: "브라우저에서 승인" }));
  expect(await screen.findByText("브라우저를 열지 못했습니다. 다시 시도해 주세요.")).toBeInTheDocument();
  expect(screen.queryByText(/sensitive diagnostic/)).not.toBeInTheDocument();
  expect(useApiTabs.getState().tabs[0]!.result).toBeUndefined();
  fireEvent.click(screen.getByRole("button", { name: "브라우저에서 승인" }));
  await waitFor(() => expect(external.open).toHaveBeenCalledTimes(2));
  expect(useApiTabs.getState().tabs[0]!.sending).toBe(true);
  await finish(s, 0);
});
it.each(["javascript:alert(1)", "file:///tmp/private", "https://user:password@auth.invalid/", "not a URL"])(
  "refuses invalid authorization URL %s before browser open",
  async (url) => {
    const s = setup();
    await start(s);
    s.emit({ type: "authorize", url });
    expect(
      await screen.findByText("승인 주소를 확인할 수 없습니다. 요청의 인증 설정을 확인해 주세요."),
    ).toBeInTheDocument();
    expect(external.open).not.toHaveBeenCalled();
    expect(s.streams[0]!.end).toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "브라우저에서 승인" })).not.toBeInTheDocument();
  },
);
it("authorization without same-stream Started fails and terminal events cannot resurrect an action", async () => {
  const s = setup();
  fireEvent.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(s.streams).toHaveLength(1));
  authorize(s);
  await waitFor(() => expect(s.streams[0]!.end).toHaveBeenCalled());
  expect(external.open).not.toHaveBeenCalled();
  await start(s, 1);
  await finish(s, 1);
  authorize(s, 1);
  expect(screen.queryByRole("button", { name: "브라우저에서 승인" })).not.toBeInTheDocument();
  expect(useApiTabs.getState().tabs[0]!.result?.response.historyId).toBe("history-1");
});

it("response head retires authorization before the history write and stream FIN", async () => {
  const s = setup();
  await start(s);
  authorize(s);
  const action = useApiTabs.getState().tabs[0]!.authorization!;
  s.emit({
    type: "head",
    status: 200,
    reason: "OK",
    httpVersion: "HTTP/1.1",
    headers: [],
    elapsedMs: 1,
    redirects: [],
  });
  expect(screen.queryByRole("button", { name: "브라우저에서 승인" })).not.toBeInTheDocument();
  expect(useApiTabs.getState().tabs[0]!.sending).toBe(true);
  await act(async () => approveHttpAuthorization(s.tab.id, action));
  expect(external.open).not.toHaveBeenCalled();
  await finish(s, 0);
});
