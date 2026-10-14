import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectWire } from "../projects/test-wire";
import { AgentRow } from "./AgentRow";
import { task } from "./test-fixtures";

test("selected row uses actual resource/token fields and hides unavailable metrics", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <AgentRow
        task={{ ...task("r"), tokens: { input: 10, output: 20, cacheRead: 30, cacheWrite: 40 }, tokensTruncated: true }}
        selected
        projectName="devbox"
        now={60001}
        stats={{ id: "r", changedFiles: 2, ahead: 1, behind: 3 }}
        resources={{ id: "r", cpuPercent: null, memoryBytes: 1048576 * 64 }}
        onSelect={() => {}}
      />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  expect(screen.getByRole("button", { current: true })).toHaveTextContent("작업 r");
  expect(screen.getByText("변경 2")).toBeInTheDocument();
  expect(screen.getByText("뒤처짐 3")).toBeInTheDocument();
  expect(screen.getByText("메모리 64 MB")).toBeInTheDocument();
  expect(screen.getByText("입력 토큰 10")).toBeInTheDocument();
  expect(screen.getByText("출력 토큰 20")).toBeInTheDocument();
  expect(screen.getByText("캐시 읽기 토큰 30")).toBeInTheDocument();
  expect(screen.getByText("캐시 쓰기 토큰 40")).toBeInTheDocument();
  expect(screen.getByText("토큰 기록 일부")).toBeInTheDocument();
  expect(screen.queryByText("토큰 100 (일부)")).toBeNull();
  expect(screen.queryByText(/CPU/)).toBeNull();
});

test("external kill displays explicit resume, typed failure then confirmed retry", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <AgentRow
        task={{ ...task("f", "failed"), reason: "killed:9" }}
        selected={false}
        projectName="devbox"
        now={1000}
        onSelect={() => {}}
      />
    </RpcProvider>,
  );
  expect(screen.getByText(/외부에서 종료됨/)).toBeInTheDocument();
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  await userEvent.click(screen.getByRole("button", { name: "재개" }));
  await waitFor(() => expect(wire.requests("agents.resume")).toHaveLength(1));
  expect(wire.request("agents.resume").params).toEqual({ id: "f" });
  await act(async () =>
    wire.deliver({ type: "response", id: wire.request("agents.resume").id, error: { code: "agents.session_missing" } }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent("재개");
  await userEvent.click(screen.getByRole("button", { name: "재개" }));
  await waitFor(() => expect(wire.requests("agents.resume")).toHaveLength(2));
  await act(async () => wire.reply("agents.resume", task("f")));
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
});

test.each(["codex", "removed-profile"])(
  "%s preserves raw cached-subset counts without inventing a provider total",
  async (profile) => {
    const wire = new ProjectWire();
    render(
      <RpcProvider client={wire.client}>
        <AgentRow
          task={{ ...task("cached"), profile, tokens: { input: 9, output: 3, cacheRead: 4, cacheWrite: 0 } }}
          selected={false}
          projectName="devbox"
          now={1000}
          onSelect={() => {}}
        />
      </RpcProvider>,
    );
    await waitFor(() => expect(wire.client.state).toBe("ready"));
    expect(screen.getByText("입력 토큰 9")).toBeInTheDocument();
    expect(screen.getByText("출력 토큰 3")).toBeInTheDocument();
    expect(screen.getByText("캐시 읽기 토큰 4")).toBeInTheDocument();
    expect(screen.getByText("캐시 쓰기 토큰 0")).toBeInTheDocument();
    expect(screen.queryByText("토큰 16")).toBeNull();
    expect(screen.queryByText("토큰 기록 일부")).toBeNull();
  },
);

test("unknown token usage stays hidden even when the record is truncated", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <AgentRow
        task={{ ...task("unknown"), profile: "removed-profile", tokens: null, tokensTruncated: true }}
        selected={false}
        projectName="devbox"
        now={1000}
        onSelect={() => {}}
      />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  expect(screen.queryByText(/토큰/)).toBeNull();
});
