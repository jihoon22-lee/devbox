import { describe, expect, it, vi } from "vitest";
import { bindTypedCall } from "./typed";

type Calls = { method: "snapshot"; args: Record<string, never> } | { method: "stop"; args: { id: string } };
type Results = { snapshot: { revision: number }; stop: null };
describe("typed component transport", () => {
  it("forwards the declared method and payload without replacing operation identity", async () => {
    const transport = vi.fn().mockResolvedValue(null);
    const call = bindTypedCall<Calls, Results>(transport);
    await call("stop", { id: "owned-task" });
    expect(transport).toHaveBeenCalledExactlyOnceWith("stop", { id: "owned-task" });
  });
  it("supplies an empty object for a no-argument call and retains native failures", async () => {
    const failure = new Error("cancelled");
    const transport = vi.fn().mockRejectedValue(failure);
    const call = bindTypedCall<Calls, Results>(transport);
    await expect(call("snapshot")).rejects.toBe(failure);
    expect(transport).toHaveBeenCalledExactlyOnceWith("snapshot", {});
  });
});
