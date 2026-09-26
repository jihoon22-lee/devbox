import { beforeEach, expect, it, vi } from "vitest";

beforeEach(() => vi.resetModules());

it("keeps the first transport when the same installation initializes again", async () => {
  const transport = await import("./transport");
  const first = vi.fn().mockResolvedValue("first");
  const replay = vi.fn().mockResolvedValue("replay");
  transport.configureProductTransport(first, "installation-a");
  expect(() => transport.configureProductTransport(replay, "installation-a")).not.toThrow();
  expect(await transport.componentInvoke("workspace.files")("load_session")).toBe("first");
  expect(replay).not.toHaveBeenCalled();
  expect(transport.productInstallationId()).toBe("installation-a");
});

it("rejects a different installation without replacing the original binding", async () => {
  const transport = await import("./transport");
  const first = vi.fn().mockResolvedValue("first");
  const replacement = vi.fn();
  transport.configureProductTransport(first, "installation-a");
  expect(() => transport.configureProductTransport(replacement, "installation-b")).toThrow();
  expect(await transport.componentInvoke("workspace.files")("load_session")).toBe("first");
  expect(transport.productInstallationId()).toBe("installation-a");
  expect(replacement).not.toHaveBeenCalled();
});
