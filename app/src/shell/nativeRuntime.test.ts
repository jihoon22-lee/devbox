import { act } from "@testing-library/react";
import { consumeNativeIntents, useNativeRuntime } from "./nativeRuntime";

beforeEach(() => useNativeRuntime.setState({ intents: [], openTasks: null, bottomFocus: 0 }));
test("native intents survive gate startup and wait for modal dismissal without disturbing its input", async () => {
  useNativeRuntime.getState().push({ kind: "palette" });
  const dialog = document.createElement("div");
  dialog.setAttribute("role", "dialog");
  document.body.append(dialog);
  const run = vi.fn();
  const off = consumeNativeIntents(run);
  expect(run).not.toHaveBeenCalled();
  await act(async () => dialog.remove());
  expect(run).toHaveBeenCalledExactlyOnceWith({ kind: "palette" });
  expect(useNativeRuntime.getState().intents).toHaveLength(0);
  off();
  useNativeRuntime.getState().push({ kind: "bottom-terminal", wasFocused: false });
  expect(run).toHaveBeenCalledTimes(1);
});
