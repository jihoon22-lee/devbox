import { focusManager } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectWire } from "../projects/test-wire";
import { type Session, useSessions } from "./useSessions";

const session = (alive: boolean, command: string): Session => ({
  id: "owned",
  projectId: "p",
  title: "fixture",
  cwd: "/fixture",
  command,
  kind: "shell",
  alive,
  createdMs: 1,
});
async function setup() {
  const wire = new ProjectWire();
  const wrapper = ({ children }: { children: ReactNode }) => <RpcProvider client={wire.client}>{children}</RpcProvider>;
  const hook = renderHook(({ enabled }) => useSessions("p", { enabled }), { wrapper, initialProps: { enabled: true } });
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(1));
  await act(async () => wire.reply("terminal.list", { rev: "7", items: [session(true, "bash")] }));
  await waitFor(() => expect(hook.result.current.items[0]?.alive).toBe(true));
  return { wire, ...hook };
}
afterEach(() => {
  vi.useRealTimers();
  focusManager.setFocused(undefined);
});
test("refetch commits external exit and command observations at the same database revision", async () => {
  const { wire, result } = await setup();
  let pending!: ReturnType<typeof result.current.refetch>;
  await act(async () => {
    pending = result.current.refetch();
  });
  await act(async () => {
    wire.reply("terminal.list", { rev: "7", items: [session(true, "vim")] });
    await pending;
  });
  await waitFor(() => expect(result.current.items[0]?.command).toBe("vim"));
  await act(async () => {
    pending = result.current.refetch();
  });
  await act(async () => {
    wire.reply("terminal.list", { rev: "7", items: [session(false, "bash")] });
    await pending;
  });
  await waitFor(() => expect(result.current.items[0]?.alive).toBe(false));
  await act(async () => {
    pending = result.current.refetch();
  });
  await act(async () => {
    wire.reply("terminal.list", { rev: "6", items: [session(true, "old")] });
    await pending;
  });
  expect(result.current.data?.rev).toBe("7");
  expect(result.current.items[0]?.alive).toBe(false);
});
test("mounted visible sessions refresh external tmux state within the bounded interval", async () => {
  const { wire, result, unmount, rerender } = await setup();
  vi.useFakeTimers();
  rerender({ enabled: false });
  rerender({ enabled: true });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2100);
  });
  expect(wire.requests("terminal.list")).toHaveLength(2);
  await act(async () => {
    wire.reply("terminal.list", { rev: "7", items: [session(false, "bash")] });
    await vi.advanceTimersByTimeAsync(20);
  });
  expect(result.current.items[0]?.alive).toBe(false);
  unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(wire.requests("terminal.list")).toHaveLength(2);
});
test("returning to a visible window refreshes a mounted terminal query", async () => {
  const { wire, result } = await setup();
  await act(async () => focusManager.setFocused(false));
  await act(async () => focusManager.setFocused(true));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(2));
  await act(async () => wire.reply("terminal.list", { rev: "7", items: [session(false, "bash")] }));
  await waitFor(() => expect(result.current.items[0]?.alive).toBe(false));
});
