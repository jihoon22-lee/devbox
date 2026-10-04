import { afterEach, expect, it, vi } from "vitest";
import {
  currentDescription,
  fixtureDescription,
  publishDescription,
  resetDescriptionCache,
} from "@devbox/product-shell/api";
import type { Registry } from "../RegistryGate";
import { nativeCall } from "../native";
import { nativePorts } from "./api";
import { selectContext } from "./agentFlow";
vi.mock("../native", () => ({ nativeCall: vi.fn() }));
const base = { projectId: "p1", worktreeId: "w1", revision: 1, target: { kind: "wsl" as const, distroId: "d1" } };
const target = { ...base, worktreeId: "w2" };
afterEach(() => {
  resetDescriptionCache();
  vi.clearAllMocks();
});
function setup(failRefresh = false) {
  let nativeContext = base;
  const description = fixtureDescription("workspace");
  publishDescription({ ...description, context: base });
  vi.mocked(nativeCall).mockImplementation(async (_component, method, args) => {
    // Session::authorize checks the cached request context for registry reads too.
    if ((await currentDescription("workspace")).context?.worktreeId !== nativeContext.worktreeId)
      throw new Error("stale_context");
    if (method === "select_project") nativeContext = args?.context as typeof base;
    return { context: nativeContext, projects: [], worktrees: [], revision: 1 };
  });
  const refreshRegistry = vi.fn(() => nativeCall<Registry>("workspace.registry", "snapshot"));
  const refreshContext = vi.fn(async () => {
    if (failRefresh) throw new Error("description unavailable");
    publishDescription({ ...description, context: nativeContext });
  });
  const ports = nativePorts(refreshContext, "owned-installation", undefined, refreshRegistry);
  return { ports, refreshRegistry };
}
it("publishes the new description before a native registry read after selection", async () => {
  const { ports, refreshRegistry } = setup();
  await selectContext(ports, target);
  expect((await currentDescription("workspace")).context).toEqual(target);
  expect(refreshRegistry).toHaveBeenCalledTimes(1);
});
it("does not send a stale registry read when description refresh fails", async () => {
  const { ports, refreshRegistry } = setup(true);
  await expect(selectContext(ports, target)).rejects.toThrow("description unavailable");
  expect(refreshRegistry).not.toHaveBeenCalled();
});
