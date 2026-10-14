import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { RpcProvider, useRpcSession } from "../../rpc/RpcProvider";
import { ProjectWire } from "../projects/test-wire";
import { type Session, useSessions } from "./useSessions";

const session = (id: string): Session => ({
  id,
  projectId: "p",
  title: id,
  cwd: "/fixture",
  command: "bash",
  kind: "shell",
  alive: false,
  createdMs: 1,
});
async function setup() {
  const wire = new ProjectWire();
  const wrapper = ({ children }: { children: ReactNode }) => <RpcProvider client={wire.client}>{children}</RpcProvider>;
  const hook = renderHook(() => ({ list: useSessions("p"), qc: useRpcSession().qc }), { wrapper });
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(1));
  return { wire, ...hook };
}

test("terminal pages publish every row only after a revision-consistent traversal", async () => {
  const { wire, result } = await setup();
  await act(async () => wire.reply("terminal.list", { rev: "1", items: [session("a")], nextAfterId: "1:a" }));
  expect(result.current.list.items).toEqual([]);
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(2));
  expect(wire.requests("terminal.list")[1]!.params).toEqual({ projectId: "p", afterId: "1:a" });
  await act(async () => wire.reply("terminal.list", { rev: "2", items: [session("b")] }));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(3));
  expect(wire.requests("terminal.list")[2]!.params).toEqual({ projectId: "p" });
  await act(async () => wire.reply("terminal.list", { rev: "2", items: [session("c")], nextAfterId: "1:c" }));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(4));
  await act(async () => wire.reply("terminal.list", { rev: "2", items: [session("d")] }));
  await waitFor(() => expect(result.current.list.items.map((item) => item.id)).toEqual(["c", "d"]));
  expect(result.current.list.data?.rev).toBe("2");
});

test("repeated terminal cursors fail without publishing a partial list", async () => {
  const { wire, result } = await setup();
  await act(async () => wire.reply("terminal.list", { rev: "1", items: [session("a")], nextAfterId: "1:a" }));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(2));
  await act(async () => wire.reply("terminal.list", { rev: "1", items: [session("b")], nextAfterId: "1:a" }));
  await waitFor(() => expect(result.current.list.error?.code).toBe("terminal.list_changed"));
  expect(result.current.list.items).toEqual([]);
  expect(wire.requests("terminal.list")).toHaveLength(2);
});

test("terminal revision drift stops after three traversals", async () => {
  const { wire, result } = await setup();
  for (let attempt = 0; attempt < 3; attempt++) {
    await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(attempt * 2 + 1));
    await act(async () =>
      wire.reply("terminal.list", {
        rev: String(attempt * 2 + 1),
        items: [session("a")],
        nextAfterId: "1:a",
      }),
    );
    await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(attempt * 2 + 2));
    await act(async () => wire.reply("terminal.list", { rev: String(attempt * 2 + 2), items: [session("b")] }));
  }
  await waitFor(() => expect(result.current.list.error?.code).toBe("terminal.list_changed"));
  expect(result.current.list.items).toEqual([]);
  expect(wire.requests("terminal.list")).toHaveLength(6);
});

test("cancelling a terminal page traversal keeps the query cache free of partial rows", async () => {
  const { wire, result } = await setup();
  await act(async () => wire.reply("terminal.list", { rev: "1", items: [session("a")], nextAfterId: "1:a" }));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(2));
  await act(async () => result.current.qc.cancelQueries({ queryKey: ["terminal.list"] }));
  await act(async () => wire.reply("terminal.list", { rev: "1", items: [session("b")] }));
  expect(result.current.qc.getQueryData(["terminal.list", { projectId: "p" }])).toBeUndefined();
  expect(result.current.list.items).toEqual([]);
});
