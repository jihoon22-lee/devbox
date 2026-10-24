import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { expect, it, vi } from "vitest";
import { useCommands } from "../../../shell/commands";
import type { ToolsSearch } from "../search";
import { ToolsCommands } from "../ToolsCommands";
import { TransformsPage } from "./TransformsPage";

it("an older clipboard read cannot overwrite newer transferred and edited input", async () => {
  const resolvers: Array<(text: string) => void> = [];
  const readText = vi.fn(() => new Promise<string>((resolve) => resolvers.push(resolve)));
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { readText } });
  function Harness() {
    const [search, setSearch] = useState<ToolsSearch>({ view: "transforms", tool: "json-format" });
    return (
      <>
        <ToolsCommands navigate={setSearch} />
        <TransformsPage toolId={search.tool} onSelect={(tool) => setSearch({ view: "transforms", tool })} />
      </>
    );
  }
  const view = render(<Harness />);
  act(() => useCommands.getState().items.get("tools.clipboard")?.run());
  await waitFor(() => expect(readText).toHaveBeenCalledTimes(1));
  act(() => useCommands.getState().items.get("tools.clipboard")?.run());
  await waitFor(() => expect(readText).toHaveBeenCalledTimes(2));
  await act(async () => resolvers[1]!('{"newer":2}'));
  expect(screen.getByRole("textbox", { name: "입력" })).toHaveValue('{"newer":2}');
  fireEvent.change(screen.getByRole("textbox", { name: "입력" }), { target: { value: '{"user_edit":3}' } });
  await act(async () => resolvers[0]!('{"older":1}'));
  expect(screen.getByRole("textbox", { name: "입력" })).toHaveValue('{"user_edit":3}');
  view.unmount();
});
it.each(["log", "terminal"] as const)(
  "a newer %s handoff invalidates an outstanding clipboard read",
  async (source) => {
    const { sendToTransforms } = await import("./handoff");
    let resolve!: (text: string) => void;
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        readText: vi.fn(
          () =>
            new Promise<string>((done) => {
              resolve = done;
            }),
        ),
      },
    });
    function Harness() {
      const [search, setSearch] = useState<ToolsSearch>({ view: "transforms", tool: "json-format" });
      return (
        <>
          <ToolsCommands navigate={setSearch} />
          <TransformsPage toolId={search.tool} onSelect={() => {}} />
        </>
      );
    }
    const view = render(<Harness />);
    act(() => useCommands.getState().items.get("tools.clipboard")?.run());
    await waitFor(() => expect(resolve).toBeDefined());
    await act(async () => sendToTransforms('{"selection":2}', source));
    fireEvent.change(screen.getByRole("textbox", { name: "입력" }), { target: { value: '{"user_edit":3}' } });
    await act(async () => resolve('{"older":1}'));
    expect(screen.getByRole("textbox", { name: "입력" })).toHaveValue('{"user_edit":3}');
    view.unmount();
  },
);
it("a superseded clipboard rejection cannot report a stale error", async () => {
  const { useToasts } = await import("../../../ui");
  useToasts.setState({ items: [] });
  const requests: Array<{ resolve(text: string): void; reject(error: Error): void }> = [];
  const readText = vi.fn(() => new Promise<string>((resolve, reject) => requests.push({ resolve, reject })));
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { readText } });
  const view = render(<ToolsCommands navigate={vi.fn()} />);
  act(() => useCommands.getState().items.get("tools.clipboard")?.run());
  await waitFor(() => expect(requests).toHaveLength(1));
  act(() => useCommands.getState().items.get("tools.clipboard")?.run());
  await waitFor(() => expect(requests).toHaveLength(2));
  await act(async () => requests[1]!.resolve('{"newer":2}'));
  await act(async () => requests[0]!.reject(new Error("stale private error")));
  expect(useToasts.getState().items).toHaveLength(0);
  view.unmount();
});
