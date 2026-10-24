import { beforeEach, expect, it, vi } from "vitest";
import { useToasts } from "../../../ui";
import { MAX_HANDOFF_BYTES, registerTransformsNavigation, sendToTransforms, useTransformHandoff } from "./handoff";

beforeEach(() => {
  useTransformHandoff.setState({ pending: null });
  useToasts.setState({ items: [] });
});
it("hands input to memory and navigates using only a tool ID", async () => {
  const navigate = vi.fn();
  const stop = registerTransformsNavigation(navigate);
  const writes = vi.spyOn(Storage.prototype, "setItem");
  await sendToTransforms('{"password":"fixture-private"}', "log");
  expect(useTransformHandoff.getState().pending).toMatchObject({
    text: '{"password":"fixture-private"}',
    source: "log",
    toolId: "json-format",
  });
  expect(navigate).toHaveBeenCalledWith({ view: "transforms", tool: "json-format" });
  expect(JSON.stringify(writes.mock.calls)).not.toContain("fixture-private");
  stop();
  writes.mockRestore();
});
it("truncates on a Unicode boundary at512KiB and gives a fixed toast", async () => {
  const stop = registerTransformsNavigation(vi.fn());
  await sendToTransforms("😀".repeat(MAX_HANDOFF_BYTES), "clipboard");
  const text = useTransformHandoff.getState().pending?.text ?? "";
  expect(new TextEncoder().encode(text).length).toBe(MAX_HANDOFF_BYTES);
  expect(text.endsWith("😀")).toBe(true);
  expect(useToasts.getState().items.at(-1)?.title).toContain("512KiB");
  stop();
});
it("clears input and reports navigation failure without raw diagnostics", async () => {
  const stop = registerTransformsNavigation(() => {
    throw new Error("fixture-secret");
  });
  await sendToTransforms("fixture-secret", "terminal");
  expect(useTransformHandoff.getState().pending).toBeNull();
  expect(JSON.stringify(useToasts.getState().items)).not.toContain("fixture-secret");
  stop();
});
it("keeps a partial UTF-8 boundary out of the transferred prefix", async () => {
  const stop = registerTransformsNavigation(vi.fn());
  const text = "a".repeat(MAX_HANDOFF_BYTES - 1) + "한";
  await sendToTransforms(text, "log");
  expect(useTransformHandoff.getState().pending?.text).toBe("a".repeat(MAX_HANDOFF_BYTES - 1));
  stop();
});
it("does not silently rewrite an in-range lone surrogate", async () => {
  const stop = registerTransformsNavigation(vi.fn());
  await sendToTransforms("broken\ud800", "log");
  expect(useTransformHandoff.getState().pending?.text).toBe("broken\ud800");
  expect(useToasts.getState().items).toHaveLength(0);
  stop();
});
