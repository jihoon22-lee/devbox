import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../../rpc/RpcProvider";
import { TooltipProvider } from "../../../ui";
import { ProjectWire } from "../../projects/test-wire";
import { task } from "../test-fixtures";
import { AgentActions } from "./AgentActions";
import { ChangesTab } from "./ChangesTab";
import { CommitsTab } from "./CommitsTab";

const changes = {
  base: "main",
  ahead: 1,
  behind: 0,
  headOid: "h1",
  baseOid: "b1",
  files: [{ path: "a.txt", oldPath: null, status: "modified", added: 1, removed: 1 }],
};
const patch = (text: string) => ({
  patch: `diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-before\n+${text}\n`,
  truncated: false,
});
const commits = (subject: string) => ({ items: [{ sha: "abc123", subject, author: "fixture", timeMs: 1 }] });
async function setup(kind: "merge" | "discard") {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <AgentActions task={task("a1", "idle")} sessionAlive />
        <ChangesTab agentId="a1" />
        <CommitsTab agentId="a1" />
        <ChangesTab agentId="unrelated" />
      </TooltipProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(2));
  const initial = wire.requests("agents.changes").find((request) => (request.params as { id: string }).id === "a1")!;
  await act(async () => {
    wire.deliver({ type: "response", id: initial.id, result: changes });
    wire.reply("agents.commits", commits("old-commit"));
  });
  await waitFor(() => expect(wire.requests("agents.patch")).toHaveLength(1));
  await act(async () => wire.reply("agents.patch", patch("old-reviewed-text")));
  await screen.findByText("old-reviewed-text");
  await userEvent.click(screen.getByRole("button", { name: kind === "merge" ? "병합" : "더 보기" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: kind === "merge" ? "병합 커밋" : "폐기" }));
  if (kind === "merge") {
    await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(3));
    await act(async () => wire.reply("agents.changes", changes));
  } else {
    await waitFor(() => expect(wire.requests("agents.discard_check")).toHaveLength(1));
    await act(async () => wire.reply("agents.discard_check", { headOid: "h1", unmergedCommits: 1, dirtyFiles: 1 }));
  }
  const label = kind === "merge" ? "main에 병합" : "작업 폴더와 브랜치 삭제";
  await waitFor(() => expect(screen.getByRole("button", { name: label })).toBeEnabled());
  await userEvent.click(screen.getByRole("button", { name: label }));
  const method = kind === "merge" ? "agents.merge" : "agents.discard";
  await waitFor(() => expect(wire.requests(method)).toHaveLength(1));
  await act(async () =>
    wire.deliver({
      type: "response",
      id: wire.request(method).id,
      error: { code: "agents.state_changed", detail: { head: "h2", base: "b2", unmerged: 2 } },
    }),
  );
  return { wire, method, label };
}
test.each(["merge", "discard"] as const)(
  "%s state mismatch refreshes only its review data and requires a fresh explicit confirmation",
  async (kind) => {
    const { wire, method, label } = await setup(kind);
    await waitFor(() => expect(wire.requests("agents.patch")).toHaveLength(2));
    await waitFor(() => expect(wire.requests("agents.commits")).toHaveLength(2));
    const count = kind === "merge" ? 4 : 3;
    await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(count));
    if (kind === "discard") await waitFor(() => expect(wire.requests("agents.discard_check")).toHaveLength(2));
    await act(async () => {
      wire.reply("agents.changes", { ...changes, headOid: "h2", baseOid: "b2", ahead: 2 });
      wire.reply("agents.commits", commits("new-commit"));
      if (kind === "discard") wire.reply("agents.discard_check", { headOid: "h2", unmergedCommits: 2, dirtyFiles: 1 });
    });
    expect(screen.getByRole("button", { name: label })).toBeDisabled();
    expect(wire.requests(method)).toHaveLength(1);
    expect(
      wire.requests("agents.changes").filter((request) => (request.params as { id: string }).id === "unrelated"),
    ).toHaveLength(1);
    await act(async () => wire.reply("agents.patch", patch("new-reviewed-text")));
    await waitFor(() => expect(screen.getByRole("button", { name: label })).toBeEnabled());
    expect(await screen.findByText(/HEAD h2/)).toBeInTheDocument();
    expect(screen.getByText("new-reviewed-text")).toBeInTheDocument();
    expect(screen.queryByText("old-reviewed-text")).toBeNull();
    expect(wire.requests(method)).toHaveLength(1);
    await userEvent.click(screen.getByRole("button", { name: label }));
    await waitFor(() => expect(wire.requests(method)).toHaveLength(2));
    expect(wire.request(method, -1).params).toMatchObject({
      expect: kind === "merge" ? { headOid: "h2", baseOid: "b2" } : { headOid: "h2", unmergedCommits: 2 },
    });
  },
);
test("failed mismatch patch refresh keeps confirmation blocked until an explicit successful retry", async () => {
  const { wire, method, label } = await setup("merge");
  await waitFor(() => expect(wire.requests("agents.patch")).toHaveLength(2));
  await act(async () => {
    wire.reply("agents.changes", { ...changes, headOid: "h2", baseOid: "b2" });
    wire.reply("agents.commits", commits("new-commit"));
    wire.fail("agents.patch");
  });
  const retry = await screen.findByRole("button", { name: "다시 확인" });
  expect(screen.getByRole("button", { name: label })).toBeDisabled();
  expect(wire.requests(method)).toHaveLength(1);
  await userEvent.click(retry);
  await waitFor(() => expect(wire.requests("agents.patch")).toHaveLength(3));
  await act(async () => {
    wire.reply("agents.changes", { ...changes, headOid: "h3", baseOid: "b3" });
    wire.reply("agents.commits", commits("latest-commit"));
    wire.reply("agents.patch", patch("latest-reviewed-text"));
  });
  await waitFor(() => expect(screen.getByRole("button", { name: label })).toBeEnabled());
  expect(wire.requests(method)).toHaveLength(1);
  await userEvent.click(screen.getByRole("button", { name: label }));
  await waitFor(() => expect(wire.requests(method)).toHaveLength(2));
  expect(wire.request(method, -1).params).toMatchObject({ expect: { headOid: "h3", baseOid: "b3" } });
});
