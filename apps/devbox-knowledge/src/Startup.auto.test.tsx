import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@devbox/product-shell/api", () => ({ nativeMode: true }));
vi.mock("@devbox/knowledge-features/transport", () => ({ componentInvoke: () => native.invoke }));
import { Startup } from "./Startup";
afterEach(() => {
  cleanup();
  native.invoke.mockReset();
});
it.each([false, true])("automatically prepares a store, existing=%s", async (existing) => {
  native.invoke.mockImplementation(async (method: string) =>
    method === "status" ? { active: false, hasExisting: existing } : { active: true },
  );
  render(
    <Startup>
      <p>notes ready</p>
    </Startup>,
  );
  await screen.findByText("notes ready");
  expect(native.invoke).toHaveBeenCalledWith(existing ? "continue_existing" : "start_empty", {});
  expect(native.invoke).toHaveBeenCalledTimes(2);
});
it("keeps a failed start idle until an explicit retry", async () => {
  native.invoke.mockImplementation(async (method: string) => {
    if (method === "status") return { active: false, hasExisting: false };
    throw new Error("저장소를 준비하지 못했습니다.");
  });
  render(
    <Startup>
      <p>notes ready</p>
    </Startup>,
  );
  expect((await screen.findByRole("alert")).textContent).toContain("저장소를 준비하지 못했습니다.");
  await waitFor(() => expect(native.invoke).toHaveBeenCalledTimes(2));
  native.invoke.mockResolvedValueOnce({ active: true });
  fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await screen.findByText("notes ready");
  expect(native.invoke).toHaveBeenCalledTimes(3);
});
it("accepts prepared readiness before Suite activation", async () => {
  native.invoke.mockResolvedValueOnce({ active: false }).mockResolvedValueOnce({ active: false, prepared: true });
  render(
    <Startup>
      <p>prepared</p>
    </Startup>,
  );
  await screen.findByText("prepared");
});
