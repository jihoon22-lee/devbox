import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import LifecycleSettings from "./LifecycleSettings";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@devbox/product-shell/api", () => ({ nativeMode: true }));
vi.mock("@devbox/knowledge-features/transport", () => ({ componentInvoke: () => invoke }));
afterEach(cleanup);
beforeEach(() => { invoke.mockReset(); });
it("keeps the saved close policy when persistence fails and never starts collection", async () => {
  invoke.mockImplementation((method: string) => method === "get_close_policy"
    ? Promise.resolve({ closeToTray: false, trayAvailable: true })
    : Promise.reject(new Error("설정을 저장하지 못했습니다.")));
  render(<LifecycleSettings/>);
  const checkbox = screen.getByRole("checkbox", { name: "창을 닫을 때 트레이로 숨기기" }) as HTMLInputElement;
  await waitFor(() => expect(checkbox.disabled).toBe(false));
  expect(checkbox.checked).toBe(false);
  fireEvent.click(checkbox);
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "설정을 저장하지 못했습니다.");
  expect(checkbox.checked).toBe(false);
  expect(invoke.mock.calls).toEqual([["get_close_policy"], ["set_close_policy", { closeToTray: true }]]);
});
it("prevents enabling an unavailable tray and explains actual close behavior", async () => {
  invoke.mockResolvedValue({ closeToTray: false, trayAvailable: false });
  render(<LifecycleSettings/>);
  expect(await screen.findByText("트레이를 사용할 수 없어 창을 닫으면 앱이 종료됩니다.")).toBeTruthy();
  const checkbox = screen.getByRole("checkbox") as HTMLInputElement;
  expect(checkbox.disabled).toBe(true);
  expect(invoke.mock.calls).toEqual([["get_close_policy"]]);
});
