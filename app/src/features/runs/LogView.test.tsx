import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { streamFrame } from "../../rpc/frame";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectWire } from "../projects/test-wire";
import { LogView } from "./LogView";

const source = { kind: "run" as const, runId: "r" };
beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(320);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(900);
  Object.defineProperty(HTMLElement.prototype, "scrollTo", { configurable: true, value: vi.fn() });
});
function deliver(wire: ProjectWire, text: string, stream = "stderr", id = 7) {
  const bytes = new TextEncoder().encode(
    `${JSON.stringify({ src: source, ts: "2026-10-20T00:00:00Z", stream, text })}\n`,
  );
  for (const cb of wire.frames) cb(streamFrame(id, 0, bytes));
}
async function setup() {
  const wire = new ProjectWire();
  const open = vi.fn();
  const view = render(
    <RpcProvider client={wire.client}>
      <LogView sources={[source]} onOpenLocation={open} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(1));
  await act(async () => wire.reply("logs.open", { streamId: 7 }));
  return { wire, view, open };
}

test("opening a finished run includes output written before the log view mounted", async () => {
  const { wire } = await setup();
  // The real file source starts tail at EOF; only an initial offset includes
  // the short task's already-persisted output.
  if ((wire.request("logs.open").params as { from: unknown }).from !== "tail") {
    act(() => deliver(wire, "completed before mount", "stdout"));
  }
  expect(await screen.findByText("completed before mount")).toBeVisible();
  expect(wire.request("logs.open").params).toMatchObject({ from: { offset: 0 } });
});

test("file-only log views retain live-tail behavior", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <LogView sources={[{ kind: "file", path: "/owned/file.log" }]} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(1));
  expect(wire.request("logs.open").params).toMatchObject({ from: "tail" });
  await act(async () => wire.reply("logs.open", { streamId: 7 }));
});

test("uses actual log stream frames, credits bytes, renders safe ANSI and file locations, and closes on unmount", async () => {
  const { wire, view, open } = await setup();
  act(() => deliver(wire, "\x1b[31mfailed\x1b[0m src/app.ts:12:3"));
  expect(await screen.findByText("failed")).toHaveStyle({ color: "#cd3131" });
  fireEvent.click(screen.getByRole("button", { name: "src/app.ts:12:3" }));
  expect(open).toHaveBeenCalledWith({ path: "src/app.ts", line: 12, column: 3, source });
  expect(wire.sent.some((m) => m.type === "streamCredit" && m.stream === 7)).toBe(true);
  // Close the consumer while the shared connection remains available. Unmounting
  // RpcProvider first stops the entire transport and disposes all streams there.
  view.rerender(
    <RpcProvider client={wire.client}>
      <div />
    </RpcProvider>,
  );
  expect(wire.client.state).toBe("ready");
  expect(wire.sent.filter((m) => m.type === "streamClose" && m.stream === 7)).toHaveLength(1);
  act(() => deliver(wire, "late data"));
  expect(screen.queryByText("late data")).not.toBeInTheDocument();
  view.unmount();
  expect(wire.frames.size).toBe(0);
});

test("scrolling up pauses follow and the bottom action resumes it", async () => {
  const { wire } = await setup();
  act(() => deliver(wire, "first"));
  const list = screen.getByRole("log");
  Object.defineProperties(list, {
    scrollHeight: { configurable: true, value: 1000 },
    clientHeight: { configurable: true, value: 200 },
    scrollTop: { configurable: true, writable: true, value: 100 },
  });
  fireEvent.scroll(list);
  expect(screen.getByRole("button", { name: "맨 아래로" })).toBeInTheDocument();
  act(() => deliver(wire, "second"));
  expect(screen.getByRole("button", { name: "맨 아래로" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "맨 아래로" }));
  expect(screen.queryByRole("button", { name: "맨 아래로" })).not.toBeInTheDocument();
});

test("applies server filters, merges sources and reports search failure", async () => {
  const { wire } = await setup();
  fireEvent.change(screen.getByLabelText("로그 검색"), { target: { value: "failure" } });
  fireEvent.click(screen.getByLabelText("정규식"));
  fireEvent.change(screen.getByLabelText("스트림"), { target: { value: "stderr" } });
  fireEvent.click(screen.getByRole("button", { name: "필터 적용" }));
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(2));
  expect(wire.request("logs.open", -1).params).toEqual({
    sources: [source],
    from: { offset: 0 },
    filter: { regex: "failure", streams: ["stderr"] },
  });
  await act(async () => wire.reply("logs.open", { streamId: 8 }));
  fireEvent.click(screen.getByRole("button", { name: "지난 로그 검색" }));
  await waitFor(() => expect(wire.requests("logs.search")).toHaveLength(1));
  expect(wire.request("logs.search").params).toEqual({ sources: [source], query: "failure", regex: true, limit: 200 });
  await act(async () => wire.fail("logs.search"));
  expect(await screen.findByRole("alert")).toHaveTextContent("로그를 검색하지 못했습니다");
  fireEvent.click(screen.getByRole("button", { name: "소스 추가" }));
  fireEvent.change(screen.getByLabelText("소스 종류"), { target: { value: "file" } });
  fireEvent.change(screen.getByLabelText("소스 경로 또는 ID"), { target: { value: "/var/log/test.log" } });
  fireEvent.click(screen.getByRole("button", { name: "추가" }));
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(3));
  expect(wire.request("logs.open", -1).params).toMatchObject({
    sources: [source, { kind: "file", path: "/var/log/test.log" }],
  });
});

test("disposes streams whose open response arrives after unmount", async () => {
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <LogView sources={[source]} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(1));
  view.rerender(
    <RpcProvider client={wire.client}>
      <div />
    </RpcProvider>,
  );
  await act(async () => wire.reply("logs.open", { streamId: 9 }));
  expect(wire.sent.some((m) => m.type === "streamClose" && m.stream === 9)).toBe(true);
});

test("closes malformed streams and offers a retry without rendering unvalidated data", async () => {
  const { wire } = await setup();
  act(() => {
    for (const cb of wire.frames) cb(streamFrame(7, 0, new TextEncoder().encode('{"text":"not a log"}\n')));
  });
  expect(await screen.findByRole("alert")).toHaveTextContent("로그를 읽지 못했습니다");
  expect(wire.sent.some((m) => m.type === "streamClose" && m.stream === 7)).toBe(true);
  expect(screen.queryByText("not a log")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "다시 연결" }));
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(2));
});

test("search stream filtering is local and changing sources ignores a pending search response", async () => {
  const { wire } = await setup();
  fireEvent.change(screen.getByLabelText("로그 검색"), { target: { value: "match" } });
  fireEvent.change(screen.getByLabelText("스트림"), { target: { value: "stderr" } });
  fireEvent.click(screen.getByRole("button", { name: "지난 로그 검색" }));
  await waitFor(() => expect(wire.requests("logs.search")).toHaveLength(1));
  const line = { src: source, ts: "2026-10-20T00:00:00Z", stream: "stdout", text: "hidden match" };
  await act(async () =>
    wire.reply("logs.search", {
      items: [line, { ...line, stream: "stderr", text: "visible match" }],
      nextCursor: null,
    }),
  );
  expect(await screen.findByText("visible match")).toBeInTheDocument();
  expect(screen.queryByText("hidden match")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "지난 로그 검색" }));
  await waitFor(() => expect(wire.requests("logs.search")).toHaveLength(2));
  fireEvent.click(screen.getByRole("button", { name: "run:r 소스 제거" }));
  await act(async () =>
    wire.reply("logs.search", { items: [{ ...line, stream: "stderr", text: "stale match" }], nextCursor: null }),
  );
  expect(screen.queryByText("stale match")).not.toBeInTheDocument();
  expect(screen.getByText("로그 소스를 추가해 주세요.")).toBeInTheDocument();
});
