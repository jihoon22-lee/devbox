import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import LifecycleSettings from "./LifecycleSettings";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@devbox/product-shell/api", () => ({ nativeMode: true }));
vi.mock("@devbox/knowledge-features/transport", () => ({ componentInvoke: () => invoke }));
afterEach(cleanup);
it("explains background collection without a close-to-tray switch or implicit consent", async () => {
  const { container } = render(<LifecycleSettings />);
  expect(screen.getByText(/설치본은 창을 닫아도/)).toBeTruthy();
  expect(screen.getByText(/portable은 창을 닫으면/)).toBeTruthy();
  expect(screen.queryByRole("checkbox")).toBeNull();
  expect(invoke).not.toHaveBeenCalled();
  await assertNoA11yViolations(container);
});
