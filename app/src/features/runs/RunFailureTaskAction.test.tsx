import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { LogLine, Run } from "../../rpc/gen/rpc";
import { messageText } from "../../rpc/messages";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NewTaskDialog } from "../agents/NewTaskDialog";
import { profiles } from "../agents/test-fixtures";
import { type AgentDraft, useAgentsUi } from "../agents/ui";
import { useCurrentProject } from "../projects/store";
import { ProjectRuns } from "./RunsPage";
import { RunWire, runFixture, serviceDefinition } from "./test-wire";

const failed = (id: string, startedMs: number, code: number): Run => ({
  ...runFixture,
  id,
  state: "exited",
  exit: { attemptId: `attempt-${id}`, code, endedMs: startedMs + 500, by: "wrapper" },
  startedMs,
  endedMs: startedMs + 500,
  ready: false,
  pid: null,
});
const row = (text: string, runId: string): LogLine => ({
  text,
  src: { kind: "run", runId },
  ts: "1",
  stream: "stderr",
});
/** The new task form as the app mounts it once for every section (AgentCommands). */
function TaskForm() {
  const { creating, setCreating } = useAgentsUi();
  return <NewTaskDialog open={creating} onOpenChange={setCreating} onCreated={() => {}} />;
}
function show(wire: RunWire, run: Run) {
  wire.runs = [run];
  wire.selected = run;
  render(
    <RpcProvider client={wire.client}>
      <ProjectRuns projectId="p" />
      <TaskForm />
    </RpcProvider>,
  );
}
const fixButton = () => screen.findByRole("button", { name: "에이전트로 고치기" });
afterEach(() => {
  cleanup();
  useAgentsUi.getState().setCreating(false);
  useCurrentProject.getState().set(undefined);
});

test("failed_run_handoff_drops_late_logs_after_target_change", async () => {
  const wire = new RunWire();
  const a = failed("run-a", 2_000, 1);
  const b = failed("run-b", 1_000, 2);
  wire.history = [a, b];
  const drafts: AgentDraft[] = [];
  const stop = useAgentsUi.subscribe((state, previous) => {
    if (state.draft && state.draft !== previous.draft) drafts.push(state.draft);
  });
  show(wire, a);
  await userEvent.click(await fixButton());
  await waitFor(() => expect(wire.requests("logs.search")).toHaveLength(1));
  const forA = wire.request("logs.search");
  expect(forA.params).toMatchObject({ sources: [{ kind: "run", runId: "run-a" }] });
  expect(await fixButton()).toBeDisabled();
  // Run B is shown while A's logs are still on their way.
  wire.selected = b;
  await userEvent.selectOptions(screen.getByRole("combobox", { name: "지난 실행" }), "run-b");
  expect(await screen.findByText("run-b")).toBeVisible();
  expect(wire.sent).toContainEqual({ type: "cancel", id: forA.id });
  // A cancel only asks the daemon to stop, so A's reply can still come and complete the read.
  await act(async () =>
    wire.deliver({ type: "response", id: forA.id, result: { items: [row("A의 오류", "run-a")], nextCursor: null } }),
  );
  expect(useAgentsUi.getState().draft).toBeUndefined();
  await userEvent.click(await fixButton());
  await waitFor(() => expect(wire.requests("logs.search")).toHaveLength(2));
  const forB = wire.request("logs.search", 1);
  expect(forB.params).toMatchObject({ sources: [{ kind: "run", runId: "run-b" }] });
  await act(async () =>
    wire.deliver({ type: "response", id: forB.id, result: { items: [row("B의 오류", "run-b")], nextCursor: null } }),
  );
  await waitFor(() => expect(drafts).toHaveLength(1));
  expect(drafts[0]?.prompt).toContain("실행 ID run-b");
  expect(drafts[0]?.prompt).toContain("> B의 오류");
  expect(drafts.some((draft) => draft.prompt.includes("A의 오류") || draft.prompt.includes("run-a"))).toBe(false);
  stop();
});

test("failed_run_draft_has_summary_and_log_tail_within_the_create_limit", async () => {
  const wire = new RunWire();
  show(wire, failed("run-1", 2_000, 1));
  await userEvent.click(await fixButton());
  await waitFor(() => expect(wire.requests("logs.search")).toHaveLength(1));
  // Newest first, as the daemon pages them: 200 rows of about 1.2 KB, far beyond the 64 KiB create limit.
  const items = Array.from({ length: 200 }, (_, i) =>
    row(`${i === 0 ? "가장 새 줄" : i === 199 ? "가장 오래된 줄" : `줄 ${i}`} ${"한".repeat(400)}`, "run-1"),
  );
  await act(async () => wire.reply("logs.search", { items, nextCursor: null }));
  await waitFor(() => expect(wire.requests("agents.profiles")).toHaveLength(1));
  await waitFor(() => expect(wire.requests("projects.list")).toHaveLength(1));
  await act(async () => {
    wire.reply("projects.list", {
      rev: "1",
      items: [{ id: "p", name: "웹 프로젝트", path: "/fixture/p", favorite: false, addedMs: 1 }],
    });
    wire.reply("agents.profiles", { items: profiles });
  });
  const prompt = (screen.getByLabelText("지시문") as HTMLTextAreaElement).value;
  expect(prompt.split("\n").slice(0, 2)).toEqual([
    "실행 web이(가) 실패했습니다(종료 코드 1, 실행 ID run-1). 아래 가려진 실행 로그 끝을 보고 원인을 고쳐 주세요.",
    "> [전송 크기 제한으로 일부 로그 생략]",
  ]);
  // The tail is what gets cut: the newest row ends the prompt and the oldest is gone.
  expect(prompt.trimEnd().split("\n").at(-1)).toMatch(/^> 가장 새 줄 한/);
  expect(prompt).not.toContain("가장 오래된 줄");
  expect(new TextEncoder().encode(prompt).length).toBeLessThanOrEqual(65536);
  // The page lists the definition with its command; none of it reaches the prompt.
  const { def } = serviceDefinition;
  if (def.kind !== "service") throw new Error("the fixture definition is a service");
  expect(prompt).not.toContain(def.command);
  expect(screen.getByLabelText("제목")).toHaveValue("Fix failed run web");
  await waitFor(() => expect(screen.getByRole("combobox", { name: "프로젝트" })).toHaveTextContent("웹 프로젝트"));
  expect(useAgentsUi.getState().draft).toEqual({ title: "Fix failed run web", projectId: "p", prompt });
  // The note-length notice belongs to notes; the quote carries its own omission line.
  expect(screen.queryByText(/노트 내용이 길어/)).toBeNull();
  expect(wire.requests("agents.create")).toHaveLength(0);
});

test("failed_run_log_read_failure_shows_the_reason_and_opens_nothing", async () => {
  const wire = new RunWire();
  show(wire, failed("run-1", 2_000, 1));
  const fix = await fixButton();
  await userEvent.click(fix);
  await waitFor(() => expect(wire.requests("logs.search")).toHaveLength(1));
  await act(async () =>
    wire.deliver({
      type: "response",
      id: wire.request("logs.search").id,
      error: { code: "internal" },
      diagnosticId: "diag-7",
    }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(messageText("internal", undefined, "diag-7"));
  expect(fix).toBeEnabled();
  expect(useAgentsUi.getState().creating).toBe(false);
});
