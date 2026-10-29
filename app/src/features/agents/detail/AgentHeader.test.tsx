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

test("cleanup_error_shows_cause_and_next_action", () => {
  const raw = JSON.stringify({ code: "agents.worktree_dirty", detail: { files: ["retained.txt"] } });
  header(raw);
  const alert = screen.getByRole("alert");
  expect(alert).toHaveTextContent("정리하지 못한 항목이 있습니다.");
  // The fixed message of the code: what stopped it, and what to do.
  expect(alert).toHaveTextContent("작업 폴더에 커밋하지 않은 변경이 있습니다.");
  expect(alert).toHaveTextContent("변경을 확인하고 커밋한 뒤 다시 시도해 주세요.");
  // A code has its message. The stored text is not shown anywhere, not even behind [자세히].
  expect(screen.queryByRole("button", { name: "자세히" })).not.toBeInTheDocument();
  expect(document.body).not.toHaveTextContent("agents.worktree_dirty");
  expect(document.body).not.toHaveTextContent("retained.txt");
});

test("a cleanup error's detail reaches the message of its code", () => {
  header(JSON.stringify({ code: "agents.base_dirty", detail: { files: ["a.txt", "b.txt"] } }));
  expect(screen.getByRole("alert")).toHaveTextContent("기준 체크아웃에 커밋하지 않은 변경이 있어 병합하지 않았습니다.");
  expect(screen.getByRole("alert")).toHaveTextContent("변경 파일: a.txt, b.txt");
  expect(screen.queryByRole("button", { name: "자세히" })).not.toBeInTheDocument();
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
    expect(alert).toHaveTextContent("작업 폴더와 브랜치가 남아 있는지 확인해 주세요.");
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
