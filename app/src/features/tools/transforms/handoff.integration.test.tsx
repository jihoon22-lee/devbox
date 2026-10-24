import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { StrictMode, useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { ToolId } from "./catalog";
import { registerTransformsNavigation, sendToTransforms, useTransformHandoff } from "./handoff";
import { TransformsPage } from "./TransformsPage";

function Harness() {
  const [tool, setTool] = useState<ToolId>("json-format");
  return <TransformsPage toolId={tool} onSelect={setTool} />;
}
afterEach(() => useTransformHandoff.setState({ pending: null }));
it("consumes handoff in StrictMode, initializes the formatter and drops input on tool changes", async () => {
  const stop = registerTransformsNavigation(vi.fn());
  await sendToTransforms('{"private":"fixture-value"}', "log");
  const writes = vi.spyOn(Storage.prototype, "setItem");
  const view = render(
    <StrictMode>
      <Harness />
    </StrictMode>,
  );
  expect(screen.getByRole("textbox", { name: "입력" })).toHaveValue('{"private":"fixture-value"}');
  await waitFor(() => expect(screen.getByLabelText("출력").textContent).toContain('"private": "fixture-value"'));
  expect(useTransformHandoff.getState().pending).toBeNull();
  fireEvent.change(screen.getByRole("combobox", { name: "변환 도구" }), { target: { value: "case" } });
  expect(screen.getByRole("textbox", { name: "입력" })).toHaveValue("");
  fireEvent.change(screen.getByRole("combobox", { name: "변환 도구" }), { target: { value: "json-format" } });
  expect(screen.getByRole("textbox", { name: "입력" })).toHaveValue("");
  expect(JSON.stringify(writes.mock.calls)).not.toContain("fixture-value");
  view.unmount();
  stop();
  writes.mockRestore();
});
it("updates an already-mounted tool on a new handoff and does not restore it on remount", async () => {
  const stop = registerTransformsNavigation(vi.fn());
  const view = render(<TransformsPage toolId="json-format" onSelect={vi.fn()} />);
  await act(async () => sendToTransforms('{"n":2}', "clipboard"));
  expect(screen.getByRole("textbox", { name: "입력" })).toHaveValue('{"n":2}');
  view.unmount();
  render(<TransformsPage toolId="json-format" onSelect={vi.fn()} />);
  expect(screen.getByRole("textbox", { name: "입력" })).toHaveValue("");
  stop();
});
it("initializes detected Base64 decoding instead of encoding the source again", async () => {
  const stop = registerTransformsNavigation(vi.fn());
  await sendToTransforms("SGVsbG8=", "terminal");
  render(<TransformsPage toolId="byte-codec" onSelect={vi.fn()} />);
  expect(screen.getByLabelText("Base64 입력")).toHaveValue("SGVsbG8=");
  expect(screen.getByLabelText("UTF-8 텍스트 출력").textContent).toBe("Hello");
  stop();
});
it("does not restore transferred input after navigation outside the tool picker", async () => {
  const stop = registerTransformsNavigation(vi.fn());
  await sendToTransforms('{"private":"navigation-fixture"}', "log");
  const view = render(<TransformsPage toolId="json-format" onSelect={vi.fn()} />);
  expect(screen.getByRole("textbox", { name: "입력" })).toHaveValue('{"private":"navigation-fixture"}');
  view.rerender(<TransformsPage toolId="case" onSelect={vi.fn()} />);
  view.rerender(<TransformsPage toolId="json-format" onSelect={vi.fn()} />);
  expect(screen.getByRole("textbox", { name: "입력" })).toHaveValue("");
  stop();
});
