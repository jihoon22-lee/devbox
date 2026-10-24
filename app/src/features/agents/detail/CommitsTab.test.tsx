import { act, render, screen, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../../rpc/RpcProvider";
import { ProjectWire } from "../../projects/test-wire";
import { CommitsTab } from "./CommitsTab";

test("shared commit review ignores late previous-agent history and never asks live Git", async () => {
  const wire = new ProjectWire();
  const view = (id: string) => (
    <RpcProvider client={wire.client}>
      <CommitsTab agentId={id} />
    </RpcProvider>
  );
  const rendered = render(view("old"));
  await waitFor(() => expect(wire.requests("agents.commits")).toHaveLength(1));
  const old = wire.request("agents.commits").id;
  expect(screen.queryByRole("region", { name: "커밋 기록" })).toBeNull();
  expect(wire.requests("git.log")).toHaveLength(0);
  rendered.rerender(view("new"));
  await waitFor(() => expect(wire.requests("agents.commits")).toHaveLength(2));
  await act(async () => {
    wire.deliver({
      type: "response",
      id: old,
      result: { items: [{ sha: "old", subject: "old subject", author: "old author", timeMs: 0 }] },
    });
    wire.reply("agents.commits", {
      items: [{ sha: "1234567890123456", subject: "closed history", author: "review author", timeMs: 0 }],
    });
  });
  expect(await screen.findByRole("region", { name: "커밋 기록" })).toHaveTextContent("closed history");
  expect(screen.queryByText(/old subject/)).toBeNull();
  expect(screen.queryAllByRole("button")).toHaveLength(0);
  expect(wire.requests("git.log")).toHaveLength(0);
});
test("empty agent commit snapshot renders the historical empty state with no fallback", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <CommitsTab agentId="closed" />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.commits")).toHaveLength(1));
  await act(async () => wire.reply("agents.commits", { items: [] }));
  expect(await screen.findByText("아직 커밋이 없습니다")).toBeVisible();
  expect(screen.getByRole("region", { name: "커밋 기록" })).toBeVisible();
  expect(wire.requests("git.log")).toHaveLength(0);
});
