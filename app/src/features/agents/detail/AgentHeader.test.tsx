import { act, render, screen, waitFor } from "@testing-library/react";
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
    cleanupError: "fixture branch retained",
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
  expect(screen.getByRole("alert")).toHaveTextContent("fixture branch retained");
});
