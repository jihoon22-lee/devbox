import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
const fixture = vi.hoisted(() => ({ background: true, policy: "keep-listening", call: vi.fn() }));
vi.mock("@devbox/product-shell/api", () => ({ nativeMode: true }));
vi.mock("@devbox/api-studio-features/calls", () => ({ webhookCall: fixture.call }));
import { ListenerControls } from "./ListenerControls";
afterEach(cleanup);
beforeEach(() => {
  fixture.background = true;
  fixture.policy = "keep-listening";
  fixture.call.mockReset().mockImplementation(async (method: string, args: { policy?: string }) => {
    if (method === "set_close_policy") {
      fixture.policy = args.policy ?? fixture.policy;
      return;
    }
    return {
      policy: fixture.policy,
      backgroundAvailable: fixture.background,
      trayAvailable: false,
      running: true,
      closing: false,
      stopFailed: false,
      settingsWritable: true,
    };
  });
});
it("keeps the installed listener without hiding a product window and exposes an explicit stop policy", async () => {
  const { container } = render(<ListenerControls />);
  const option = await screen.findByRole("option", { name: "닫아도 계속 듣기 (기본)" });
  await waitFor(() => expect((screen.getByRole("combobox") as HTMLSelectElement).disabled).toBe(false));
  expect((option as HTMLOptionElement).disabled).toBe(false);
  expect(screen.queryByRole("button", { name: /숨기기/ })).toBeNull();
  fireEvent.change(screen.getByRole("combobox"), { target: { value: "stop-on-close" } });
  await waitFor(() => expect(fixture.call).toHaveBeenCalledWith("set_close_policy", { policy: "stop-on-close" }));
  await assertNoA11yViolations(container);
});
it("explains portable ownership and keeps its background option unavailable", async () => {
  fixture.background = false;
  render(<ListenerControls />);
  await screen.findByText(/portable에서는 창을 닫으면 리스너도 중지됩니다/);
  expect((screen.getByRole("option", { name: "닫아도 계속 듣기 (기본)" }) as HTMLOptionElement).disabled).toBe(true);
});
