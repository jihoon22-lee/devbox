import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../../rpc/RpcProvider";
import { ProjectWire } from "../../projects/test-wire";
import { task } from "../test-fixtures";
import { AgentHeader } from "./AgentHeader";

test("header shows raw overlapping token counters, fixed closed age and cleanup diagnostics", async () => {
  const wire = new ProjectWire();
  const item = {
    ...task("a", "closed", 1000),
    closedMs: 2000,
    closedHow: "merged" as const,
    cleanupError: JSON.stringify({ code: "agents.unsafe_worktree" }),
  };
  render(
    <RpcProvider client={wire.client}>
      <AgentHeader task={item} projectName="fixture" toolName="Codex" now={999999} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.usage")).toHaveLength(1));
  expect(screen.getByText("토큰 알 수 없음")).toBeInTheDocument();
  await act(async () =>
    wire.reply("agents.usage", { tokens: { input: 10, output: 2, cacheRead: 8, cacheWrite: 3 }, truncated: true }),
  );
  expect(await screen.findByText("입력 토큰 10")).toBeInTheDocument();
  expect(screen.getByText("출력 토큰 2")).toBeInTheDocument();
  expect(screen.getByText("캐시 읽기 토큰 8")).toBeInTheDocument();
  expect(screen.getByText("캐시 쓰기 토큰 3")).toBeInTheDocument();
  expect(screen.getByText("토큰 기록 일부")).toBeInTheDocument();
  expect(screen.getByText("경과 1초")).toBeInTheDocument();
  expect(screen.getByText("CPU 알 수 없음")).toBeInTheDocument();
  expect(screen.getByText("메모리 알 수 없음")).toBeInTheDocument();
  expect(screen.getByRole("alert")).toHaveTextContent("작업 폴더의 소유 상태를 확인할 수 없습니다.");
});

function header(cleanupError: string) {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <AgentHeader
        task={{ ...task("a", "closed", 1000), closedHow: "merged", cleanupError }}
        projectName="fixture"
        toolName="Codex"
        now={999999}
      />
    </RpcProvider>,
  );
}

test("cleanup_error_shows_cause_and_next_action", async () => {
  const raw = JSON.stringify({ code: "agents.worktree_dirty", detail: { files: ["retained.txt"] } });
  header(raw);
  const alert = screen.getByRole("alert");
  expect(alert).toHaveTextContent("정리하지 못한 항목이 있습니다.");
  // Cleanup first undoes what changed during it, so what stopped it are changes devbox does not
  // delete. The closed task has nothing to retry, and a commit would move HEAD off the closed one,
  // which takes the work folder out of the cleanup list for good.
  expect(alert).toHaveTextContent(
    "작업 폴더에 devbox가 지우지 않는 변경이 남아 있습니다. 다시 정리해도 같은 것이 또 생기면 정리 명령부터 고치세요. 남은 파일은 옮기거나 지우고 바뀐 파일은 되돌린 뒤(커밋하지 말고) 설정 → 에이전트 → 정리 후보에서 다시 정리하세요.",
  );
  expect(alert).not.toHaveTextContent("커밋한 뒤");
  expect(alert).not.toHaveTextContent("다시 시도");
  // The stored text, with the files it names, is only the details behind [자세히].
  expect(screen.queryByText(raw)).not.toBeInTheDocument();
  expect(document.body).not.toHaveTextContent("agents.worktree_dirty");
  expect(document.body).not.toHaveTextContent("retained.txt");
  const details = screen.getByRole("button", { name: "자세히" });
  expect(details).toHaveAttribute("aria-expanded", "false");
  await userEvent.click(details);
  expect(details).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByText(raw)).toBeInTheDocument();
  expect(screen.getByText(raw)).toHaveTextContent("retained.txt");
  await userEvent.click(details);
  expect(screen.queryByText(raw)).not.toBeInTheDocument();
});

test("a failed teardown's notice names the command to run in the work folder and the full way to the cleanup list", async () => {
  const raw = JSON.stringify({ code: "agents.teardown_failed", detail: { index: 2, exit_code: 3 } });
  header(raw);
  const alert = screen.getByRole("alert");
  expect(alert).toHaveTextContent("정리하지 못한 항목이 있습니다. 정리 명령이 실패했습니다(종료 코드 3).");
  // The notice is outside settings, so it names the whole way to the list.
  expect(alert).toHaveTextContent(
    ".devbox/devbox.toml의 [agent] teardown 2번째 명령을 작업 폴더에서 직접 실행해 원인을 고친 뒤 설정 → 에이전트 → 정리 후보에서 다시 정리하세요.",
  );
  expect(alert).not.toHaveTextContent("원인을 확인하지 못했습니다.");
  expect(alert).not.toHaveTextContent("남은 항목을 확인하세요");
});

test("a teardown stopped without an exit code says it hit the time limit or was killed", () => {
  header(JSON.stringify({ code: "agents.teardown_failed", detail: { index: 1, exit_code: null } }));
  expect(screen.getByRole("alert")).toHaveTextContent(
    "정리하지 못한 항목이 있습니다. 정리 명령이 60초 제한 시간을 넘겨 중단됐거나 강제로 종료됐습니다.",
  );
});

test("a work folder kept for its submodules: the notice gives the command that removes it, then points to the cleanup list", () => {
  header(JSON.stringify({ code: "agents.worktree_has_submodules", detail: { path: "/fixture/owned" } }));
  expect(screen.getByRole("alert")).toHaveTextContent(
    "정리하지 못한 항목이 있습니다. 하위 모듈이 있는 작업 폴더는 devbox가 지우지 않습니다. 하위 모듈의 변경은 커밋하고 커밋은 push한 뒤 프로젝트 폴더의 터미널에서 git worktree remove --force '/fixture/owned'로 직접 지우세요. 설정 → 에이전트 → 정리 후보에서 남은 항목을 확인하세요.",
  );
});

test("a work folder kept for an unfinished merge or rebase: the notice says to abort it, not finish it, then points to the cleanup list", () => {
  header(JSON.stringify({ code: "agents.operation_in_progress" }));
  const alert = screen.getByRole("alert");
  expect(alert).toHaveTextContent(
    "정리하지 못한 항목이 있습니다. 작업 폴더에 끝나지 않은 Git 작업(병합·rebase·cherry-pick 등)이 있습니다. 작업 폴더에서 그 작업을 마치지 말고 중단한 뒤(예: git merge --abort, git rebase --abort) 설정 → 에이전트 → 정리 후보에서 다시 정리하세요. 마치면 커밋이 생겨 이 작업 폴더는 다시 정리 후보에 나오지 않습니다.",
  );
  // The table's copy, for a rebase or merge, says to finish the operation, which would commit.
  expect(alert).not.toHaveTextContent("다른 Git 작업");
  expect(alert).not.toHaveTextContent("진행 중인 작업을 마친 뒤");
});

test("a merge's own next action never reaches the notice: other codes point to the cleanup list, and the stored text stays under [자세히]", async () => {
  const raw = JSON.stringify({ code: "agents.base_dirty", detail: { files: ["a.txt", "b.txt"] } });
  header(raw);
  const alert = screen.getByRole("alert");
  expect(alert).toHaveTextContent("기준 체크아웃에 커밋하지 않은 변경이 있어 병합하지 않았습니다.");
  expect(alert).toHaveTextContent("설정 → 에이전트 → 정리 후보에서 남은 항목을 확인하세요.");
  // That action was written for the merge; a closed task has no merge to try again.
  expect(alert).not.toHaveTextContent("다시 시도");
  expect(alert).not.toHaveTextContent("a.txt");
  expect(screen.queryByText(raw)).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "자세히" }));
  expect(screen.getByText(raw)).toHaveTextContent("a.txt");
});

test.each([
  ["fixture branch retained"],
  ["정리 중 오류가 발생했습니다. 작업 폴더와 브랜치를 확인해 주세요"],
  ["agents.cleanup_failed"],
  ['{"code":"agents.cleanup_failed"}'],
  ['{"code":"agents.does_not_exist","detail":{}}'],
  ['{"code":"constructor"}'],
  ['{"detail":{"files":["x"]}}'],
  ['{"code":7}'],
  ['"agents.worktree_dirty"'],
  ["null"],
  ["{not json"],
])(
  "a cleanup error without a known code gets the general cause and action, and its text stays hidden (%s)",
  async (raw) => {
    header(raw);
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("정리하지 못한 항목이 있습니다.");
    expect(alert).toHaveTextContent("원인을 확인하지 못했습니다.");
    expect(alert).toHaveTextContent("설정 → 에이전트 → 정리 후보에서 남은 항목을 확인하세요.");
    expect(document.body).not.toHaveTextContent(raw);
    await userEvent.click(screen.getByRole("button", { name: "자세히" }));
    expect(screen.getByText(raw)).toBeInTheDocument();
  },
);

test("no cleanup notice is shown when nothing was left behind", () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <AgentHeader task={task("a", "closed", 1000)} projectName="fixture" toolName="Codex" now={999999} />
    </RpcProvider>,
  );
  expect(screen.queryByText(/정리하지 못한 항목/)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "자세히" })).not.toBeInTheDocument();
});
