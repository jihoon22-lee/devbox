import { act, cleanup, render } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { afterEach, it, vi } from "vitest";
import App from "./App";

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: vi.fn(async () => { throw new Error("native command in browser fixture"); }),
}));
afterEach(() => { cleanup(); localStorage.clear(); });
it("renders the shared feature through the standalone accessible entry", async () => {
  const { container } = render(<App />);
  await act(async () => { await vi.dynamicImportSettled(); });
  await assertNoA11yViolations(container);
});
