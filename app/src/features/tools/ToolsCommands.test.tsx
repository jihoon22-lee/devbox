import { act, render, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { useCommands } from "../../shell/commands";
import { useToasts } from "../../ui";
import { ToolsCommands } from "./ToolsCommands";
import { useTransformHandoff } from "./transforms/handoff";

it("registers clipboard command, reports failure safely and unregisters", async () => {
  const readText = vi.fn().mockRejectedValue(new Error("private diagnostic"));
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { readText } });
  const navigate = vi.fn();
  const view = render(<ToolsCommands navigate={navigate} />);
  const command = useCommands.getState().items.get("tools.clipboard");
  expect(command?.label).toBe("클립보드로 변환…");
  await act(async () => command?.run());
  await waitFor(() => expect(useToasts.getState().items.at(-1)?.title).toContain("클립보드"));
  expect(JSON.stringify(useToasts.getState().items)).not.toContain("private diagnostic");
  expect(navigate).not.toHaveBeenCalled();
  view.unmount();
  expect(useCommands.getState().items.has("tools.clipboard")).toBe(false);
});
it("uses clipboard only on command activation and ignores a read resolving after unmount", async () => {
  let resolve!: (text: string) => void;
  const readText = vi.fn(
    () =>
      new Promise<string>((done) => {
        resolve = done;
      }),
  );
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { readText } });
  const navigate = vi.fn();
  const view = render(<ToolsCommands navigate={navigate} />);
  expect(readText).not.toHaveBeenCalled();
  act(() => useCommands.getState().items.get("tools.clipboard")?.run());
  await waitFor(() => expect(readText).toHaveBeenCalledOnce());
  view.unmount();
  await act(async () => resolve('{"private":1}'));
  expect(navigate).not.toHaveBeenCalled();
  expect(useTransformHandoff.getState().pending).toBeNull();
});
it("reads clipboard on demand and hands the text to transforms without putting it in navigation", async () => {
  const readText = vi.fn().mockResolvedValue('{"clipboard":"fixture-value"}');
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { readText } });
  const navigate = vi.fn();
  const view = render(<ToolsCommands navigate={navigate} />);
  expect(readText).not.toHaveBeenCalled();
  act(() => useCommands.getState().items.get("tools.clipboard")?.run());
  await waitFor(() => expect(navigate).toHaveBeenCalledWith({ view: "transforms", tool: "json-format" }));
  expect(useTransformHandoff.getState().pending?.text).toBe('{"clipboard":"fixture-value"}');
  view.unmount();
});
