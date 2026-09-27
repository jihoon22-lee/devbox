import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { findA11yViolations } from "@devbox/a11y/testing";
import { toolsCall } from "@devbox/control-center-features/calls";
import AgentAutostart from "./AgentAutostart";
vi.mock("@devbox/control-center-features/calls", () => ({ toolsCall: vi.fn() }));
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});
it("reads and changes the shared preference without enabling it merely by visiting", async () => {
  vi.mocked(toolsCall).mockResolvedValue({ supported: true, enabled: false });
  const { container } = render(<AgentAutostart />);
  const toggle = await screen.findByRole("checkbox", { name: "로그인할 때 백그라운드 서비스 시작" });
  expect(toolsCall).toHaveBeenCalledTimes(1);
  expect(toolsCall).toHaveBeenCalledWith("autostart_status", {});
  vi.mocked(toolsCall).mockResolvedValue({ supported: true, enabled: true });
  fireEvent.click(toggle);
  await waitFor(() => expect((toggle as HTMLInputElement).checked).toBe(true));
  expect(toolsCall).toHaveBeenLastCalledWith("set_autostart", { enabled: true });
  expect(await findA11yViolations(container)).toEqual([]);
});
it("keeps a failed write visible without claiming the preference changed", async () => {
  vi.mocked(toolsCall)
    .mockResolvedValueOnce({ supported: true, enabled: false })
    .mockRejectedValue(new Error("설정을 저장할 수 없습니다."));
  render(<AgentAutostart />);
  const toggle = await screen.findByRole("checkbox");
  fireEvent.click(toggle);
  expect((await screen.findByRole("alert")).textContent).toContain("설정을 저장할 수 없습니다.");
  expect((toggle as HTMLInputElement).checked).toBe(false);
});
