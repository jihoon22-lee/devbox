import { afterEach, expect, it, vi } from "vitest";
import { confirmAction } from "@devbox/product-shell/confirm";
import { confirmDevSetupApply, DEV_SETUP_CONFIGURATION_EXPIRED } from "./confirmDevSetupApply";
vi.mock("@devbox/product-shell/confirm", () => ({ confirmAction: vi.fn() }));
afterEach(() => vi.restoreAllMocks());
const review = { expiresAtMs: 200, packages: [], canApply: true, hasChanges: true };
it("cancelled confirmation does not authorize package application", async () => {
  vi.spyOn(Date, "now").mockReturnValue(100);
  vi.mocked(confirmAction).mockResolvedValue(false);
  expect(await confirmDevSetupApply(review, vi.fn())).toBe(false);
});
it("an accepted review that expired during confirmation is rejected", async () => {
  let decide!: (value: boolean) => void;
  let now = 100;
  vi.spyOn(Date, "now").mockImplementation(() => now);
  vi.mocked(confirmAction).mockReturnValue(
    new Promise((resolve) => {
      decide = resolve;
    }),
  );
  const issue = vi.fn();
  const pending = confirmDevSetupApply(review, issue);
  now = 200;
  decide(true);
  expect(await pending).toBe(false);
  expect(issue).toHaveBeenCalledWith(DEV_SETUP_CONFIGURATION_EXPIRED);
});
it("accepted unexpired review authorizes the already acknowledged application", async () => {
  vi.spyOn(Date, "now").mockReturnValue(100);
  vi.mocked(confirmAction).mockResolvedValue(true);
  expect(await confirmDevSetupApply(review, vi.fn())).toBe(true);
});
