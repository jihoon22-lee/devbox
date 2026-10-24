import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { streamFrame } from "../../rpc/frame";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectWire } from "../projects/test-wire";
import { registerTransformsNavigation, useTransformHandoff } from "../tools/transforms/handoff";
import { LogView } from "./LogView";

it("sends only the selected log text through the context menu", async () => {
  const wire = new ProjectWire();
  const navigate = vi.fn();
  const stop = registerTransformsNavigation(navigate);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(320);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(900);
  Object.defineProperty(HTMLElement.prototype, "scrollTo", { configurable: true, value: vi.fn() });
  const source = { kind: "run" as const, runId: "selection-fixture" };
  render(
    <RpcProvider client={wire.client}>
      <LogView sources={[source]} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(1));
  await act(async () => wire.reply("logs.open", { streamId: 7 }));
  act(() => {
    const bytes = new TextEncoder().encode(
      `${JSON.stringify({ src: source, ts: "2026-10-24T00:00:00Z", stream: "stdout", text: '{"selected":true}' })}\n`,
    );
    for (const cb of wire.frames) cb(streamFrame(7, 0, bytes));
  });
  const text = await screen.findByText('{"selected":true}');
  const range = document.createRange();
  range.selectNodeContents(text);
  window.getSelection()?.removeAllRanges();
  window.getSelection()?.addRange(range);
  fireEvent.contextMenu(text);
  fireEvent.click(screen.getByRole("menuitem", { name: "변환 도구로 보내기" }));
  expect(useTransformHandoff.getState().pending).toMatchObject({ text: '{"selected":true}', source: "log" });
  expect(navigate).toHaveBeenCalledWith({ view: "transforms", tool: "json-format" });
  window.getSelection()?.removeAllRanges();
  fireEvent.contextMenu(screen.getByRole("log"));
  expect(screen.queryByRole("menu")).toBeNull();
  stop();
});
