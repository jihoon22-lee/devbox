import { webcrypto } from "node:crypto";
import type { QueryClient } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RpcProvider, useRpcSession } from "../../../rpc/RpcProvider";
import { ProjectWire } from "../../projects/test-wire";
import { ChangesTab } from "./ChangesTab";
import { useSeen } from "./seen";

const file = { path: "a.txt", oldPath: null, status: "modified", added: 1, removed: 1 };
const changes = { base: "main", ahead: 1, behind: 0, headOid: "h1", baseOid: "b1", files: [file] };
const patch = (text: string, truncated = false) => ({
  patch: `diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-before\n+${text}\n`,
  truncated,
});
let qc: QueryClient;
function Capture() {
  qc = useRpcSession().qc;
  return null;
}
beforeEach(() => {
  useSeen.setState({ entries: {} });
  vi.stubGlobal("crypto", webcrypto);
});
afterEach(() => vi.unstubAllGlobals());
async function setup(truncated = false, files = changes.files) {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <Capture />
      <ChangesTab agentId="a" />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(1));
  await act(async () => wire.reply("agents.changes", { ...changes, files }));
  await waitFor(() => expect(wire.requests("agents.patch")).toHaveLength(1));
  await act(async () => wire.reply("agents.patch", patch("first", truncated)));
  await screen.findByText("first");
  return wire;
}
test("new patch content with identical status/counts and HEAD invalidates the seen mark", async () => {
  const wire = await setup();
  await waitFor(() => expect(screen.getByRole("button", { name: "봤음" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "봤음" }));
  await screen.findByRole("button", { name: /✓ 봤음/ });
  act(() => {
    void qc.invalidateQueries({ queryKey: ["agents.patch"] });
  });
  await waitFor(() => expect(wire.requests("agents.patch")).toHaveLength(2));
  await act(async () => wire.reply("agents.patch", patch("other")));
  await screen.findByText("other");
  await waitFor(() => expect(screen.queryByRole("button", { name: /✓ 봤음/ })).not.toBeInTheDocument());
});
test("a new reviewed HEAD invalidates marks even when the cached patch is identical", async () => {
  const wire = await setup();
  await waitFor(() => expect(screen.getByRole("button", { name: "봤음" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "봤음" }));
  await screen.findByRole("button", { name: /✓ 봤음/ });
  act(() => {
    void qc.invalidateQueries({ queryKey: ["agents.changes"] });
  });
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(2));
  await act(async () => wire.reply("agents.changes", { ...changes, headOid: "h2" }));
  await waitFor(() => expect(screen.queryByRole("button", { name: /✓ 봤음/ })).not.toBeInTheDocument());
});
test("a truncated patch cannot be marked fully seen", async () => {
  await setup(true);
  expect(screen.getByRole("button", { name: "봤음" })).toBeDisabled();
  expect(screen.getByText(/일부만 가져왔습니다/)).toBeInTheDocument();
});

test("successful explicit patch retry restores marking without another changes receipt", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <Capture />
      <ChangesTab agentId="a" />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(1));
  await act(async () => wire.reply("agents.changes", changes));
  await waitFor(() => expect(wire.requests("agents.patch")).toHaveLength(1));
  await act(async () => wire.fail("agents.patch"));
  expect(screen.getByRole("button", { name: "봤음" })).toBeDisabled();
  fireEvent.click(await screen.findByRole("button", { name: "다시 확인" }));
  await waitFor(() => expect(wire.requests("agents.patch")).toHaveLength(2));
  await act(async () => wire.reply("agents.patch", patch("first")));
  await screen.findByText("first");
  await waitFor(() => expect(screen.getByRole("button", { name: "봤음" })).toBeEnabled());
  expect(wire.requests("agents.changes")).toHaveLength(1);
});

test("a deferred patch request that predates a changes refresh cannot verify the new snapshot", async () => {
  const wire = await setup();
  await waitFor(() => expect(screen.getByRole("button", { name: "봤음" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "봤음" }));
  await screen.findByRole("button", { name: /✓ 봤음/ });
  act(() => {
    void qc.invalidateQueries({ queryKey: ["agents.patch"] });
  });
  await waitFor(() => expect(wire.requests("agents.patch")).toHaveLength(2));
  const old = wire.request("agents.patch", -1);
  act(() => {
    void qc.invalidateQueries({ queryKey: ["agents.changes"] });
  });
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(2));
  await act(async () => wire.reply("agents.changes", changes));
  await waitFor(() => expect(wire.requests("agents.patch")).toHaveLength(3));
  await act(async () => wire.deliver({ type: "response", id: old.id, result: patch("first") }));
  expect(screen.getByRole("button", { name: "봤음" })).toBeDisabled();
  expect(screen.queryByRole("button", { name: /✓ 봤음/ })).not.toBeInTheDocument();
  await act(async () => wire.reply("agents.patch", patch("other")));
  await screen.findByText("other");
  await waitFor(() => expect(screen.getByRole("button", { name: "봤음" })).toBeEnabled());
  expect(screen.queryByRole("button", { name: /✓ 봤음/ })).not.toBeInTheDocument();
});

test("stable snapshot preserves earlier marks; refresh requires selecting off-screen content again without query fan-out", async () => {
  vi.spyOn(Date, "now").mockReturnValue(1000);
  const wire = await setup(false, [file, { ...file, path: "b.txt" }]);
  await waitFor(() => expect(screen.getByRole("button", { name: "봤음" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "봤음" }));
  await screen.findByRole("button", { name: /a.txt.*✓ 봤음/ });
  await waitFor(() =>
    expect(wire.requests("agents.patch").some((request) => (request.params as { path: string }).path === "b.txt")).toBe(
      true,
    ),
  );
  await act(async () => wire.reply("agents.patch", patch("second")));
  await screen.findByText("second");
  expect(screen.getByRole("button", { name: /a.txt.*✓ 봤음/ })).toBeInTheDocument();
  act(() => {
    void qc.invalidateQueries({ queryKey: ["agents.changes"] });
  });
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(2));
  await act(async () => wire.reply("agents.changes", { ...changes, files: [file, { ...file, path: "b.txt" }] }));
  await waitFor(() =>
    expect(
      wire.requests("agents.patch").filter((request) => (request.params as { path: string }).path === "b.txt"),
    ).toHaveLength(2),
  );
  await act(async () => wire.reply("agents.patch", patch("second")));
  await waitFor(() => expect(screen.getByRole("button", { name: "봤음" })).toBeEnabled());
  await waitFor(() => expect(screen.queryByRole("button", { name: /a.txt.*✓ 봤음/ })).not.toBeInTheDocument());
  expect(
    wire.requests("agents.patch").filter((request) => (request.params as { path: string }).path === "a.txt"),
  ).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: /a.txt/ }));
  await waitFor(() =>
    expect(
      wire.requests("agents.patch").filter((request) => (request.params as { path: string }).path === "a.txt"),
    ).toHaveLength(2),
  );
  const refreshed = wire
    .requests("agents.patch")
    .filter((request) => (request.params as { path: string }).path === "a.txt")
    .at(-1)!;
  await act(async () => wire.deliver({ type: "response", id: refreshed.id, result: patch("other") }));
  await waitFor(() => expect(screen.queryByRole("button", { name: /a.txt.*✓ 봤음/ })).not.toBeInTheDocument());
});
