import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { RpcError } from "../../rpc/client";
import { RpcProvider } from "../../rpc/RpcProvider";
import { InsertSummary } from "./InsertSummary";
import { NotesWire } from "./test-wire";

afterEach(cleanup);

test("today's summary calls the daemon once while pending and opens the returned daily path", async () => {
  const wire = new NotesWire();
  let finish!: (value: unknown) => void;
  wire.handlers["notes.insert_summary"] = () =>
    new Promise((resolve) => {
      finish = resolve;
    });
  const open = vi.fn();
  render(
    <RpcProvider client={wire.client}>
      <InsertSummary onOpen={open} />
    </RpcProvider>,
  );
  const button = await screen.findByRole("button", { name: "오늘 한 일 넣기" });
  await waitFor(() => expect(button).toBeEnabled());
  await userEvent.click(button);
  expect(button).toBeDisabled();
  expect(wire.requests("notes.insert_summary")).toEqual([{ date: null }]);
  expect(open).not.toHaveBeenCalled();
  finish({ path: "daily/2026-10-24.md", rev: { hash: "fixture", mtimeNs: 1, size: 10 } });
  await waitFor(() => expect(open).toHaveBeenCalledWith("daily/2026-10-24.md"));
  expect(button).toBeEnabled();
});

test("failure stays visible, preserves the selected note and allows retry", async () => {
  const wire = new NotesWire();
  wire.handlers["notes.insert_summary"] = () => {
    throw new RpcError("notes.busy");
  };
  const open = vi.fn();
  render(
    <RpcProvider client={wire.client}>
      <InsertSummary onOpen={open} />
    </RpcProvider>,
  );
  const button = await screen.findByRole("button", { name: "오늘 한 일 넣기" });
  await waitFor(() => expect(button).toBeEnabled());
  await userEvent.click(button);
  expect(await screen.findByRole("alert")).toHaveTextContent(/./);
  expect(open).not.toHaveBeenCalled();
  expect(button).toBeEnabled();
});
