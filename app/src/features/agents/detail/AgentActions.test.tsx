import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../../rpc/RpcProvider";
import { TooltipProvider, useToasts } from "../../../ui";
import { ProjectWire } from "../../projects/test-wire";
import { task } from "../test-fixtures";
import { AgentActions } from "./AgentActions";

const changes = { base: "main", ahead: 1, behind: 0, headOid: "h1", baseOid: "b1", files: [] };
async function setup(inline = false) {
  const wire = new ProjectWire();
  const item = { ...task("a1", "idle"), ...(inline ? { worktreePath: null, branch: null } : {}) };
  render(
    <TooltipProvider>
      <RpcProvider client={wire.client}>
        <AgentActions task={item} sessionAlive onInstruct={vi.fn()} />
      </RpcProvider>
    </TooltipProvider>,
  );
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  return wire;
}
test("discard freezes displayed head/count and cancel receives focus", async () => {
  const wire = await setup();
  await userEvent.click(screen.getByRole("button", { name: "더 보기" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "폐기" }));
  await waitFor(() => expect(wire.requests("agents.discard_check")).toHaveLength(1));
  await act(async () => wire.reply("agents.discard_check", { headOid: "h1", unmergedCommits: 3, dirtyFiles: 1 }));
  expect(await screen.findByText(/병합하지 않은 커밋 3개/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "취소" })).toHaveFocus();
  expect(wire.requests("agents.discard")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "작업 폴더와 브랜치 삭제" }));
  await waitFor(() => expect(wire.requests("agents.discard")).toHaveLength(1));
  expect(wire.request("agents.discard").params).toEqual({ id: "a1", expect: { headOid: "h1", unmergedCommits: 3 } });
});
test("state_changed refetches the snapshot but requires a second explicit confirmation", async () => {
  const wire = await setup();
  await userEvent.click(screen.getByRole("button", { name: "병합" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "병합 커밋" }));
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(1));
  await act(async () => wire.reply("agents.changes", changes));
  await userEvent.click(await screen.findByRole("button", { name: "main에 병합" }));
  await waitFor(() => expect(wire.requests("agents.merge")).toHaveLength(1));
  expect(wire.request("agents.merge").params).toEqual({
    id: "a1",
    strategy: "merge",
    message: null,
    expect: { headOid: "h1", baseOid: "b1" },
  });
  await act(async () =>
    wire.deliver({
      type: "response",
      id: wire.request("agents.merge").id,
      error: { code: "agents.state_changed", detail: { head: "h2", base: "b2", unmerged: 2 } },
    }),
  );
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(2));
  await act(async () => wire.reply("agents.changes", { ...changes, headOid: "h2", baseOid: "b2", ahead: 2 }));
  expect(await screen.findByText(/확인한 뒤 작업이 바뀌었습니다/)).toBeInTheDocument();
  expect(wire.requests("agents.merge")).toHaveLength(1);
  await userEvent.click(screen.getByRole("button", { name: "main에 병합" }));
  await waitFor(() => expect(wire.requests("agents.merge")).toHaveLength(2));
  expect(wire.request("agents.merge", -1).params).toMatchObject({ expect: { headOid: "h2", baseOid: "b2" } });
});
test("inline tasks hide Git actions and describe ending the session", async () => {
  await setup(true);
  expect(screen.queryByRole("button", { name: "기준 반영" })).toBeNull();
  expect(screen.queryByRole("button", { name: "병합" })).toBeNull();
  expect(screen.queryByRole("button", { name: "PR" })).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "더 보기" }));
  expect(await screen.findByRole("menuitem", { name: "세션 끝내기" })).toBeInTheDocument();
});

test("PR edits title/body, confirms exact refs, and exposes a successful open action", async () => {
  const wire = await setup();
  await userEvent.click(screen.getByRole("button", { name: "PR" }));
  await waitFor(() => expect(wire.requests("agents.commits")).toHaveLength(1));
  await act(async () =>
    wire.reply("agents.commits", { items: [{ sha: "abc12345", subject: "Fix login", author: "fixture", timeMs: 1 }] }),
  );
  expect(await screen.findByLabelText("PR 본문")).toHaveValue("\n\n- abc12345 Fix login");
  await userEvent.clear(screen.getByLabelText("PR 제목"));
  await userEvent.type(screen.getByLabelText("PR 제목"), "변경 제목");
  await userEvent.click(screen.getByRole("button", { name: "내용 확인" }));
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(1));
  await act(async () => wire.reply("agents.changes", changes));
  expect(wire.requests("agents.pr")).toHaveLength(0);
  expect(screen.getByRole("button", { name: "취소" })).toHaveFocus();
  await userEvent.click(screen.getByRole("button", { name: "PR 만들기" }));
  await waitFor(() => expect(wire.requests("agents.pr")).toHaveLength(1));
  expect(wire.request("agents.pr").params).toEqual({
    id: "a1",
    title: "변경 제목",
    body: "\n\n- abc12345 Fix login",
    expect: { headOid: "h1", baseOid: "b1" },
  });
  await act(async () =>
    wire.reply("agents.pr", {
      ...task("a1", "closed"),
      prUrl: "https://github.com/example/fixture/pull/1",
      closedHow: "pullRequest",
    }),
  );
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  const toast = useToasts.getState().items.find((item) => item.title === "PR을 만들었습니다");
  expect(toast?.action?.label).toBe("PR 열기");
});
test("rebase conflict offers resolution instruction and abort; review and configured test return their results", async () => {
  const wire = await setup();
  await act(async () =>
    wire.reply("projects.config", {
      config: {
        agent: { test: "cargo test", copy: [], setup: [], teardown: [] },
        launch: { services: [], terminals: [], agents: [] },
      },
    }),
  );
  await userEvent.click(screen.getByRole("button", { name: "기준 반영" }));
  await waitFor(() => expect(wire.requests("agents.rebase")).toHaveLength(1));
  await act(async () => wire.reply("agents.rebase", { result: "conflict", files: ["a.txt"] }));
  await userEvent.click(await screen.findByRole("button", { name: "해결 지시 보내기" }));
  await waitFor(() => expect(wire.requests("agents.send")).toHaveLength(1));
  expect(wire.request("agents.send").params).toMatchObject({
    id: "a1",
    text: expect.stringContaining("a.txt. 충돌을 해결하고 git rebase --continue"),
  });
  await act(async () => wire.reply("agents.send", task("a1")));
  await userEvent.click(screen.getByRole("button", { name: "기준 반영" }));
  await waitFor(() => expect(wire.requests("agents.rebase")).toHaveLength(2));
  await act(async () => wire.reply("agents.rebase", { result: "conflict", files: ["a.txt"] }));
  await userEvent.click(await screen.findByRole("button", { name: "되돌리기" }));
  await waitFor(() => expect(wire.requests("agents.rebase_abort")).toHaveLength(1));
  await act(async () => wire.reply("agents.rebase_abort", task("a1")));
  expect(screen.queryByRole("button", { name: "해결 지시 보내기" })).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "더 보기" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "검토 시작" }));
  await waitFor(() => expect(wire.requests("agents.review")).toHaveLength(1));
  await act(async () => wire.reply("agents.review", task("a1", "review")));
  await userEvent.click(screen.getByRole("button", { name: "더 보기" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "테스트 실행" }));
  await waitFor(() => expect(wire.requests("agents.test")).toHaveLength(1));
});
test("lost session resumes; closed tasks provide no actions", async () => {
  const wire = new ProjectWire(),
    resumed = vi.fn();
  const view = render(
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <AgentActions task={task("a", "failed")} sessionAlive={false} onResume={resumed} />
      </TooltipProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  expect(screen.queryByRole("button", { name: "이어서 지시" })).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "재개" }));
  await waitFor(() => expect(wire.requests("agents.resume")).toHaveLength(1));
  await act(async () => wire.reply("agents.resume", task("a")));
  await waitFor(() => expect(resumed).toHaveBeenCalledOnce());
  view.rerender(
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <AgentActions task={task("a", "closed")} sessionAlive={false} />
      </TooltipProvider>
    </RpcProvider>,
  );
  expect(screen.queryAllByRole("button")).toHaveLength(0);
});
