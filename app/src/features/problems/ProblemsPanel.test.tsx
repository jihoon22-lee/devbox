import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { openFileLocation } from "../../shell/fileLocation";
import { useCurrentProject } from "../projects/store";
import { ProjectWire } from "../projects/test-wire";
import { ProblemsPanel } from "./ProblemsPanel";

vi.mock("../../shell/fileLocation", () => ({ openFileLocation: vi.fn().mockResolvedValue(undefined) }));
const problem = { file: "src/x.rs", line: 10, col: 4, severity: "error", message: "컴파일 실패", source: "run:run-1" };
beforeEach(() => {
  useCurrentProject.setState({ id: "p" });
  vi.clearAllMocks();
});
async function setup() {
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <ProblemsPanel />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("observe.problems")).toHaveLength(1));
  return { wire, view };
}
test("renders a source file link and opens the original run location in the chosen external editor", async () => {
  const { wire } = await setup();
  await act(async () =>
    wire.reply("observe.problems", { rev: "1", items: [problem], nextCursor: null, truncated: false }),
  );
  expect(await screen.findByRole("link", { name: "src/x.rs:10:4" })).toBeVisible();
  fireEvent.change(screen.getByLabelText("문제 파일 편집기"), { target: { value: "cursor" } });
  fireEvent.click(screen.getByRole("link", { name: "src/x.rs:10:4" }));
  await waitFor(() =>
    expect(openFileLocation).toHaveBeenCalledWith(
      wire.client,
      "p",
      { path: "src/x.rs", line: 10, column: 4, source: { kind: "run", runId: "run-1" } },
      "cursor",
    ),
  );
});
test("reads every page, reports truncation and restarts from a fresh snapshot after invalid cursor", async () => {
  const { wire } = await setup();
  await act(async () =>
    wire.reply("observe.problems", {
      rev: "1",
      items: [problem],
      nextCursor: { rev: "1", offset: 1 },
      truncated: false,
    }),
  );
  await waitFor(() => expect(wire.requests("observe.problems")).toHaveLength(2));
  await act(async () =>
    wire.deliver({
      type: "response",
      id: wire.request("observe.problems", -1).id,
      error: { code: "observe.invalid_cursor" },
    }),
  );
  await waitFor(() => expect(wire.requests("observe.problems")).toHaveLength(3));
  expect(wire.request("observe.problems", -1).params).not.toHaveProperty("cursor");
  await act(async () =>
    wire.reply("observe.problems", {
      rev: "2",
      items: [{ ...problem, message: "새 진단" }],
      nextCursor: { rev: "2", offset: 1 },
      truncated: true,
    }),
  );
  await waitFor(() => expect(wire.requests("observe.problems")).toHaveLength(4));
  await act(async () =>
    wire.reply("observe.problems", {
      rev: "2",
      items: [{ ...problem, file: "src/y.rs" }],
      nextCursor: null,
      truncated: true,
    }),
  );
  expect(await screen.findByText("새 진단")).toBeVisible();
  expect(screen.getAllByRole("link")).toHaveLength(2);
  expect(screen.getByText("보관 한도를 넘어 일부 문제가 누락되었습니다.")).toBeVisible();
});
test("project changes cancel old pages and ignore late responses while file open failure stays visible", async () => {
  const { wire } = await setup();
  const old = wire.request("observe.problems").id;
  act(() => useCurrentProject.setState({ id: "other" }));
  await waitFor(() => expect(wire.requests("observe.problems")).toHaveLength(2));
  await act(async () => {
    wire.deliver({
      type: "response",
      id: old,
      result: { rev: "1", items: [problem], nextCursor: null, truncated: false },
    });
    wire.reply("observe.problems", {
      rev: "2",
      items: [{ ...problem, file: "other.rs" }],
      nextCursor: null,
      truncated: false,
    });
  });
  expect(screen.queryByRole("link", { name: "src/x.rs:10:4" })).toBeNull();
  vi.mocked(openFileLocation).mockRejectedValueOnce(new Error("unavailable"));
  fireEvent.click(await screen.findByRole("link", { name: "other.rs:10:4" }));
  expect(await screen.findByText("파일을 열지 못했습니다. 원본 파일과 외부 편집기 설치를 확인하세요.")).toBeVisible();
  expect(vi.mocked(openFileLocation).mock.calls.at(-1)?.[1]).toBe("other");
});
test("changed topic invalidates the complete snapshot and unmount removes its subscription", async () => {
  const { wire, view } = await setup();
  await act(async () =>
    wire.reply("observe.problems", { rev: "1", items: [problem], nextCursor: null, truncated: false }),
  );
  const sub = wire.sent.find((m) => m.type === "subscribe" && m.topic === "observe.problems.changed")!;
  await act(async () =>
    wire.deliver({ type: "event", subId: sub.id, rev: "2", payload: { rev: "2", projectId: "p" } }),
  );
  await waitFor(() => expect(wire.requests("observe.problems")).toHaveLength(2));
  view.rerender(
    <RpcProvider client={wire.client}>
      <div />
    </RpcProvider>,
  );
  expect(wire.sent.some((m) => m.type === "unsubscribe" && m.subId === sub.id)).toBe(true);
});
test("Devbox editor action resolves the owned project path and preserves line and column", async () => {
  const wire = new ProjectWire(),
    onOpenCode = vi.fn();
  render(
    <RpcProvider client={wire.client}>
      <ProblemsPanel onOpenCode={onOpenCode} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("observe.problems")).toHaveLength(1));
  await act(async () =>
    wire.reply("observe.problems", { rev: "1", items: [problem], nextCursor: null, truncated: false }),
  );
  fireEvent.click(await screen.findByRole("link", { name: "src/x.rs:10:4" }));
  await waitFor(() => expect(wire.requests("observe.resolve_location")).toHaveLength(1));
  await act(async () => wire.reply("observe.resolve_location", { path: "/owned/src/x.rs", line: 10, column: 4 }));
  await waitFor(() => expect(wire.requests("projects.list")).toHaveLength(1));
  await act(async () =>
    wire.reply("projects.list", {
      rev: "1",
      items: [{ id: "p", name: "owned", path: "/owned", favorite: false, addedMs: 0 }],
    }),
  );
  await waitFor(() => expect(wire.requests("files.stat")).toHaveLength(1));
  await act(async () =>
    wire.reply("files.stat", {
      key: { root: "project:p", path: "src/x.rs" },
      kind: "file",
      rev: { hash: "h", size: 1, mtimeNs: "1", ino: "2" },
    }),
  );
  await waitFor(() =>
    expect(onOpenCode).toHaveBeenCalledWith({ root: "project:p", path: "src/x.rs" }, { line: 10, column: 4 }),
  );
  expect(openFileLocation).not.toHaveBeenCalled();
});
