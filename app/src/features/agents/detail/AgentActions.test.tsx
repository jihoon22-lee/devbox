import { invoke } from "@tauri-apps/api/core";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../../rpc/RpcProvider";
import { TooltipProvider, useToasts } from "../../../ui";
import { useCurrentProject } from "../../projects/store";
import { ProjectWire } from "../../projects/test-wire";
import { trustPreview } from "../../projects/trust-fixtures";
import { openPopout } from "../../terminal/popout";
import { task } from "../test-fixtures";
import { AgentActions } from "./AgentActions";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
vi.mock("../../terminal/popout", () => ({ openPopout: vi.fn().mockResolvedValue(undefined) }));
const changes = { base: "main", ahead: 1, behind: 0, headOid: "h1", baseOid: "b1", files: [] };
const CLOSES =
  "확인하면 먼저 이 에이전트 도구를 멈춥니다. 성공하면 작업을 닫고, 자동 정리가 켜져 있으면 작업 폴더와 로컬 브랜치도 지웁니다. 닫힌 작업에는 이어서 지시할 수 없습니다.";
const REBASE = "에이전트 도구를 멈추고 브랜치를 기준 위로 옮깁니다. 옮긴 뒤 이어서 지시하려면 [재개]를 누르세요.";
async function confirmRebase() {
  await userEvent.click(screen.getByRole("button", { name: "기준 반영" }));
  const dialog = await screen.findByRole("dialog", { name: "기준 반영 · 작업 a1" });
  await userEvent.click(within(dialog).getByRole("button", { name: "기준 반영" }));
}
async function setup(inline = false, trustRequired = false) {
  const wire = new ProjectWire({ trustNotRequired: !trustRequired });
  const item = { ...task("a1", "idle"), ...(inline ? { worktreePath: null, branch: null } : {}) };
  render(
    <TooltipProvider>
      <RpcProvider client={wire.client}>
        <AgentActions task={item} sessionAlive onInstruct={vi.fn()} />
      </RpcProvider>
    </TooltipProvider>,
  );
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  if (!inline) {
    await waitFor(() => expect(wire.requests("agents.rebase_status")).toHaveLength(1));
    await act(async () => wire.reply("agents.rebase_status", { owned: false, files: [] }));
  }
  return wire;
}
test("discard freezes displayed head/count and cancel receives focus", async () => {
  const wire = await setup();
  await userEvent.click(screen.getByRole("button", { name: "더 보기" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "폐기" }));
  await waitFor(() => expect(wire.requests("agents.discard_check")).toHaveLength(1));
  await act(async () =>
    wire.reply("agents.discard_check", { headOid: "h1", unmergedCommits: 3, dirtyFiles: 1, fingerprint: "snapshot1" }),
  );
  expect(await screen.findByText(/병합하지 않은 커밋 3개/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "취소" })).toHaveFocus();
  expect(wire.requests("agents.discard")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "작업 폴더와 브랜치 삭제" }));
  await waitFor(() => expect(wire.requests("agents.discard")).toHaveLength(1));
  expect(wire.request("agents.discard").params).toEqual({
    id: "a1",
    expect: { headOid: "h1", unmergedCommits: 3, discardFingerprint: "snapshot1" },
  });
});
const SUBMODULES = "하위 모듈이 있습니다. 하위 모듈 안의 push하지 않은 커밋도 함께 사라집니다.";
test.each([true, false])(
  "a discard's confirmation warns of submodules only when the folder has them (%s)",
  async (hasSubmodules) => {
    const wire = await setup();
    await userEvent.click(screen.getByRole("button", { name: "더 보기" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "폐기" }));
    await waitFor(() => expect(wire.requests("agents.discard_check")).toHaveLength(1));
    await act(async () =>
      wire.reply("agents.discard_check", {
        headOid: "h1",
        unmergedCommits: 0,
        dirtyFiles: 0,
        fingerprint: "snapshot1",
        hasSubmodules,
      }),
    );
    const confirm = await screen.findByRole("button", { name: "작업 폴더와 브랜치 삭제" });
    // The forced removal takes the submodules' store too; the discard stays allowed.
    expect(document.body.textContent?.includes(SUBMODULES)).toBe(hasSubmodules);
    expect(confirm).toBeEnabled();
  },
);
test("a discard stopped by its failed teardown says to fix the command and discard again, never the cleanup list", async () => {
  const wire = await setup();
  await userEvent.click(screen.getByRole("button", { name: "더 보기" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "폐기" }));
  await waitFor(() => expect(wire.requests("agents.discard_check")).toHaveLength(1));
  await act(async () =>
    wire.reply("agents.discard_check", { headOid: "h1", unmergedCommits: 3, dirtyFiles: 1, fingerprint: "snapshot1" }),
  );
  await userEvent.click(await screen.findByRole("button", { name: "작업 폴더와 브랜치 삭제" }));
  await waitFor(() => expect(wire.requests("agents.discard")).toHaveLength(1));
  await act(async () =>
    wire.deliver({
      type: "response",
      id: wire.request("agents.discard").id,
      error: { code: "agents.teardown_failed", detail: { index: 2, exit_code: 3 } },
    }),
  );
  // The task is still open, so it is not in the cleanup list: the discard is what to try again.
  const notice = await screen.findByText(/정리 명령이 실패했습니다\(종료 코드 3\)/);
  expect(notice).toHaveTextContent(
    "정리 명령이 실패했습니다(종료 코드 3). .devbox/devbox.toml의 [agent] teardown 2번째 명령을 작업 폴더에서 직접 실행해 원인을 고친 뒤 다시 폐기하세요.",
  );
  expect(document.body).not.toHaveTextContent("정리 후보");
  expect(wire.requests("agents.discard")).toHaveLength(1);
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
  await waitFor(() => expect(wire.requests("projects.config")).toHaveLength(1));
  await act(async () =>
    wire.reply("projects.config", {
      config: {
        agent: { test: "cargo test", copy: [], setup: [], teardown: [] },
        launch: { services: [], terminals: [], agents: [] },
      },
    }),
  );
  await confirmRebase();
  await waitFor(() => expect(wire.requests("agents.rebase")).toHaveLength(1));
  await act(async () => wire.reply("agents.rebase", { result: "conflict", files: ["a.txt"] }));
  await waitFor(() => expect(wire.requests("agents.rebase_status")).toHaveLength(2));
  await act(async () => wire.reply("agents.rebase_status", { owned: true, files: ["a.txt"] }));
  await userEvent.click(await screen.findByRole("button", { name: "해결 지시 보내기" }));
  await waitFor(() => expect(wire.requests("agents.send")).toHaveLength(1));
  expect(wire.request("agents.send").params).toMatchObject({
    id: "a1",
    text: expect.stringContaining("a.txt. 충돌을 해결하고 git rebase --continue"),
  });
  await act(async () => wire.reply("agents.send", task("a1")));
  await confirmRebase();
  await waitFor(() => expect(wire.requests("agents.rebase")).toHaveLength(2));
  await act(async () => wire.reply("agents.rebase", { result: "conflict", files: ["a.txt"] }));
  await waitFor(() => expect(wire.requests("agents.rebase_status")).toHaveLength(4));
  await act(async () => wire.reply("agents.rebase_status", { owned: true, files: ["a.txt"] }));
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
  const wire = new ProjectWire({ trustNotRequired: true }),
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

test("trust denial discards old Git confirmation and explicitly rechecks trust before fresh Git", async () => {
  const wire = await setup(false, true);
  await userEvent.click(screen.getByRole("button", { name: "병합" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "병합 커밋" }));
  await waitFor(() => expect(wire.requests("projects.trust_preview")).toHaveLength(1));
  expect(wire.requests("agents.changes")).toHaveLength(0);
  await act(async () => wire.reply("projects.trust_preview", trustPreview("a", { approved: true })));
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(1));
  await act(async () => wire.reply("agents.changes", changes));
  await userEvent.click(await screen.findByRole("button", { name: "main에 병합" }));
  await waitFor(() => expect(wire.requests("agents.merge")).toHaveLength(1));
  await act(async () =>
    wire.deliver({
      type: "response",
      id: wire.request("agents.merge").id,
      error: { code: "agents.project_trust_required", detail: { id: "a" } },
    }),
  );
  expect(screen.getByRole("button", { name: "main에 병합" })).toBeDisabled();
  expect(wire.requests("agents.changes")).toHaveLength(1);
  expect(wire.requests("projects.trust_preview")).toHaveLength(1);
  await userEvent.click(await screen.findByRole("button", { name: "다시 확인" }));
  await waitFor(() => expect(wire.requests("projects.trust_preview")).toHaveLength(2));
  await act(async () => wire.reply("projects.trust_preview", trustPreview("a", { digest: "changed-definition" })));
  expect(wire.requests("agents.changes")).toHaveLength(1);
  await userEvent.click(await screen.findByRole("button", { name: "명령 실행 승인" }));
  await waitFor(() => expect(wire.requests("projects.trust_approve")).toHaveLength(1));
  await act(async () => wire.reply("projects.trust_approve", { rev: "2" }));
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(2));
  await act(async () => wire.reply("agents.changes", { ...changes, headOid: "fresh-head", baseOid: "fresh-base" }));
  expect(wire.requests("agents.merge")).toHaveLength(1);
  await waitFor(() => expect(screen.getByRole("button", { name: "main에 병합" })).toBeEnabled());
  await userEvent.click(screen.getByRole("button", { name: "main에 병합" }));
  await waitFor(() => expect(wire.requests("agents.merge")).toHaveLength(2));
  expect(wire.request("agents.merge", -1).params).toMatchObject({
    expect: { headOid: "fresh-head", baseOid: "fresh-base" },
  });
});

test("the resolution instruction is held while the agent waits for a choice in its terminal", async () => {
  const wire = new ProjectWire({ trustNotRequired: true });
  const node = (state: ReturnType<typeof task>["state"]) => (
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <AgentActions task={task("a1", state)} sessionAlive onInstruct={vi.fn()} />
      </TooltipProvider>
    </RpcProvider>
  );
  const view = render(node("waiting"));
  await waitFor(() => expect(wire.requests("agents.rebase_status")).toHaveLength(1));
  await act(async () => wire.reply("agents.rebase_status", { owned: true, files: ["a.txt"] }));
  // Text sent now could answer the approval prompt, as in the composer. The other choices stay open.
  expect(await screen.findByRole("button", { name: "해결 지시 보내기" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "되돌리기" })).toBeEnabled();
  expect(wire.requests("agents.send")).toHaveLength(0);
  view.rerender(node("idle"));
  await waitFor(() => expect(screen.getByRole("button", { name: "해결 지시 보내기" })).toBeEnabled());
});

test("PR·병합 확인 창은 성공하면 작업을 닫는다고 알린다", async () => {
  const wire = await setup();
  await userEvent.click(screen.getByRole("button", { name: "병합" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "병합 커밋" }));
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(1));
  await act(async () => wire.reply("agents.changes", changes));
  const merge = await screen.findByRole("dialog", { name: "main에 병합 · 작업 a1" });
  await waitFor(() => expect(merge).toHaveTextContent("HEAD h1 · 기준 b1"));
  expect(merge).toHaveTextContent("성공하면 작업을 닫고");
  // The sentence ends the body, after the refs the user confirms.
  expect(merge).toHaveAccessibleDescription(expect.stringMatching(/기준 b1\s확인하면 먼저 .*할 수 없습니다\.$/));
  expect(merge).toHaveTextContent(CLOSES);
  await userEvent.click(within(merge).getByRole("button", { name: "취소" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await userEvent.click(screen.getByRole("button", { name: "PR" }));
  await waitFor(() => expect(wire.requests("agents.commits")).toHaveLength(1));
  await act(async () => wire.reply("agents.commits", { items: [] }));
  await userEvent.click(await screen.findByRole("button", { name: "내용 확인" }));
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(2));
  await act(async () => wire.reply("agents.changes", changes));
  const pr = await screen.findByRole("dialog", { name: "PR 만들기 · 작업 a1" });
  await waitFor(() => expect(pr).toHaveTextContent("HEAD h1 · 기준 b1"));
  expect(pr).toHaveTextContent("성공하면 작업을 닫고");
  expect(pr).toHaveAccessibleDescription(expect.stringMatching(/PR 제목: 작업 a1[\s\S]*할 수 없습니다\.$/));
  expect(pr).toHaveTextContent(CLOSES);
  expect(wire.requests("agents.merge")).toHaveLength(0);
  expect(wire.requests("agents.pr")).toHaveLength(0);
});

test("도구가 살아 있으면 [기준 반영]은 확인한 뒤에만 보낸다", async () => {
  const wire = await setup();
  await userEvent.click(screen.getByRole("button", { name: "기준 반영" }));
  const dialog = await screen.findByRole("dialog", { name: "기준 반영 · 작업 a1" });
  expect(dialog).toHaveAccessibleDescription(REBASE);
  expect(within(dialog).getByRole("button", { name: "취소" })).toHaveFocus();
  await userEvent.click(within(dialog).getByRole("button", { name: "취소" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(wire.requests("agents.rebase")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "기준 반영" }));
  const again = await screen.findByRole("dialog", { name: "기준 반영 · 작업 a1" });
  expect(wire.requests("agents.rebase")).toHaveLength(0);
  await userEvent.click(within(again).getByRole("button", { name: "기준 반영" }));
  await waitFor(() => expect(wire.requests("agents.rebase")).toHaveLength(1));
  expect(wire.request("agents.rebase").params).toEqual({ id: "a1" });
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

async function renderTool(tool: { sessionAlive: boolean; sessionKnown: boolean; toolExited: boolean }) {
  const wire = new ProjectWire({ trustNotRequired: true });
  render(
    <TooltipProvider>
      <RpcProvider client={wire.client}>
        <AgentActions
          task={{ ...task("a1", "review"), toolExited: tool.toolExited }}
          sessionAlive={tool.sessionAlive}
          sessionKnown={tool.sessionKnown}
        />
      </RpcProvider>
    </TooltipProvider>,
  );
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  await waitFor(() => expect(wire.requests("agents.rebase_status")).toHaveLength(1));
  await act(async () => wire.reply("agents.rebase_status", { owned: false, files: [] }));
  return wire;
}
test.each([
  { sessionAlive: true, sessionKnown: true, toolExited: true },
  { sessionAlive: false, sessionKnown: true, toolExited: false },
  { sessionAlive: false, sessionKnown: false, toolExited: true },
])("도구가 살아 있지 않으면 [기준 반영]은 확인 없이 보낸다 %o", async (tool) => {
  const wire = await renderTool(tool);
  await userEvent.click(screen.getByRole("button", { name: "기준 반영" }));
  await waitFor(() => expect(wire.requests("agents.rebase")).toHaveLength(1));
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("세션 목록을 아직 모르면 [기준 반영]은 확인한 뒤에만 보낸다", async () => {
  // Until the terminal list loads, a running tool cannot be ruled out.
  const wire = await renderTool({ sessionAlive: false, sessionKnown: false, toolExited: false });
  await userEvent.click(screen.getByRole("button", { name: "기준 반영" }));
  const dialog = await screen.findByRole("dialog", { name: "기준 반영 · 작업 a1" });
  expect(dialog).toHaveAccessibleDescription(REBASE);
  expect(wire.requests("agents.rebase")).toHaveLength(0);
  await userEvent.click(within(dialog).getByRole("button", { name: "기준 반영" }));
  await waitFor(() => expect(wire.requests("agents.rebase")).toHaveLength(1));
});

test("병합 충돌 띠의 [기준 반영 실행]도 도구가 살아 있으면 확인한 뒤에만 보낸다", async () => {
  const wire = await setup();
  await userEvent.click(screen.getByRole("button", { name: "병합" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "병합 커밋" }));
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(1));
  await act(async () => wire.reply("agents.changes", changes));
  await userEvent.click(await screen.findByRole("button", { name: "main에 병합" }));
  await waitFor(() => expect(wire.requests("agents.merge")).toHaveLength(1));
  await act(async () =>
    wire.deliver({
      type: "response",
      id: wire.request("agents.merge").id,
      error: { code: "agents.conflict", detail: { files: ["a.txt"] } },
    }),
  );
  // The merge's own conflict shows the banner without an owned rebase.
  await userEvent.click(await screen.findByRole("button", { name: "기준 반영 실행" }));
  const dialog = await screen.findByRole("dialog", { name: "기준 반영 · 작업 a1" });
  expect(dialog).toHaveAccessibleDescription(REBASE);
  expect(wire.requests("agents.rebase")).toHaveLength(0);
  await userEvent.click(within(dialog).getByRole("button", { name: "기준 반영" }));
  await waitFor(() => expect(wire.requests("agents.rebase")).toHaveLength(1));
  expect(wire.request("agents.rebase").params).toEqual({ id: "a1" });
});

async function openMore(item: ReturnType<typeof task>) {
  const wire = new ProjectWire({ trustNotRequired: true });
  render(
    <TooltipProvider>
      <RpcProvider client={wire.client}>
        <AgentActions task={item} sessionAlive onInstruct={vi.fn()} />
      </RpcProvider>
    </TooltipProvider>,
  );
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  await userEvent.click(await screen.findByRole("button", { name: "더 보기" }));
  return wire;
}
test("opens_task_worktree_without_switching_global_project", async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  useCurrentProject.setState({ id: "other-project" });
  try {
    const item = { ...task("a1", "idle"), projectId: "p-task" };
    const wire = await openMore(item);
    await userEvent.click(await screen.findByRole("menuitem", { name: "VS Code로 작업 폴더 열기" }));
    expect(invoke).toHaveBeenLastCalledWith("open_external", { kind: "vscode", path: item.worktreePath });
    await userEvent.click(screen.getByRole("button", { name: "더 보기" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "작업 폴더 터미널" }));
    await waitFor(() => expect(wire.requests("terminal.create")).toHaveLength(1));
    expect(wire.request("terminal.create").params).toEqual({
      projectId: "p-task",
      cwd: item.worktreePath,
      title: null,
      command: null,
      attachZellij: null,
    });
    await act(async () => wire.reply("terminal.create", { id: "t1", projectId: "p-task" }));
    await waitFor(() => expect(openPopout).toHaveBeenLastCalledWith("terminal", "t1"));
    expect(useCurrentProject.getState().id).toBe("other-project");
  } finally {
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  }
});
test("the VS Code item is only in the Windows app", async () => {
  await openMore(task("a1", "idle"));
  expect(await screen.findByRole("menuitem", { name: "작업 폴더 터미널" })).toBeInTheDocument();
  expect(screen.queryByRole("menuitem", { name: "VS Code로 작업 폴더 열기" })).toBeNull();
});
test("a task without a worktree folder has neither folder item", async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  try {
    await openMore({ ...task("a1", "idle"), worktreePath: null, branch: null });
    await screen.findByRole("menuitem", { name: "테스트 실행" });
    expect(screen.queryByRole("menuitem", { name: /작업 폴더 터미널|VS Code로 작업 폴더/ })).toBeNull();
  } finally {
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  }
});
